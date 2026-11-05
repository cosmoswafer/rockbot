//! Large-file WebDAV editing tool.
//!
//! Exposes windowed reads, streamed grep, unique literal edits, line
//! insert/replace, and multi-hunk patches over the room's WebDAV space. Small
//! files use bounded read-modify-write; files above `max_edit_bytes` stream
//! through the Nextcloud chunked-upload API so they are never buffered whole.
//!
//! DFD: `_dfd/tools/webdav/main-path.md`, `_dfd/tools/webdav/large-file-edit.md`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::RwLock;

use async_trait::async_trait;
use regex::RegexBuilder;
use serde::Deserialize;
use tracing::debug;
use webdav::{FileStat, LineIndex, LineOp, LineWindow, WebDavClient, WebDavConfig, WebDavPath};

use crate::error::{Result, RockBotError};
use crate::tool::Tool;
use crate::validated::NonEmptyString;

/// Limits mirrored from `[webdav]` in the config.
#[derive(Debug, Clone)]
pub struct WebDavEditLimits {
    pub stream_threshold_bytes: u64,
    pub max_edit_bytes: u64,
    pub chunk_size_bytes: usize,
    pub read_default_lines: usize,
    pub read_max_lines: usize,
    pub read_max_bytes: usize,
    pub read_max_line_chars: usize,
    pub index_checkpoint_lines: u64,
}

impl Default for WebDavEditLimits {
    fn default() -> Self {
        Self {
            stream_threshold_bytes: 10 * 1024 * 1024,
            max_edit_bytes: 32 * 1024 * 1024,
            chunk_size_bytes: 5 * 1024 * 1024,
            read_default_lines: 2000,
            read_max_lines: 2000,
            read_max_bytes: 65_536,
            read_max_line_chars: 2000,
            index_checkpoint_lines: 1000,
        }
    }
}

impl WebDavEditLimits {
    pub fn from_config(config: &WebDavConfig) -> Self {
        Self {
            stream_threshold_bytes: config.stream_threshold_bytes,
            max_edit_bytes: config.max_edit_bytes,
            chunk_size_bytes: config.chunk_size_bytes,
            read_default_lines: config.read_default_lines,
            read_max_lines: config.read_max_lines,
            read_max_bytes: config.read_max_bytes,
            read_max_line_chars: config.read_max_line_chars,
            index_checkpoint_lines: config.index_checkpoint_lines,
        }
    }
}

#[derive(Debug, Deserialize)]
struct WebDavEditParams {
    action: NonEmptyString,
    path: String,
    #[serde(default)]
    room_id: Option<String>,
    #[serde(default)]
    webdav_dir: Option<String>,
    #[serde(default)]
    offset: Option<u64>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default, rename = "byteOffset")]
    byte_offset: Option<u64>,
    #[serde(default, rename = "byteLimit")]
    byte_limit: Option<u64>,
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default, rename = "ignoreCase")]
    ignore_case: Option<bool>,
    #[serde(default)]
    context: Option<usize>,
    #[serde(default, rename = "maxMatches")]
    max_matches: Option<usize>,
    #[serde(default, rename = "oldString")]
    old_string: Option<String>,
    #[serde(default, rename = "newString")]
    new_string: Option<String>,
    #[serde(default, rename = "replaceAll")]
    replace_all: Option<bool>,
    #[serde(default, rename = "expectedEtag")]
    expected_etag: Option<String>,
    #[serde(default)]
    line: Option<u64>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default, rename = "startLine")]
    start_line: Option<u64>,
    #[serde(default, rename = "endLine")]
    end_line: Option<u64>,
    #[serde(default)]
    hunks: Option<Vec<Hunk>>,
}

#[derive(Debug, Clone, Deserialize)]
struct Hunk {
    #[serde(rename = "oldString")]
    old_string: String,
    #[serde(rename = "newString")]
    new_string: String,
}

pub struct WebDavEditTool {
    client: WebDavClient,
    limits: WebDavEditLimits,
    index_cache: RwLock<HashMap<String, LineIndex>>,
    observed: RwLock<HashMap<String, String>>,
}

impl WebDavEditTool {
    pub fn new(client: WebDavClient, limits: WebDavEditLimits) -> Self {
        Self {
            client,
            limits,
            index_cache: RwLock::new(HashMap::new()),
            observed: RwLock::new(HashMap::new()),
        }
    }

