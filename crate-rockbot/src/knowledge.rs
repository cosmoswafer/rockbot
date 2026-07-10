use serde::{Deserialize, Serialize};
use serde_valid::Validate;
use std::collections::HashSet;
use tracing::{debug, warn};
use webdav::{WebDavClient, WebDavError, WebDavPath};

use crate::error::Result;
use crate::utils::now_iso_string;
use crate::validated::NonEmptyString;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[derive(Default)]
pub enum KnowledgePriority {
    #[serde(rename = "P0")]
    P0,
    #[serde(rename = "P1")]
    #[default]
    P1,
    #[serde(rename = "P2")]
    P2,
    #[serde(rename = "P3")]
    P3,
}

impl std::fmt::Display for KnowledgePriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KnowledgePriority::P0 => write!(f, "P0"),
            KnowledgePriority::P1 => write!(f, "P1"),
            KnowledgePriority::P2 => write!(f, "P2"),
            KnowledgePriority::P3 => write!(f, "P3"),
        }
    }
}


impl KnowledgePriority {}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct IndexEntry {
    #[validate(min_length = 1)]
    pub filename: String,
    #[serde(default)]
    pub when_useful: String,
    #[serde(default)]
    pub priority: KnowledgePriority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_promoted_at: Option<String>,
}

impl IndexEntry {
    pub fn display_title(&self) -> &str {
        self.filename.strip_suffix(".md").unwrap_or(&self.filename)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct KnowledgeIndex {
    #[validate(min_length = 1)]
    pub version: String,
    #[validate(min_length = 1)]
    pub room_id: String,
    #[validate]
    pub entries: Vec<IndexEntry>,
}

impl KnowledgeIndex {
    /// Format a compact one-line-per-entry summary for context injection.
    /// The AI uses this to decide which entries to recall via `recall_knowledge`.
    pub fn format_summary(&self) -> String {
        if self.entries.is_empty() {
            return String::new();
        }
        let mut lines = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            lines.push(format!(
                "[{}] {} — {}",
                entry.priority,
                entry.display_title(),
                entry.when_useful
            ));
        }
        format!(
            "[Knowledge Index — use recall_knowledge to retrieve full entries]\n{}",
            lines.join("\n")
        )
    }
}

/// Parsed tool arguments for save_knowledge — typed boundary for "parse, don't validate".
#[derive(Debug, Clone, Deserialize)]
pub struct SaveKnowledgeParams {
    pub topic: NonEmptyString,
    pub content: NonEmptyString,
    pub when_useful: NonEmptyString,
    pub priority: KnowledgePriority,
    #[serde(default)]
    pub tags: Option<String>,
    #[serde(default)]
    pub webdav_dir: Option<String>,
}

/// Parsed tool arguments for forget_knowledge.
#[derive(Debug, Clone, Deserialize)]
pub struct ForgetKnowledgeParams {
    pub topic: NonEmptyString,
    #[serde(default)]
    pub webdav_dir: Option<String>,
}

/// Parsed tool arguments for recall_knowledge.
#[derive(Debug, Clone, Deserialize)]
pub struct RecallKnowledgeParams {
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub webdav_dir: Option<String>,
}

pub struct KnowledgeManager;

impl KnowledgeManager {
    pub fn knowledge_dir(webdav_dir: &str) -> String {
        format!("{}knowledge/", WebDavPath::new("").room_dir(webdav_dir))
    }

    pub fn index_path(webdav_dir: &str) -> String {
        format!("{}index.json", Self::knowledge_dir(webdav_dir))
    }

    pub fn slugify(title: &str) -> String {
        let lower = title.to_lowercase();
        let mut result = String::with_capacity(lower.len());
        for ch in lower.chars() {
            if ch.is_alphanumeric() {
                result.push(ch);
            } else if ch == ' ' || ch == '-' || ch == '_' {
                result.push('_');
            }
        }
        if result.is_empty() {
            "untitled".to_string()
        } else {
            result.trim_matches('_').to_string()
        }
    }