    fn room_path(&self, dir_key: &str, subpath: &str) -> Result<String> {
        WebDavPath::new("")
            .room_path(dir_key, subpath)
            .map_err(|e| err(format!("Path traversal rejected: {e}")))
    }

    fn cache_key(full: &str, stat: &FileStat) -> String {
        format!(
            "{full}|{}|{}",
            stat.etag.as_deref().unwrap_or(""),
            stat.size
        )
    }

    fn remember(&self, full: &str, stat: &FileStat) {
        let Some(etag) = stat.etag.as_deref() else {
            return;
        };
        let mut map = self
            .observed
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        map.insert(full.to_string(), etag.to_string());
    }

    fn observed_etag(&self, full: &str) -> Option<String> {
        self.observed
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(full)
            .cloned()
    }

    async fn index_for(&self, full: &str, stat: &FileStat) -> Result<LineIndex> {
        let key = Self::cache_key(full, stat);
        if let Some(index) = self
            .index_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&key)
            .cloned()
        {
            return Ok(index);
        }

        let index = self
            .client
            .build_line_index(full, self.limits.index_checkpoint_lines)
            .await
            .map_err(|e| map_dav_error(e, full))?;

        let mut cache = self
            .index_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if cache.len() >= 64 {
            cache.clear();
        }
        cache.insert(key, index.clone());
        Ok(index)
    }

    async fn ensure_text(&self, full: &str, stat: &FileStat) -> Result<()> {
        if is_text_extension(full)
            || stat.content_type.as_deref().is_some_and(|content_type| {
                content_type.starts_with("text/")
                    || content_type.contains("xml")
                    || content_type.contains("json")
            })
        {
            return Ok(());
        }

        let probe = self
            .client
            .read_range(full, 0, 8192)
            .await
            .map_err(|e| map_dav_error(e, full))?;
        let has_nul = probe.bytes.contains(&0);
        let invalid = match std::str::from_utf8(&probe.bytes) {
            Ok(_) => false,
            Err(e) => e.valid_up_to() + 4 < probe.bytes.len(),
        };
        if has_nul || invalid {
            return Err(err(format!(
                "{full} is binary; webdav_edit handles UTF-8 text files only"
            )));
        }
        Ok(())
    }

    async fn do_stat(&self, full: &str) -> Result<String> {
        let stat = self
            .client
            .stat(full)
            .await
            .map_err(|e| map_dav_error(e, full))?;
        self.remember(full, &stat);
        Ok(format!(
            "<path>{full}</path>\n<type>{}</type>\n<size>{}</size>\n<etag>{}</etag>\n<contentType>{}</contentType>\n<lastModified>{}</lastModified>\n",
            if stat.is_dir { "directory" } else { "file" },
            stat.size,
            stat.etag.as_deref().unwrap_or("-"),
            stat.content_type.as_deref().unwrap_or("-"),
            stat.last_modified.as_deref().unwrap_or("-"),
        ))
    }

    async fn do_read(&self, full: &str, params: &WebDavEditParams) -> Result<String> {
        let stat = self
            .client
            .stat(full)
            .await
            .map_err(|e| map_dav_error(e, full))?;
        if stat.is_dir {
            return Err(err(format!("{full} is a directory")));
        }
        self.ensure_text(full, &stat).await?;
        self.remember(full, &stat);

        if let Some(byte_offset) = params.byte_offset {
            let len = params
                .byte_limit
                .unwrap_or(self.limits.read_max_bytes as u64)
                .clamp(1, self.limits.read_max_bytes as u64 * 8);
            let window = self
                .client
                .read_byte_window(full, byte_offset, len)
                .await
                .map_err(|e| map_dav_error(e, full))?;
            return Ok(render_byte_window(full, &stat, &window));
        }

        let start = params.offset.unwrap_or(1).max(1);
        let limit = params
            .limit
            .unwrap_or(self.limits.read_default_lines)
            .clamp(1, self.limits.read_max_lines.max(1));

        let index = if stat.size >= self.limits.stream_threshold_bytes {
            Some(self.index_for(full, &stat).await?)
        } else {
            None
        };

        let window = match &index {
            Some(index) if start <= index.total_lines.max(1) => {
                let (byte_start, first_line) = index.checkpoint(start);
                self.client
                    .read_window_from(
                        full,
                        byte_start,
                        first_line,
                        start,
                        limit,
                        self.limits.read_max_bytes,
                        self.limits.read_max_line_chars,
                    )
                    .await
                    .map_err(|e| map_dav_error(e, full))?
            }
            Some(_) => LineWindow {
                lines: Vec::new(),
                next_line: start,
                reached_eof: true,
                truncated: false,
            },
            None => self
                .client
                .read_window(
                    full,
                    start,
                    limit,
                    self.limits.read_max_bytes,
                    self.limits.read_max_line_chars,
                )
                .await
                .map_err(|e| map_dav_error(e, full))?,
        };

        let total = index.as_ref().map(|index| index.total_lines);
        Ok(render_line_window(full, &stat, start, &window, total))
    }

    async fn do_grep(&self, full: &str, params: &WebDavEditParams) -> Result<String> {
        let stat = self
            .client
            .stat(full)
            .await
            .map_err(|e| map_dav_error(e, full))?;
        if stat.is_dir {
            return Err(err(format!("{full} is a directory")));
        }
        self.ensure_text(full, &stat).await?;
        self.remember(full, &stat);

        let pattern = params
            .pattern
            .as_deref()
            .ok_or_else(|| err("grep requires 'pattern'"))?;
        let regex = RegexBuilder::new(pattern)
            .case_insensitive(params.ignore_case.unwrap_or(false))
            .size_limit(1 << 20)
            .build()
            .map_err(|e| err(format!("invalid regex: {e}")))?;

        let context = params.context.unwrap_or(0).min(3);
        let max_matches = params.max_matches.unwrap_or(50).clamp(1, 200);

        let mut out: Vec<String> = Vec::new();
        let mut previous: VecDeque<(u64, String)> = VecDeque::new();
        let mut printed: HashSet<u64> = HashSet::new();
        let mut after_left = 0usize;
        let mut shown = 0usize;

        self.client
            .for_each_line(full, |number, line| {
                let is_match = regex.is_match(line);
                if is_match && shown < max_matches {
                    for (prev_number, prev_text) in &previous {
                        if printed.insert(*prev_number) {
                            out.push(format!("{prev_number}: {prev_text}"));
                        }
                    }
                    if printed.insert(number) {
                        out.push(format!("{number}: {line}"));
                    }
                    shown += 1;
                    after_left = context;
                } else if after_left > 0 {
                    if printed.insert(number) {
                        out.push(format!("{number}: {line}"));
                    }
                    after_left -= 1;
                }

                previous.push_back((number, line.to_string()));
                if previous.len() > context {
                    previous.pop_front();
                }
                shown < max_matches
            })
            .await
            .map_err(|e| map_dav_error(e, full))?;

        if shown == 0 {
            return Ok(format!("No matches for /{pattern}/ in {full}."));
        }
        let mut result = format!(
            "Matches for /{pattern}/ in {full} (showing {shown}{}):\n\n",
            if shown >= max_matches {
                ", scan stopped at maxMatches"
            } else {
                ""
            }
        );
        for line in out {
            result.push_str(&line);
            result.push('\n');
        }
        Ok(result)
    }

    async fn do_edit(&self, full: &str, params: &WebDavEditParams) -> Result<String> {
        let old = params
            .old_string
            .as_deref()
            .ok_or_else(|| err("edit requires 'oldString'"))?;
        let new = params
            .new_string
            .as_deref()
            .ok_or_else(|| err("edit requires 'newString'"))?;
        if old.is_empty() {
            return Err(err("oldString must not be empty"));
        }
        if old == new {
            return Err(err("oldString and newString must differ"));
        }
        let replace_all = params.replace_all.unwrap_or(false);

        let stat = self
            .client
            .stat(full)
            .await
            .map_err(|e| map_dav_error(e, full))?;
        if stat.is_dir {
            return Err(err(format!("{full} is a directory")));
        }
        self.ensure_text(full, &stat).await?;
        self.remember(full, &stat);
        check_expected(full, &stat, params.expected_etag.as_deref())?;

        if stat.size > self.limits.max_edit_bytes {
            return self
                .large_edit(
                    full,
                    &stat,
                    old,
                    new,
                    replace_all,
                    params.expected_etag.as_deref(),
                )
                .await;
        }

        let content = self
            .client
            .read_file_to_string(full)
            .await
            .map_err(|e| map_dav_error(e, full))?;
        let count = content.matches(old).count();
        match (count, replace_all) {
            (0, _) => {
                return Err(err(format!(
                    "oldString not found in {full} — re-read the file and retry"
                )));
            }
            (n, false) if n > 1 => {
                let lines = match_line_numbers(&content, old);
                return Err(err(format!(
                    "oldString matched {n} times in {full} at lines {lines:?}; add surrounding \
                     context to make it unique, or use action=insert / replace_lines"
                )));
            }
            _ => {}
        }

        let updated = if replace_all {
            content.replace(old, new)
        } else {
            content.replacen(old, new, 1)
        };
        let written = updated.len();
        self.guarded_write(full, &stat, updated, params.expected_etag.as_deref())
            .await?;

        Ok(format!(
            "Edited {full}: replaced {count} occurrence(s) ({written} bytes written)"
        ))
    }

    async fn large_edit(
        &self,
        full: &str,
        stat: &FileStat,
        old: &str,
        new: &str,
        replace_all: bool,
        expected_etag: Option<&str>,
    ) -> Result<String> {
        let etag = expected_etag.or(stat.etag.as_deref());
        let matches = self
            .client
            .rewrite_literal(
                full,
                old,
                new,
                replace_all,
                self.limits.chunk_size_bytes,
                etag,
            )
            .await
            .map_err(|e| map_dav_error(e, full))?;
        if matches == 0 {
            return Err(err(format!(
                "oldString not found in {full} — re-read the file and retry"
            )));
        }
        if let Ok(new_stat) = self.client.stat(full).await {
            self.remember(full, &new_stat);
        }
        Ok(format!(
            "Edited {full}: replaced {matches} occurrence(s) via streaming rewrite ({} bytes source)",
            stat.size
        ))
    }

    async fn do_insert(&self, full: &str, params: &WebDavEditParams) -> Result<String> {
        let line = params.line.ok_or_else(|| err("insert requires 'line'"))?;
        let text = params
            .text
            .as_deref()
            .ok_or_else(|| err("insert requires 'text'"))?;
        if line == 0 {
            return Err(err("line must be >= 1"));
        }

        let stat = self
            .client
            .stat(full)
            .await
            .map_err(|e| map_dav_error(e, full))?;
        if stat.is_dir {
            return Err(err(format!("{full} is a directory")));
        }
        self.ensure_text(full, &stat).await?;
        self.remember(full, &stat);
        check_expected(full, &stat, params.expected_etag.as_deref())?;

        if stat.size > self.limits.max_edit_bytes {
            self.client
                .rewrite_lines(
                    full,
                    LineOp::InsertAfter {
                        line,
                        text: text.to_string(),
                    },
                    self.limits.chunk_size_bytes,
                    params.expected_etag.as_deref().or(stat.etag.as_deref()),
                )
                .await
                .map_err(|e| map_dav_error(e, full))?;
        } else {
            let content = self
                .client
                .read_file_to_string(full)
                .await
                .map_err(|e| map_dav_error(e, full))?;
            let updated = insert_after_line(&content, line, text)
                .ok_or_else(|| err(format!("line {line} is out of range for {full}")))?;
            self.guarded_write(full, &stat, updated, params.expected_etag.as_deref())
                .await?;
        }

        if let Ok(new_stat) = self.client.stat(full).await {
            self.remember(full, &new_stat);
        }
        Ok(format!(
            "Inserted {} byte(s) after line {line} in {full}",
            text.len()
        ))
    }

    async fn do_replace_lines(&self, full: &str, params: &WebDavEditParams) -> Result<String> {
        let start = params
            .start_line
            .ok_or_else(|| err("replace_lines requires 'startLine'"))?;
        let end = params
            .end_line
            .ok_or_else(|| err("replace_lines requires 'endLine'"))?;
        let text = params
            .text
            .as_deref()
            .ok_or_else(|| err("replace_lines requires 'text'"))?;
        if start == 0 || end < start {
            return Err(err("invalid line range"));
        }

        let stat = self
            .client
            .stat(full)
            .await
            .map_err(|e| map_dav_error(e, full))?;
        if stat.is_dir {
            return Err(err(format!("{full} is a directory")));
        }
        self.ensure_text(full, &stat).await?;
        self.remember(full, &stat);
        check_expected(full, &stat, params.expected_etag.as_deref())?;

        if stat.size > self.limits.max_edit_bytes {
            self.client
                .rewrite_lines(
                    full,
                    LineOp::ReplaceRange {
                        start,
                        end,
                        text: text.to_string(),
                    },
                    self.limits.chunk_size_bytes,
                    params.expected_etag.as_deref().or(stat.etag.as_deref()),
                )
                .await
                .map_err(|e| map_dav_error(e, full))?;
        } else {
            let content = self
                .client
                .read_file_to_string(full)
                .await
                .map_err(|e| map_dav_error(e, full))?;
            let updated = replace_line_range(&content, start, end, text)
                .ok_or_else(|| err(format!("lines {start}-{end} are out of range for {full}")))?;
            self.guarded_write(full, &stat, updated, params.expected_etag.as_deref())
                .await?;
        }

        if let Ok(new_stat) = self.client.stat(full).await {
            self.remember(full, &new_stat);
        }
        Ok(format!("Replaced lines {start}-{end} in {full}"))
    }

    async fn do_patch(&self, full: &str, params: &WebDavEditParams) -> Result<String> {
        let hunks = params
            .hunks
            .as_ref()
            .filter(|hunks| !hunks.is_empty())
            .ok_or_else(|| err("patch requires a non-empty 'hunks' array"))?;

        let stat = self
            .client
            .stat(full)
            .await
            .map_err(|e| map_dav_error(e, full))?;
        if stat.is_dir {
            return Err(err(format!("{full} is a directory")));
        }
        self.ensure_text(full, &stat).await?;
        self.remember(full, &stat);
        check_expected(full, &stat, params.expected_etag.as_deref())?;

        if stat.size > self.limits.max_edit_bytes {
            // Large files: one streaming rewrite per hunk. Correct and bounded,
            // at the cost of one pass per hunk; the model should prefer
            // replace_lines for adjacent changes.
            let mut applied = 0usize;
            for (index, hunk) in hunks.iter().enumerate() {
                let etag = if index == 0 {
                    params.expected_etag.as_deref().or(stat.etag.as_deref())
                } else {
                    None
                };
                let matches = self
                    .client
                    .rewrite_literal(
                        full,
                        &hunk.old_string,
                        &hunk.new_string,
                        false,
                        self.limits.chunk_size_bytes,
                        etag,
                    )
                    .await
                    .map_err(|e| map_dav_error(e, full))?;
                if matches == 0 {
                    return Err(err(format!(
                        "patch hunk {} oldString not found in {full}; {} hunk(s) already applied",
                        index + 1,
                        applied
                    )));
                }
                applied += 1;
            }
            if let Ok(new_stat) = self.client.stat(full).await {
                self.remember(full, &new_stat);
            }
            return Ok(format!(
                "Patched {full}: {applied} hunk(s) applied via streaming rewrites"
            ));
        }

        let content = self
            .client
            .read_file_to_string(full)
            .await
            .map_err(|e| map_dav_error(e, full))?;
        let updated = apply_hunks(&content, hunks)?;
        self.guarded_write(full, &stat, updated, params.expected_etag.as_deref())
            .await?;

        if let Ok(new_stat) = self.client.stat(full).await {
            self.remember(full, &new_stat);
        }
        Ok(format!("Patched {full}: {} hunk(s) applied", hunks.len()))
    }

    async fn guarded_write(
        &self,
        full: &str,
        stat: &FileStat,
        content: String,
        expected_etag: Option<&str>,
    ) -> Result<()> {
        let etag = expected_etag
            .map(str::to_string)
            .or_else(|| stat.etag.clone())
            .or_else(|| self.observed_etag(full));

        match etag {
            Some(etag) => self
                .client
                .write_file_if_match(full, content, &etag)
                .await
                .map_err(|e| map_dav_error(e, full)),
            None => self
                .client
                .write_file_atomic(full, content)
                .await
                .map_err(|e| map_dav_error(e, full)),
        }
    }
}

#[async_trait]
impl Tool for WebDavEditTool {
    fn name(&self) -> &str {
        "webdav_edit"
    }

    fn description(&self) -> &str {
        "Read and edit large UTF-8 text files (SVG, XML, JSON, code, logs) on the room's \
         WebDAV storage without loading them whole. Use 'stat' for size/etag, 'read' with \
         offset/limit for a line window (byteOffset/byteLimit for single-line files), \
         'grep' to find anchors, then 'edit' (unique literal replace), 'insert' after a \
         line, 'replace_lines' for a range, or 'patch' for several hunks in one call. \
         Always read before editing; edits are guarded by the file etag. For whole-file \
         writes, listing, mkdir, delete and rename use the 'webdav' tool."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["stat", "read", "grep", "edit", "insert", "replace_lines", "patch"],
                    "description": "Large-file operation"
                },
                "room_id": {
                    "type": "string",
                    "description": "Room ID for scoping the operation (injected automatically if omitted)"
                },
                "path": {
                    "type": "string",
                    "description": "File path relative to the room root"
                },
                "offset": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "1-based first line to read (action=read)"
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 2000,
                    "description": "Maximum lines to return (action=read)"
                },
                "byteOffset": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Byte offset, for files with no newlines (action=read)"
                },
                "byteLimit": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum bytes to return with byteOffset (action=read)"
                },
                "pattern": {
                    "type": "string",
                    "description": "Rust regex to search for (action=grep)"
                },
                "ignoreCase": {
                    "type": "boolean",
                    "description": "Case-insensitive grep"
                },
                "context": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 3,
                    "description": "Context lines around grep matches"
                },
                "maxMatches": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 200,
                    "description": "Maximum grep matches to return"
                },
                "oldString": {
                    "type": "string",
                    "description": "Exact text to replace (action=edit/patch); must be unique unless replaceAll"
                },
                "newString": {
                    "type": "string",
                    "description": "Replacement text (action=edit/patch)"
                },
                "replaceAll": {
                    "type": "boolean",
                    "description": "Replace every match instead of requiring a unique one"
                },
                "expectedEtag": {
                    "type": "string",
                    "description": "Optional etag guard; fails if the file changed since it was read"
                },
                "line": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Insert after this 1-based line (action=insert)"
                },
                "text": {
                    "type": "string",
                    "description": "Text to insert or use as the replacement block"
                },
                "startLine": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "First line to replace (action=replace_lines)"
                },
                "endLine": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Last line to replace, inclusive (action=replace_lines)"
                },
                "hunks": {
                    "type": "array",
                    "description": "Ordered literal replacements (action=patch)",
                    "items": {
                        "type": "object",
                        "properties": {
                            "oldString": { "type": "string" },
                            "newString": { "type": "string" }
                        },
                        "required": ["oldString", "newString"]
                    }
                }
            },
            "required": ["action", "path"]
        })
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let params: WebDavEditParams = serde_json::from_str(arguments).map_err(|e| {
            RockBotError::ToolCallParse(format!("Failed to parse webdav_edit arguments: {e}"))
        })?;

        let room_id = params.room_id.as_deref().unwrap_or("unknown");
        let webdav_dir = params.webdav_dir.as_deref().unwrap_or(room_id);
        let full = self.room_path(webdav_dir, &params.path)?;
        debug!(
            "webdav_edit: action={} path={}",
            params.action.as_str(),
            full
        );

        match params.action.as_str() {
            "stat" => self.do_stat(&full).await,
            "read" => self.do_read(&full, &params).await,
            "grep" => self.do_grep(&full, &params).await,
            "edit" => self.do_edit(&full, &params).await,
            "insert" => self.do_insert(&full, &params).await,
            "replace_lines" => self.do_replace_lines(&full, &params).await,
            "patch" => self.do_patch(&full, &params).await,
            other => Err(err(format!(
                "Unknown webdav_edit action: {other}. Valid: stat, read, grep, edit, insert, replace_lines, patch"
            ))),
        }
    }
}