    pub async fn load_index(
        webdav: &WebDavClient,
        webdav_dir: &str,
    ) -> Result<KnowledgeIndex> {
        let path = Self::index_path(webdav_dir);
        match webdav.read_file_to_string(&path).await {
            Ok(content) => {
                let index: KnowledgeIndex = serde_json::from_str(&content).map_err(|e| {
                    crate::error::RockBotError::Provider(format!("Failed to parse knowledge index: {e}"))
                })?;
                Ok(index)
            }
            Err(WebDavError::NotFound(_)) => {
                debug!("No knowledge index found for room {}, starting fresh", webdav_dir);
                Ok(KnowledgeIndex {
                    version: "rockbot-knowledge/1".into(),
                    room_id: webdav_dir.to_string(),
                    entries: Vec::new(),
                })
            }
            Err(e) => {
                Err(crate::error::RockBotError::Provider(format!(
                    "Failed to read knowledge index: {e}"
                )))
            }
        }
    }

    pub async fn save_index(
        webdav: &WebDavClient,
        webdav_dir: &str,
        index: &KnowledgeIndex,
    ) -> Result<()> {
        let path = Self::index_path(webdav_dir);
        let json = serde_json::to_vec(index).map_err(|e| {
            crate::error::RockBotError::Provider(format!("Failed to serialize knowledge index: {e}"))
        })?;
        let folder = format!("{}knowledge/", WebDavPath::new("").room_dir(webdav_dir));
        if let Err(e) = webdav.ensure_directory_all(&folder).await {
            warn!("Failed to ensure knowledge directory {}: {}", folder, e);
        }
        webdav.write_file_with_fallback(&path, json).await.map_err(|e| {
            crate::error::RockBotError::Provider(format!("Failed to write knowledge index: {e}"))
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn save_entry(
        webdav: &WebDavClient,
        webdav_dir: &str,
        topic: &str,
        content: &str,
        when_useful: &str,
        tags: &[String],
        priority: &KnowledgePriority,
    ) -> Result<()> {
        let now = now_iso_string();
        let slug = Self::slugify(topic);
        let filename = format!("{}.md", slug);

        // Update index first — the index is the source of truth
        let mut index = Self::load_index(webdav, webdav_dir).await?;
        if let Some(existing) = index.entries.iter_mut().find(|e| e.filename == filename) {
            existing.when_useful = when_useful.to_string();
        } else {
            index.entries.push(IndexEntry {
                filename: filename.clone(),
                when_useful: when_useful.to_string(),
                priority: priority.clone(),
                last_promoted_at: None,
            });
        }

        let index_body = serde_json::to_string_pretty(&index).map_err(|e| {
            crate::error::RockBotError::Provider(format!("Failed to serialize knowledge index: {e}"))
        })?;
        webdav
            .write_file_with_fallback(&Self::index_path(webdav_dir), index_body.as_bytes().to_vec())
            .await
            .map_err(|e| {
                crate::error::RockBotError::Provider(format!("Knowledge index write failed: {e}"))
            })?;

        // Write .md file after index is committed
        let md_path = format!("{}{}", Self::knowledge_dir(webdav_dir), filename);
        let folder = Self::knowledge_dir(webdav_dir);
        if let Err(e) = webdav.ensure_directory_all(&folder).await {
            warn!("Failed to ensure knowledge directory {}: {}", folder, e);
        }

        let md_body = format!(
            "# {}\n\n**When Useful:** {}\n**Tags:** {}\n**Created:** {}\n**Updated:** {}\n\n{}",
            topic, when_useful, tags.join(", "), now, now, content
        );

        webdav
            .write_file_with_fallback(&md_path, md_body.as_bytes().to_vec())
            .await
            .map_err(|e| {
                crate::error::RockBotError::Provider(format!("Knowledge write failed: {e}"))
            })?;

        debug!("Saved knowledge entry {} in room {}", filename, webdav_dir);
        Ok(())
    }

    pub async fn delete_entry(
        webdav: &WebDavClient,
        webdav_dir: &str,
        topic: &str,
    ) -> Result<()> {
        let topic_slug = Self::slugify(topic);

        let mut index = Self::load_index(webdav, webdav_dir).await?;

        let matching_entries: Vec<_> = index.entries.iter().filter(|e| {
            e.filename.to_lowercase().contains(&topic_slug)
        }).cloned().collect();

        if matching_entries.is_empty() {
            return Err(crate::error::RockBotError::ToolCallParse(format!(
                "Knowledge entry '{topic}' not found."
            )));
        }

        index.entries.retain(|e| {
            !e.filename.to_lowercase().contains(&topic_slug)
        });

        let index_body = serde_json::to_string_pretty(&index).map_err(|e| {
            crate::error::RockBotError::Provider(format!("Failed to serialize knowledge index: {e}"))
        })?;
        webdav
            .write_file_with_fallback(&Self::index_path(webdav_dir), index_body.as_bytes().to_vec())
            .await
            .map_err(|e| {
                crate::error::RockBotError::Provider(format!("Knowledge index write failed: {e}"))
            })?;

        let mut _deleted_files = 0usize;
        for entry in &matching_entries {
            let md_path = format!(
                "{}{}",
                Self::knowledge_dir(webdav_dir),
                entry.filename
            );
            match webdav.delete(&md_path).await {
                Ok(()) => _deleted_files += 1,
                Err(e) => warn!("Failed to delete knowledge file {}: {}", entry.filename, e),
            }
        }

        debug!("Deleted {} knowledge file(s) for topic '{}' in room {}", _deleted_files, topic, webdav_dir);
        Ok(())
    }

    pub fn match_relevant(
        index: &KnowledgeIndex,
        recent_messages: &[&str],
    ) -> Vec<IndexEntry> {
        let keywords: HashSet<String> = recent_messages
            .iter()
            .flat_map(|msg| {
                msg.split(|c: char| !c.is_alphanumeric())
                    .filter(|w| w.len() > 2)
                    .map(|w| w.to_lowercase())
            })
            .collect();

        if keywords.is_empty() {
            return index.entries.clone();
        }

        let mut scored: Vec<(usize, IndexEntry)> = index
            .entries
            .iter()
            .filter_map(|entry| {
                let title_lower = entry.display_title().to_lowercase();
                let when_lower = entry.when_useful.to_lowercase();
                let mut score = 0usize;
                for kw in &keywords {
                    if title_lower.contains(kw.as_str()) {
                        score += 2;
                    }
                    if when_lower.contains(kw.as_str()) {
                        score += 1;
                    }
                }
                let priority_bonus: usize = match entry.priority {
                    KnowledgePriority::P0 => 8,
                    KnowledgePriority::P1 => 5,
                    KnowledgePriority::P2 => 2,
                    KnowledgePriority::P3 => 0,
                };
                // P0 always selected regardless of keyword overlap
                if entry.priority == KnowledgePriority::P0 {
                    Some((score + priority_bonus, entry.clone()))
                } else if score > 0 {
                    Some((score + priority_bonus, entry.clone()))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        scored.into_iter().map(|(_, e)| e).collect()
    }

    pub async fn recall_entry(
        webdav: &WebDavClient,
        webdav_dir: &str,
        query: &str,
    ) -> Result<Option<String>> {
        let index = Self::load_index(webdav, webdav_dir).await?;
        if index.entries.is_empty() {
            return Ok(None);
        }

        let query_lower = query.to_lowercase();
        if query_lower.is_empty() {
            let mut content_parts = Vec::new();
            for e in &index.entries {
                if let Ok(body) =
                    Self::read_entry_md(webdav, webdav_dir, &e.filename).await
                {
                    content_parts.push(format!(
                        "[Knowledge: {}]\n{}",
                        e.display_title(), body
                    ));
                }
            }
            if content_parts.is_empty() {
                return Ok(Some("No knowledge entries found for this room.".into()));
            }
            Ok(Some(content_parts.join("\n---\n")))
        } else {
            // Use keyword scoring to find all matching entries
            let recent = &[query];
            let matching = Self::match_relevant(&index, recent);
            if matching.is_empty() {
                return Ok(Some(format!("No knowledge entry found matching '{query}'.")));
            }
            let mut content_parts = Vec::new();
            for e in &matching {
                if let Ok(body) =
                    Self::read_entry_md(webdav, webdav_dir, &e.filename).await
                {
                    content_parts.push(format!(
                        "[Knowledge: {}]\n{}",
                        e.display_title(), body
                    ));
                }
            }
            Ok(Some(content_parts.join("\n---\n")))
        }
    }

    pub async fn read_entry_md(
        webdav: &WebDavClient,
        webdav_dir: &str,
        filename: &str,
    ) -> Result<String> {
        let path = format!("{}{}", Self::knowledge_dir(webdav_dir), filename);
        webdav.read_file_to_string(&path).await.map_err(|e| {
            crate::error::RockBotError::Provider(format!("Failed to read knowledge entry: {e}"))
        })
    }

    pub async fn review_priorities(
        webdav: &WebDavClient,
        webdav_dir: &str,
        used_filenames: &[String],
    ) -> Result<bool> {
        let mut index = match Self::load_index(webdav, webdav_dir).await {
            Ok(i) => i,
            Err(_) => return Ok(false),
        };
        if index.entries.is_empty() {
            return Ok(false);
        }

        let now = now_iso_string();
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let used_set: HashSet<&str> = used_filenames.iter().map(|s| s.as_str()).collect();
        let mut changed = false;

        for entry in &mut index.entries {
            let was_priority = entry.priority.clone();
            if used_set.contains(entry.filename.as_str()) {
                // Promote one level
                entry.priority = match entry.priority {
                    KnowledgePriority::P3 => KnowledgePriority::P2,
                    KnowledgePriority::P2 => KnowledgePriority::P1,
                    KnowledgePriority::P1 => KnowledgePriority::P0,
                    KnowledgePriority::P0 => KnowledgePriority::P0,
                };
                entry.last_promoted_at = Some(now.clone());
            } else {
                // Decay based on recency
                if let Some(ref promoted_at) = entry.last_promoted_at {
                    let promoted_secs = parse_iso_to_secs(promoted_at).unwrap_or(0);
                    let days_since = (now_secs.saturating_sub(promoted_secs)) / 86400;
                    entry.priority = match (entry.priority.clone(), days_since) {
                        (KnowledgePriority::P0, d) if d >= 1 => KnowledgePriority::P1,
                        (KnowledgePriority::P1, d) if d >= 3 => KnowledgePriority::P2,
                        (KnowledgePriority::P2, d) if d >= 7 => KnowledgePriority::P3,
                        (p, _) => p,
                    };
                }
            }
            if entry.priority != was_priority {
                changed = true;
            }
        }

        if changed {
            Self::save_index(webdav, webdav_dir, &index).await?;
        }
        Ok(changed)
    }
}

fn parse_iso_to_secs(iso: &str) -> Option<u64> {
    // Parse "YYYY-MM-DD" or full ISO 8601 ("YYYY-MM-DDTHH:MM:SSZ") to epoch seconds.
    // Extract the date portion only — time precision is not needed for day-level comparisons.
    let date_part = if let Some(t_pos) = iso.find('T') {
        &iso[..t_pos]
    } else {
        iso
    };
    let parts: Vec<&str> = date_part.split('-').collect();
    if parts.len() < 3 { return None; }
    let y: i64 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    let d: u32 = parts[2].parse().ok()?;
    if m < 1 || m > 12 || d < 1 || d > 31 { return None; }
    // Use the same algorithm as old date_to_days
    let m = if m <= 2 { m + 12 } else { m };
    let y = if m > 12 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as u64;
    let doy: u64 = (153 * (m as u64 - 3) + 2) / 5 + d as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days: i64 = era * 146097 + doe as i64 - 719468;
    Some((days as u64) * 86400)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::civil_from_days;

    #[test]
    fn test_slugify_basic() {
        assert_eq!(KnowledgeManager::slugify("DB API"), "db_api");
        assert_eq!(KnowledgeManager::slugify("Hello World"), "hello_world");
        assert_eq!(KnowledgeManager::slugify("test"), "test");
    }

    #[test]
    fn test_slugify_chinese() {
        let slug = KnowledgeManager::slugify("數據庫");
        assert!(!slug.is_empty());
    }

    #[test]
    fn test_slugify_empty() {
        assert_eq!(KnowledgeManager::slugify(""), "untitled");
        assert_eq!(KnowledgeManager::slugify("..."), "untitled");
    }

    #[test]
    fn test_match_relevant_finds_by_title() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![IndexEntry {
                filename: "build.md".into(),
                when_useful: "When building cargo projects".into(),
                priority: KnowledgePriority::P1, last_promoted_at: None,
            }],
        };

        let matches =
            KnowledgeManager::match_relevant(&index, &["how do I build this cargo project"]);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].filename, "build.md");
    }

    #[test]
    fn test_match_relevant_finds_by_when_useful_keyword() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![IndexEntry {
                filename: "api.md".into(),
                when_useful: "When working with database APIs".into(),
                priority: KnowledgePriority::P1, last_promoted_at: None,
            }],
        };

        let matches =
            KnowledgeManager::match_relevant(&index, &["how do I connect to the database?"]);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].filename, "api.md");
    }

    #[test]
    fn test_match_relevant_finds_by_when_useful() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![IndexEntry {
                filename: "contact.md".into(),
                when_useful: "When someone asks about office hours or support phone numbers".into(),
                priority: KnowledgePriority::P1, last_promoted_at: None,
            }],
        };

        let matches =
            KnowledgeManager::match_relevant(&index, &["what are your office hours?"]);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].filename, "contact.md");
    }

    #[test]
    fn test_match_relevant_no_match() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![IndexEntry {
                filename: "api.md".into(),
                when_useful: "When working with REST APIs".into(),
                priority: KnowledgePriority::P1, last_promoted_at: None,
            }],
        };

        let matches = KnowledgeManager::match_relevant(
            &index,
            &["hello", "how are you", "nice weather"],
        );
        assert!(matches.is_empty());
    }

    #[test]
    fn test_match_relevant_p0_always_selected() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![IndexEntry {
                filename: "critical.md".into(),
                when_useful: "Always important".into(),
                priority: KnowledgePriority::P0, last_promoted_at: None,
            }],
        };

        let matches = KnowledgeManager::match_relevant(
            &index,
            &["hello", "how are you", "nice weather"],
        );
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].filename, "critical.md");
    }

    #[test]
    fn test_match_relevant_priority_bonus_sorting() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![
                IndexEntry {
                    filename: "p0_doc.md".into(),
                    when_useful: "When working with databases".into(),
                    priority: KnowledgePriority::P0, last_promoted_at: None,
                },
                IndexEntry {
                    filename: "p3_doc.md".into(),
                    when_useful: "When working with databases".into(),
                    priority: KnowledgePriority::P3, last_promoted_at: None,
                },
            ],
        };

        let matches = KnowledgeManager::match_relevant(
            &index,
            &["databases"],
        );
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].filename, "p0_doc.md",
            "P0 should sort before P3 when both match same keywords");
        assert_eq!(matches[1].filename, "p3_doc.md");
    }

    #[test]
    fn test_match_relevant_p1_no_keyword_match_excluded() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![IndexEntry {
                filename: "p1_item.md".into(),
                when_useful: "When configuring the server".into(),
                priority: KnowledgePriority::P1, last_promoted_at: None,
            }],
        };

        let matches = KnowledgeManager::match_relevant(
            &index,
            &["completely", "unrelated", "topic"],
        );
        assert!(matches.is_empty(),
            "P1 should not be included without keyword match");
    }

    #[test]
    fn test_knowledge_dir_path() {
        let dir = KnowledgeManager::knowledge_dir("r-general");
        assert!(dir.contains("knowledge/"));
        assert!(dir.contains("r-general"));
    }

    #[test]
    fn test_knowledge_index_path() {
        let path = KnowledgeManager::index_path("d-saru");
        assert!(path.ends_with("knowledge/index.json"));
        assert!(path.contains("d-saru"));
    }

    #[test]
    fn test_parse_iso_to_secs_date_only() {
        let secs = parse_iso_to_secs("2026-06-14");
        assert!(secs.is_some(), "should parse date-only format");
    }

    #[test]
    fn test_parse_iso_to_secs_full_iso() {
        let secs = parse_iso_to_secs("2026-06-14T12:30:45Z");
        assert!(secs.is_some(), "should parse full ISO 8601 format");
    }

    #[test]
    fn test_parse_iso_to_secs_full_iso_no_z() {
        let secs = parse_iso_to_secs("2026-06-14T12:30:45");
        assert!(secs.is_some(), "should parse ISO without Z suffix");
    }

    #[test]
    fn test_parse_iso_to_secs_same_day_roundtrip() {
        let a = parse_iso_to_secs("2026-06-14").unwrap();
        let b = parse_iso_to_secs("2026-06-14T23:59:59Z").unwrap();
        assert_eq!(a, b, "date-only and full ISO for same day must be equal");
    }

    #[test]
    fn test_parse_iso_to_secs_different_days() {
        let d1 = parse_iso_to_secs("2026-06-10").unwrap();
        let d2 = parse_iso_to_secs("2026-06-14").unwrap();
        let diff_days = (d2 - d1) / 86400;
        assert_eq!(diff_days, 4, "difference should be 4 days");
    }

    #[test]
    fn test_parse_iso_to_secs_now_roundtrip() {
        let iso = now_iso_string();
        let parsed = parse_iso_to_secs(&iso);
        assert!(parsed.is_some(), "now_iso_string() output must be parseable");
        let parsed = parsed.unwrap();
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let diff = if now_secs > parsed { now_secs - parsed } else { parsed - now_secs };
        assert!(diff < 86400, "parsed epoch must be within 1 day of now, got diff={diff}");
    }

    #[test]
    fn test_parse_iso_to_secs_invalid_formats() {
        assert!(parse_iso_to_secs("").is_none(), "empty string");
        assert!(parse_iso_to_secs("not-a-date").is_none(), "garbage");
        assert!(parse_iso_to_secs("2026-06").is_none(), "missing day");
        assert!(parse_iso_to_secs("2026-13-01").is_none(), "invalid month");
    }

    #[test]
    fn test_priority_promotion_happy_path() {
        let now = now_iso_string();
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let used_set: HashSet<&str> = ["note_test.md"].into();

        let mut entry = IndexEntry {
            filename: "note_test.md".into(),
            when_useful: "for testing".into(),
            priority: KnowledgePriority::P1,
            last_promoted_at: None,
        };

        let was = entry.priority.clone();
        if used_set.contains(entry.filename.as_str()) {
            entry.priority = match entry.priority {
                KnowledgePriority::P3 => KnowledgePriority::P2,
                KnowledgePriority::P2 => KnowledgePriority::P1,
                KnowledgePriority::P1 => KnowledgePriority::P0,
                KnowledgePriority::P0 => KnowledgePriority::P0,
            };
            entry.last_promoted_at = Some(now.clone());
        }
        assert!(entry.priority != was, "priority should change on promotion");
        assert_eq!(entry.priority, KnowledgePriority::P0);
        assert!(entry.last_promoted_at.is_some(), "timestamp should be set");
        assert_eq!(entry.last_promoted_at.unwrap(), now);

        let promoted_secs = parse_iso_to_secs(&now).unwrap();
        let days_since = (now_secs.saturating_sub(promoted_secs)) / 86400;
        assert_eq!(days_since, 0, "just-promoted entry should have 0 days since");
    }

    #[test]
    fn test_priority_decay_p0_to_p1() {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let two_days_ago_epoch = now_secs - 2 * 86400;
        let two_days_ago = civil_from_days((two_days_ago_epoch / 86400) as i64);
        let ts = format!("{:04}-{:02}-{:02}", two_days_ago.0, two_days_ago.1, two_days_ago.2);

        let mut entry = IndexEntry {
            filename: "note_stale.md".into(),
            when_useful: "stale entry".into(),
            priority: KnowledgePriority::P0,
            last_promoted_at: Some(ts),
        };

        let promoted_secs = parse_iso_to_secs(&entry.last_promoted_at.as_ref().unwrap()).unwrap();
        let days_since = (now_secs.saturating_sub(promoted_secs)) / 86400;
        assert_eq!(days_since, 2, "should be 2 days since promotion");

        entry.priority = match (entry.priority.clone(), days_since) {
            (KnowledgePriority::P0, d) if d >= 1 => KnowledgePriority::P1,
            (KnowledgePriority::P1, d) if d >= 3 => KnowledgePriority::P2,
            (KnowledgePriority::P2, d) if d >= 7 => KnowledgePriority::P3,
            (p, _) => p,
        };
        assert_eq!(entry.priority, KnowledgePriority::P1);
    }

    #[test]
    fn test_priority_decay_p1_to_p2() {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let four_days_ago_epoch = now_secs - 4 * 86400;
        let four_days_ago = civil_from_days((four_days_ago_epoch / 86400) as i64);
        let ts = format!("{:04}-{:02}-{:02}", four_days_ago.0, four_days_ago.1, four_days_ago.2);

        let entry = IndexEntry {
            filename: "note_older.md".into(),
            when_useful: "older entry".into(),
            priority: KnowledgePriority::P1,
            last_promoted_at: Some(ts),
        };

        let promoted_secs = parse_iso_to_secs(&entry.last_promoted_at.as_ref().unwrap()).unwrap();
        let days_since = (now_secs.saturating_sub(promoted_secs)) / 86400;

        let new_priority = match (entry.priority, days_since) {
            (KnowledgePriority::P0, d) if d >= 1 => KnowledgePriority::P1,
            (KnowledgePriority::P1, d) if d >= 3 => KnowledgePriority::P2,
            (KnowledgePriority::P2, d) if d >= 7 => KnowledgePriority::P3,
            (p, _) => p,
        };
        assert_eq!(new_priority, KnowledgePriority::P2);
    }

    #[test]
    fn test_priority_decay_p2_to_p3() {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let eight_days_ago_epoch = now_secs - 8 * 86400;
        let eight_days_ago = civil_from_days((eight_days_ago_epoch / 86400) as i64);
        let ts = format!("{:04}-{:02}-{:02}", eight_days_ago.0, eight_days_ago.1, eight_days_ago.2);

        let entry = IndexEntry {
            filename: "note_dead.md".into(),
            when_useful: "dead entry".into(),
            priority: KnowledgePriority::P2,
            last_promoted_at: Some(ts),
        };

        let promoted_secs = parse_iso_to_secs(&entry.last_promoted_at.as_ref().unwrap()).unwrap();
        let days_since = (now_secs.saturating_sub(promoted_secs)) / 86400;

        let new_priority = match (entry.priority, days_since) {
            (KnowledgePriority::P0, d) if d >= 1 => KnowledgePriority::P1,
            (KnowledgePriority::P1, d) if d >= 3 => KnowledgePriority::P2,
            (KnowledgePriority::P2, d) if d >= 7 => KnowledgePriority::P3,
            (p, _) => p,
        };
        assert_eq!(new_priority, KnowledgePriority::P3);
    }

    #[test]
    fn test_priority_p0_stays_p0_when_promoted() {
        let now = now_iso_string();
        let mut entry = IndexEntry {
            filename: "note_p0.md".into(),
            when_useful: "always used".into(),
            priority: KnowledgePriority::P0,
            last_promoted_at: Some(now.clone()),
        };
        let used: HashSet<&str> = ["note_p0.md"].into();

        let was = entry.priority.clone();
        if used.contains(entry.filename.as_str()) {
            entry.priority = match entry.priority {
                KnowledgePriority::P3 => KnowledgePriority::P2,
                KnowledgePriority::P2 => KnowledgePriority::P1,
                KnowledgePriority::P1 => KnowledgePriority::P0,
                KnowledgePriority::P0 => KnowledgePriority::P0,
            };
        }
        assert_eq!(entry.priority, was);
        assert_eq!(entry.priority, KnowledgePriority::P0);
    }

    #[test]
    fn test_priority_no_decay_when_never_promoted() {
        let mut entry = IndexEntry {
            filename: "note_fresh.md".into(),
            when_useful: "fresh entry".into(),
            priority: KnowledgePriority::P1,
            last_promoted_at: None,
        };

        let was = entry.priority.clone();
        if let Some(ref promoted_at) = entry.last_promoted_at {
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let promoted_secs = parse_iso_to_secs(promoted_at).unwrap_or(0);
            let days_since = (now_secs.saturating_sub(promoted_secs)) / 86400;
            entry.priority = match (entry.priority.clone(), days_since) {
                (KnowledgePriority::P0, d) if d >= 1 => KnowledgePriority::P1,
                (KnowledgePriority::P1, d) if d >= 3 => KnowledgePriority::P2,
                (KnowledgePriority::P2, d) if d >= 7 => KnowledgePriority::P3,
                (p, _) => p,
            };
        }
        assert_eq!(entry.priority, was, "never-promoted entry should not decay");
        assert_eq!(entry.priority, KnowledgePriority::P1);
    }
}