fn err(message: impl Into<String>) -> RockBotError {
    RockBotError::ToolCallParse(message.into())
}

fn check_expected(full: &str, stat: &FileStat, expected: Option<&str>) -> Result<()> {
    if let Some(expected) = expected {
        if stat.etag.as_deref() != Some(expected) {
            return Err(err(format!(
                "cannot modify {full}: file changed since it was read (expected {expected}, \
                 server has {}) — re-read the file, then retry",
                stat.etag.as_deref().unwrap_or("<none>")
            )));
        }
    }
    Ok(())
}

fn map_dav_error(error: webdav::WebDavError, full: &str) -> RockBotError {
    match error {
        webdav::WebDavError::PreconditionFailed(detail) => err(format!(
            "cannot modify {full}: file changed since it was read ({detail}) — re-read the file, then retry"
        )),
        webdav::WebDavError::ChunkedUploadUnsupported => err(format!(
            "{full} is above max_edit_bytes and this WebDAV endpoint does not support Nextcloud \
             chunked upload; use insert/replace_lines or move the file to a Nextcloud instance"
        )),
        other => RockBotError::Provider(format!("WebDAV: {other}")),
    }
}

fn is_text_extension(path: &str) -> bool {
    const TEXT_EXTENSIONS: &[&str] = &[
        ".svg",
        ".xml",
        ".json",
        ".jsonl",
        ".yaml",
        ".yml",
        ".toml",
        ".csv",
        ".tsv",
        ".md",
        ".txt",
        ".rs",
        ".py",
        ".js",
        ".mjs",
        ".cjs",
        ".ts",
        ".tsx",
        ".jsx",
        ".html",
        ".htm",
        ".css",
        ".scss",
        ".ics",
        ".log",
        ".sh",
        ".bash",
        ".zsh",
        ".sql",
        ".ini",
        ".cfg",
        ".conf",
        ".env",
        ".gitignore",
        ".dockerfile",
        ".lock",
    ];
    let lower = path.to_ascii_lowercase();
    TEXT_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
}

fn render_line_window(
    full: &str,
    stat: &FileStat,
    start: u64,
    window: &LineWindow,
    total: Option<u64>,
) -> String {
    let mut out = format!(
        "<path>{full}</path>\n<type>{}</type>\n<size>{}</size>\n<content>\n",
        stat.content_type.as_deref().unwrap_or("text/plain"),
        stat.size
    );
    for line in &window.lines {
        out.push_str(&format!("{}: {}\n", line.number, line.text));
    }
    out.push_str("</content>\n");

    if window.lines.is_empty() {
        match total {
            Some(total) => out.push_str(&format!(
                "(No lines at offset={start}; file has {total} lines)\n"
            )),
            None => out.push_str(&format!("(No lines at offset={start})\n")),
        }
    } else if window.reached_eof {
        let end = window.lines.last().map(|line| line.number).unwrap_or(start);
        out.push_str(&format!("(End of file - total {end} lines)\n"));
    } else {
        let first = window
            .lines
            .first()
            .map(|line| line.number)
            .unwrap_or(start);
        let last = window.lines.last().map(|line| line.number).unwrap_or(first);
        match total {
            Some(total) => out.push_str(&format!(
                "(Showing lines {first}-{last} of {total}. Use offset={} to continue.)\n",
                window.next_line
            )),
            None => out.push_str(&format!(
                "(Output capped. Showing lines {first}-{last}. Use offset={} to continue.)\n",
                window.next_line
            )),
        }
    }
    out
}

fn render_byte_window(full: &str, stat: &FileStat, window: &webdav::ByteWindow) -> String {
    let end = window.start + window.bytes.len() as u64;
    let total = window
        .total
        .map(|total| total.to_string())
        .unwrap_or_else(|| "?".to_string());
    format!(
        "<path>{full}</path>\n<type>{}</type>\n<size>{}</size>\n<content>\n{}\n</content>\n\
         (Bytes {}-{} of {total}. Use byteOffset={end} to continue.)\n",
        stat.content_type.as_deref().unwrap_or("text/plain"),
        stat.size,
        String::from_utf8_lossy(&window.bytes),
        window.start,
        end,
    )
}

fn match_line_numbers(content: &str, needle: &str) -> Vec<u64> {
    content
        .match_indices(needle)
        .map(|(index, _)| {
            content[..index]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count() as u64
                + 1
        })
        .collect()
}

fn insert_after_line(content: &str, line: u64, text: &str) -> Option<String> {
    if line == 0 {
        return None;
    }
    let mut current = 1u64;
    let mut offset = None;
    for (index, byte) in content.bytes().enumerate() {
        if byte == b'\n' {
            if current == line {
                offset = Some(index + 1);
                break;
            }
            current += 1;
        }
    }
    let offset = match offset {
        Some(offset) => offset,
        None if current == line && !content.ends_with('\n') => content.len(),
        None => return None,
    };

    let mut out = String::with_capacity(content.len() + text.len() + 2);
    out.push_str(&content[..offset]);
    if offset == content.len() && !content.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(text);
    if !text.ends_with('\n') && offset < content.len() {
        out.push('\n');
    }
    out.push_str(&content[offset..]);
    Some(out)
}

fn replace_line_range(content: &str, start: u64, end: u64, text: &str) -> Option<String> {
    if start == 0 || end < start {
        return None;
    }
    let parts: Vec<&str> = content.split_inclusive('\n').collect();
    if start as usize > parts.len() {
        return None;
    }
    let end_index = (end as usize).min(parts.len()) as u64;

    let mut out = String::with_capacity(content.len() + text.len());
    for (index, part) in parts.iter().enumerate() {
        let number = index as u64 + 1;
        if number < start || number > end_index {
            out.push_str(part);
        } else if number == start && !text.is_empty() {
            out.push_str(text);
            if part.ends_with('\n') && !text.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    Some(out)
}

fn apply_hunks(content: &str, hunks: &[Hunk]) -> Result<String> {
    let mut positions: Vec<(usize, usize, &str)> = Vec::with_capacity(hunks.len());
    for (index, hunk) in hunks.iter().enumerate() {
        if hunk.old_string.is_empty() {
            return Err(err(format!("patch hunk {} oldString is empty", index + 1)));
        }
        let mut matches = content.match_indices(&hunk.old_string);
        let position = matches
            .next()
            .map(|(position, _)| position)
            .ok_or_else(|| {
                err(format!(
                    "patch hunk {} oldString not found; re-read the file",
                    index + 1
                ))
            })?;
        if matches.next().is_some() {
            return Err(err(format!(
                "patch hunk {} oldString matched more than once; add surrounding context",
                index + 1
            )));
        }
        positions.push((position, hunk.old_string.len(), hunk.new_string.as_str()));
    }

    positions.sort_by_key(|entry| entry.0);
    for pair in positions.windows(2) {
        if pair[0].0 + pair[0].1 > pair[1].0 {
            return Err(err("patch hunks overlap; merge them into one hunk"));
        }
    }

    let mut out = String::with_capacity(content.len());
    let mut cursor = 0usize;
    for (position, old_len, new_text) in positions {
        out.push_str(&content[cursor..position]);
        out.push_str(new_text);
        cursor = position + old_len;
    }
    out.push_str(&content[cursor..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_extension_covers_svg() {
        assert!(is_text_extension("artwork/map.svg"));
        assert!(is_text_extension("notes.txt"));
        assert!(is_text_extension("data.json"));
        assert!(!is_text_extension("photo.png"));
    }

    #[test]
    fn insert_after_line_middle() {
        assert_eq!(
            insert_after_line("a\nb\nc\n", 1, "x").unwrap(),
            "a\nx\nb\nc\n"
        );
    }

    #[test]
    fn insert_after_last_line_without_newline() {
        assert_eq!(insert_after_line("a\nb", 2, "x").unwrap(), "a\nb\nx");
    }

    #[test]
    fn insert_out_of_range() {
        assert!(insert_after_line("a\nb\n", 3, "x").is_none());
    }

    #[test]
    fn replace_range_middle() {
        assert_eq!(
            replace_line_range("a\nb\nc\nd\n", 2, 3, "X\nY\n").unwrap(),
            "a\nX\nY\nd\n"
        );
    }

    #[test]
    fn replace_range_to_eof() {
        assert_eq!(
            replace_line_range("a\nb\nc\n", 2, 3, "X").unwrap(),
            "a\nX\n"
        );
    }

    #[test]
    fn patch_applies_in_file_order() {
        let hunks = vec![
            Hunk {
                old_string: "b".into(),
                new_string: "B".into(),
            },
            Hunk {
                old_string: "d".into(),
                new_string: "D".into(),
            },
        ];
        assert_eq!(apply_hunks("a\nb\nc\nd\n", &hunks).unwrap(), "a\nB\nc\nD\n");
    }

    #[test]
    fn patch_rejects_multiple_matches() {
        let hunks = vec![Hunk {
            old_string: "x".into(),
            new_string: "y".into(),
        }];
        assert!(apply_hunks("x x", &hunks).is_err());
    }

    #[test]
    fn patch_rejects_overlap() {
        let hunks = vec![
            Hunk {
                old_string: "ab".into(),
                new_string: "AB".into(),
            },
            Hunk {
                old_string: "bc".into(),
                new_string: "BC".into(),
            },
        ];
        assert!(apply_hunks("abc", &hunks).is_err());
    }

    #[test]
    fn match_lines_are_one_based() {
        assert_eq!(match_line_numbers("a\nb\nb\n", "b"), vec![2, 3]);
    }
}
