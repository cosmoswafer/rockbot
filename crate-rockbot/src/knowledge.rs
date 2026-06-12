use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tracing::{debug, warn};
use webdav::{WebDavClient, WebDavPath};

use crate::error::Result;
use crate::memory::DailySummary;
use crate::utils::{now_iso_string, today_iso_date};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum KnowledgeCategory {
    #[serde(rename = "skill")]
    Skill,
    #[serde(rename = "secret")]
    Secret,
    #[serde(rename = "note")]
    Note,
}

impl std::fmt::Display for KnowledgeCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KnowledgeCategory::Skill => write!(f, "skill"),
            KnowledgeCategory::Secret => write!(f, "secret"),
            KnowledgeCategory::Note => write!(f, "note"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum KnowledgePriority {
    #[serde(rename = "P0")]
    P0,
    #[serde(rename = "P1")]
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

impl Default for KnowledgePriority {
    fn default() -> Self {
        KnowledgePriority::P1
    }
}

impl KnowledgePriority {
    pub(crate) fn score_bonus(&self) -> usize {
        match self {
            KnowledgePriority::P0 => 8,
            KnowledgePriority::P1 => 5,
            KnowledgePriority::P2 => 2,
            KnowledgePriority::P3 => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub filename: String,
    pub tags: Vec<String>,
    pub when_useful: String,
    #[serde(default)]
    pub priority: KnowledgePriority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_degraded_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl IndexEntry {
    pub fn display_title(&self) -> &str {
        let name = self.filename.strip_suffix(".md").unwrap_or(&self.filename);
        name
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeIndex {
    pub version: String,
    pub room_id: String,
    pub entries: Vec<IndexEntry>,
    pub updated: String,
}

#[derive(Debug, Clone)]
pub struct KnowledgeEntry {
    pub id: String,
    pub room_id: String,
    pub category: KnowledgeCategory,
    pub title: String,
    pub content: String,
    pub when_useful: String,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub struct KnowledgeManager;

impl KnowledgeManager {
    fn knowledge_dir(webdav_dir: &str) -> String {
        format!("{}knowledge/", WebDavPath::new("").room_dir(webdav_dir))
    }

    fn index_path(webdav_dir: &str) -> String {
        format!("{}index.json", Self::knowledge_dir(webdav_dir))
    }

    fn slugify(title: &str) -> String {
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
            Err(_) => {
                debug!("No knowledge index found for room {}, starting fresh", webdav_dir);
                Ok(KnowledgeIndex {
                    version: "rockbot-knowledge/1".into(),
                    room_id: webdav_dir.to_string(),
                    entries: Vec::new(),
                    updated: String::new(),
                })
            }
        }
    }

    pub async fn save_entry(
        webdav: &WebDavClient,
        webdav_dir: &str,
        category: &KnowledgeCategory,
        topic: &str,
        content: &str,
        when_useful: &str,
        tags: &[String],
        priority: &KnowledgePriority,
    ) -> Result<()> {
        let now = now_iso_string();
        let slug = format!("{}_{}", category, Self::slugify(topic));
        let filename = format!("{}.md", slug);
        let md_path = format!("{}{}", Self::knowledge_dir(webdav_dir), filename);

        let folder = Self::knowledge_dir(webdav_dir);
        if let Err(e) = webdav.ensure_directory_all(&folder).await {
            warn!("Failed to ensure knowledge directory {}: {}", folder, e);
        }

        let md_body = format!(
            "# {}\n\n**Category:** {}\n**Priority:** {}\n**When Useful:** {}\n**Tags:** {}\n**Created:** {}\n**Updated:** {}\n\n{}",
            topic, category, priority, when_useful, tags.join(", "), now, now, content
        );

        webdav
            .write_file_with_fallback(&md_path, md_body.as_bytes().to_vec())
            .await
            .map_err(|e| {
                crate::error::RockBotError::Provider(format!("Knowledge write failed: {e}"))
            })?;

        let mut index = Self::load_index(webdav, webdav_dir).await?;
        if let Some(existing) = index.entries.iter_mut().find(|e| e.filename == filename) {
            existing.when_useful = when_useful.to_string();
            existing.tags = tags.to_vec();
            existing.priority = priority.clone();
            existing.updated_at = now.clone();
        } else {
            index.entries.push(IndexEntry {
                filename: filename.clone(),
                when_useful: when_useful.to_string(),
                tags: tags.to_vec(),
                priority: priority.clone(),
                last_degraded_at: None,
                created_at: now.clone(),
                updated_at: now.clone(),
            });
        }
        index.updated = now;

        let index_body = serde_json::to_string_pretty(&index).map_err(|e| {
            crate::error::RockBotError::Provider(format!("Failed to serialize knowledge index: {e}"))
        })?;
        webdav
            .write_file_with_fallback(&Self::index_path(webdav_dir), index_body.as_bytes().to_vec())
            .await
            .map_err(|e| {
                crate::error::RockBotError::Provider(format!("Knowledge index write failed: {e}"))
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

        let mut deleted_files = 0usize;
        let matching_entries: Vec<_> = index.entries.iter().filter(|e| {
            e.filename.to_lowercase().contains(&topic_slug)
        }).collect();

        for entry in &matching_entries {
            let md_path = format!(
                "{}{}",
                Self::knowledge_dir(webdav_dir),
                entry.filename
            );
            match webdav.delete(&md_path).await {
                Ok(()) => deleted_files += 1,
                Err(e) => warn!("Failed to delete knowledge file {}: {}", entry.filename, e),
            }
        }

        let len_before = index.entries.len();
        index.entries.retain(|e| {
            !e.filename.to_lowercase().contains(&topic_slug)
        });

        if len_before == index.entries.len() && deleted_files == 0 {
            return Err(crate::error::RockBotError::ToolCallParse(format!(
                "Knowledge entry '{topic}' not found."
            )));
        }

        index.updated = now_iso_string();
        let index_body = serde_json::to_string_pretty(&index).map_err(|e| {
            crate::error::RockBotError::Provider(format!("Failed to serialize knowledge index: {e}"))
        })?;
        webdav
            .write_file_with_fallback(&Self::index_path(webdav_dir), index_body.as_bytes().to_vec())
            .await
            .map_err(|e| {
                crate::error::RockBotError::Provider(format!("Knowledge index write failed: {e}"))
            })?;

        debug!("Deleted knowledge entry for topic '{}' in room {}", topic, webdav_dir);
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
            return Vec::new();
        }

        let mut scored: Vec<(usize, IndexEntry)> = index
            .entries
            .iter()
            .filter_map(|entry| {
                let useful_lower = entry.when_useful.to_lowercase();
                let title_lower = entry.display_title().to_lowercase();
                let tag_set: HashSet<String> = entry
                    .tags
                    .iter()
                    .map(|t| t.to_lowercase())
                    .collect();

                let mut score = entry.priority.score_bonus();
                for kw in &keywords {
                    if useful_lower.contains(kw.as_str()) {
                        score += 3;
                    }
                    if title_lower.contains(kw.as_str()) {
                        score += 2;
                    }
                    if tag_set.contains(kw.as_str()) {
                        score += 2;
                    }
                }

                if score > 0 || entry.priority == KnowledgePriority::P0 {
                    Some((score, entry.clone()))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
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
                        "[Knowledge: {}]\n{}\nUse when: {}",
                        e.display_title(), body, e.when_useful
                    ));
                }
            }
            if content_parts.is_empty() {
                return Ok(Some("No knowledge entries found for this room.".into()));
            }
            return Ok(Some(content_parts.join("\n---\n")));
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
                        "[Knowledge: {}]\n{}\nUse when: {}",
                        e.display_title(), body, e.when_useful
                    ));
                }
            }
            return Ok(Some(content_parts.join("\n---\n")));
        }
    }

    async fn read_entry_md(
        webdav: &WebDavClient,
        webdav_dir: &str,
        filename: &str,
    ) -> Result<String> {
        let path = format!("{}{}", Self::knowledge_dir(webdav_dir), filename);
        webdav.read_file_to_string(&path).await.map_err(|e| {
            crate::error::RockBotError::Provider(format!("Failed to read knowledge entry: {e}"))
        })
    }

    /// Extracts keywords from an entry: title tokens, when_useful tokens,
    /// and tags, each > 2 chars, lowercased.
    fn entry_keywords(entry: &IndexEntry) -> Vec<String> {
        let mut keys: Vec<String> = entry
            .display_title()
            .split(|c: char| !c.is_alphanumeric())
            .chain(entry.when_useful.split(|c: char| !c.is_alphanumeric()))
            .filter(|w| w.len() > 2)
            .map(|w| w.to_lowercase())
            .collect();
        for tag in &entry.tags {
            if tag.len() > 2 {
                keys.push(tag.to_lowercase());
            }
        }
        keys.sort();
        keys.dedup();
        keys
    }

    /// Checks whether any keyword from the entry appears in the given text.
    fn entry_mentioned_in_text(entry: &IndexEntry, text: &str) -> bool {
        let text_lower = text.to_lowercase();
        Self::entry_keywords(entry)
            .iter()
            .any(|kw| text_lower.contains(kw.as_str()))
    }

    /// Counts how many of the latest 3 daily summaries mention the entry.
    fn count_mentioned_days(entry: &IndexEntry, summaries: &[DailySummary]) -> usize {
        let mut sorted: Vec<&DailySummary> = summaries.iter().collect();
        sorted.sort_by_key(|s| std::cmp::Reverse(s.date.as_str()));
        sorted
            .iter()
            .take(3)
            .filter(|s| Self::entry_mentioned_in_text(entry, &s.summary))
            .count()
    }

    /// Determines whether a degradation is allowed (at most once per day).
    fn can_degrade(last_degraded_at: &Option<String>) -> bool {
        let Some(iso_str) = last_degraded_at else {
            return true;
        };
        if iso_str.len() < 10 {
            return true;
        }
        let today = today_iso_date();
        &iso_str[..10] != today.as_str()
    }

    /// Computes the new priority given current priority and mention day count.
    /// Returns (new_priority, is_degradation).
    /// Rule: if recalled in 3-day window → promote/maintain; if not → degrade one step.
    /// P0 = 3/3 days, P1 = default for new entries, P2 = 1st no-mention, P3 = stale.
    pub fn compute_new_priority(
        current: &KnowledgePriority,
        day_count: usize,
    ) -> (KnowledgePriority, bool) {
        let new = if day_count == 3 {
            // 3/3 days: P1→P0, P0 stays, all others → P1
            match current {
                KnowledgePriority::P0 | KnowledgePriority::P1 => KnowledgePriority::P0,
                _ => KnowledgePriority::P1,
            }
        } else if day_count >= 1 {
            // ≥1 but <3: P0→P1 (degrade), all others → P1 (promote/maintain)
            match current {
                KnowledgePriority::P0 => KnowledgePriority::P1,
                _ => KnowledgePriority::P1,
            }
        } else {
            // day_count == 0: degrade one step
            match current {
                KnowledgePriority::P0 => KnowledgePriority::P1,
                KnowledgePriority::P1 => KnowledgePriority::P2,
                KnowledgePriority::P2 => KnowledgePriority::P3,
                KnowledgePriority::P3 => KnowledgePriority::P3,
            }
        };

        // Degradation = new priority is higher enum ordinal (P0=0, P3=3)
        let is_degradation = priority_ord(&new) > priority_ord(current);
        (new, is_degradation)
    }

    /// Scans daily summaries for mentions and recalculates priorities for all
    /// knowledge entries in a room. Returns true if any priority was changed.
    pub async fn review_priorities(
        webdav: &WebDavClient,
        webdav_dir: &str,
        summaries: &[DailySummary],
    ) -> Result<bool> {
        let mut index = match Self::load_index(webdav, webdav_dir).await {
            Ok(idx) => idx,
            Err(_) => return Ok(false),
        };

        if index.entries.is_empty() {
            return Ok(false);
        }

        let now = now_iso_string();
        let mut changed = false;

        for entry in &mut index.entries {
            let day_count = Self::count_mentioned_days(entry, summaries);
            let (new_prio, is_degradation) =
                Self::compute_new_priority(&entry.priority, day_count);

            if new_prio == entry.priority {
                continue;
            }

            if is_degradation && !Self::can_degrade(&entry.last_degraded_at) {
                debug!(
                    "Rate-limited degradation for {} (last degraded {})",
                    entry.display_title(),
                    entry.last_degraded_at.as_deref().unwrap_or("never")
                );
                continue;
            }

            debug!(
                "Priority change for {}: {:?} → {:?} (day_count={})",
                entry.display_title(), entry.priority, new_prio, day_count
            );
            entry.priority = new_prio;
            if is_degradation {
                entry.last_degraded_at = Some(now.clone());
            } else {
                entry.last_degraded_at = None;
            }
            changed = true;
        }

        if changed {
            index.updated = now;
            let index_body = serde_json::to_string_pretty(&index).map_err(|e| {
                crate::error::RockBotError::Provider(format!(
                    "Failed to serialize knowledge index: {e}"
                ))
            })?;
            webdav
                .write_file_with_fallback(
                    &Self::index_path(webdav_dir),
                    index_body.as_bytes().to_vec(),
                )
                .await
                .map_err(|e| {
                    crate::error::RockBotError::Provider(format!(
                        "Knowledge index write failed: {e}"
                    ))
                })?;
            debug!(
                "Updated knowledge priorities for room {}",
                webdav_dir
            );
        }

        Ok(changed)
    }
}

/// Returns ordinal for priority: P0=0, P1=1, P2=2, P3=3
fn priority_ord(p: &KnowledgePriority) -> u8 {
    match p {
        KnowledgePriority::P0 => 0,
        KnowledgePriority::P1 => 1,
        KnowledgePriority::P2 => 2,
        KnowledgePriority::P3 => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_knowledge_category_display() {
        assert_eq!(KnowledgeCategory::Skill.to_string(), "skill");
        assert_eq!(KnowledgeCategory::Secret.to_string(), "secret");
        assert_eq!(KnowledgeCategory::Note.to_string(), "note");
    }

    #[test]
    fn test_match_relevant_empty_messages() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![IndexEntry {
                filename: "skill_api.md".into(),
                when_useful: "when calling the database API".into(),
                tags: vec!["api".into(), "database".into()],
                priority: KnowledgePriority::P3,
                last_degraded_at: None,
                created_at: String::new(),
                updated_at: String::new(),
            }],
            updated: String::new(),
        };

        let matches = KnowledgeManager::match_relevant(&index, &[]);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_match_relevant_finds_by_when_useful() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![IndexEntry {
                filename: "skill_api.md".into(),
                when_useful: "when calling the database API".into(),
                tags: vec![],
                priority: KnowledgePriority::P3,
                last_degraded_at: None,
                created_at: String::new(),
                updated_at: String::new(),
            }],
            updated: String::new(),
        };

        let matches = KnowledgeManager::match_relevant(
            &index,
            &["I need to call the database", "can you help?"],
        );
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].display_title(), "skill_api");
    }

    #[test]
    fn test_match_relevant_finds_by_tag() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![IndexEntry {
                filename: "note_build.md".into(),
                when_useful: "general reference".into(),
                tags: vec!["build".into(), "cargo".into()],
                priority: KnowledgePriority::P3,
                last_degraded_at: None,
                created_at: String::new(),
                updated_at: String::new(),
            }],
            updated: String::new(),
        };

        let matches =
            KnowledgeManager::match_relevant(&index, &["how do I build this cargo project"]);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].filename, "note_build.md");
    }

    #[test]
    fn test_match_relevant_no_match() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![IndexEntry {
                filename: "skill_api.md".into(),
                when_useful: "when calling the database API".into(),
                tags: vec!["api".into()],
                priority: KnowledgePriority::P3,
                last_degraded_at: None,
                created_at: String::new(),
                updated_at: String::new(),
            }],
            updated: String::new(),
        };

        let matches = KnowledgeManager::match_relevant(
            &index,
            &["hello", "how are you", "nice weather"],
        );
        assert!(matches.is_empty());
    }

    #[test]
    fn test_match_relevant_scores_higher_on_when_useful() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![
                IndexEntry {
                    filename: "a.md".into(),
                    when_useful: "when talking about weather".into(),
                    tags: vec![],
                    priority: KnowledgePriority::P3,
                    last_degraded_at: None,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
                IndexEntry {
                    filename: "b.md".into(),
                    when_useful: "general reference".into(),
                    tags: vec!["weather".into()],
                    priority: KnowledgePriority::P3,
                    last_degraded_at: None,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
            ],
            updated: String::new(),
        };

        let matches = KnowledgeManager::match_relevant(&index, &["what is the weather like"]);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].filename, "a.md");
        assert_eq!(matches[1].filename, "b.md");
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
    fn test_match_relevant_returns_all_when_more_than_5() {
        let mut entries = Vec::new();
        for i in 1..=7 {
            entries.push(IndexEntry {
                filename: format!("entry_{}.md", i),
                when_useful: "when talking about shared topics".into(),
                tags: vec!["shared".into()],
                priority: KnowledgePriority::P3,
                last_degraded_at: None,
                created_at: String::new(),
                updated_at: String::new(),
            });
        }
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries,
            updated: String::new(),
        };

        let matches = KnowledgeManager::match_relevant(
            &index,
            &["shared topic discussion"],
        );
        assert_eq!(matches.len(), 7, "all 7 matching entries should be returned (no cap)");
    }

    #[test]
    fn test_match_relevant_p0_always_matches_even_without_keywords() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![IndexEntry {
                filename: "critical_rule.md".into(),
                when_useful: "for specific rare scenarios only".into(),
                tags: vec!["rare".into()],
                priority: KnowledgePriority::P0,
                last_degraded_at: None,
                created_at: String::new(),
                updated_at: String::new(),
            }],
            updated: String::new(),
        };

        let matches = KnowledgeManager::match_relevant(
            &index,
            &["completely unrelated topic"],
        );
        assert_eq!(matches.len(), 1, "P0 entry should always be returned");
        assert_eq!(matches[0].filename, "critical_rule.md");
    }

    // --- Knowledge priority algorithm tests ---

    fn make_entry(filename: &str, priority: KnowledgePriority, when_useful: &str, tags: &[&str]) -> IndexEntry {
        IndexEntry {
            filename: filename.to_string(),
            when_useful: when_useful.to_string(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            priority,
            last_degraded_at: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn make_summary(date: &str, text: &str) -> DailySummary {
        DailySummary {
            date: date.to_string(),
            summary: text.to_string(),
            msg_count: 5,
            char_count: 200,
        }
    }

    #[test]
    fn test_compute_new_priority_p0_to_p0_when_mentioned_every_day() {
        let (prio, degraded) = KnowledgeManager::compute_new_priority(&KnowledgePriority::P0, 3);
        assert_eq!(prio, KnowledgePriority::P0);
        assert!(!degraded, "P0→P0 should not be a degradation");
    }

    #[test]
    fn test_compute_new_priority_p0_to_p1_when_mentioned_some_days() {
        let (prio, degraded) = KnowledgeManager::compute_new_priority(&KnowledgePriority::P0, 1);
        assert_eq!(prio, KnowledgePriority::P1);
        assert!(degraded, "P0→P1 is a degradation");
    }

    #[test]
    fn test_compute_new_priority_p0_to_p1_when_zero_mentions() {
        // P0 with 0 mentions in 3-day window → P1 (one step degradation)
        let (prio, degraded) = KnowledgeManager::compute_new_priority(&KnowledgePriority::P0, 0);
        assert_eq!(prio, KnowledgePriority::P1);
        assert!(degraded);
    }

    #[test]
    fn test_compute_new_priority_p1_to_p0_when_3() {
        let (prio, _) = KnowledgeManager::compute_new_priority(&KnowledgePriority::P1, 3);
        assert_eq!(prio, KnowledgePriority::P0);
    }

    #[test]
    fn test_compute_new_priority_p1_to_p1_when_some() {
        let (prio, degraded) = KnowledgeManager::compute_new_priority(&KnowledgePriority::P1, 1);
        assert_eq!(prio, KnowledgePriority::P1);
        assert!(!degraded, "P1→P1 is not a degradation");
    }

    #[test]
    fn test_compute_new_priority_p1_to_p2_when_zero() {
        let (prio, degraded) = KnowledgeManager::compute_new_priority(&KnowledgePriority::P1, 0);
        assert_eq!(prio, KnowledgePriority::P2);
        assert!(degraded);
    }

    #[test]
    fn test_compute_new_priority_p2_to_p1_when_3() {
        let (prio, _) = KnowledgeManager::compute_new_priority(&KnowledgePriority::P2, 3);
        assert_eq!(prio, KnowledgePriority::P1);
    }

    #[test]
    fn test_compute_new_priority_p2_to_p1_when_some() {
        let (prio, degraded) = KnowledgeManager::compute_new_priority(&KnowledgePriority::P2, 1);
        assert_eq!(prio, KnowledgePriority::P1);
        assert!(!degraded, "P2→P1 is not a degradation");
    }

    #[test]
    fn test_compute_new_priority_p2_to_p3_when_zero() {
        let (prio, degraded) = KnowledgeManager::compute_new_priority(&KnowledgePriority::P2, 0);
        assert_eq!(prio, KnowledgePriority::P3);
        assert!(degraded);
    }

    #[test]
    fn test_compute_new_priority_p3_to_p1_when_3() {
        let (prio, _) = KnowledgeManager::compute_new_priority(&KnowledgePriority::P3, 3);
        assert_eq!(prio, KnowledgePriority::P1);
    }

    #[test]
    fn test_compute_new_priority_p3_to_p1_when_some() {
        let (prio, degraded) = KnowledgeManager::compute_new_priority(&KnowledgePriority::P3, 1);
        assert_eq!(prio, KnowledgePriority::P1);
        assert!(!degraded, "P3→P1 is not a degradation");
    }

    #[test]
    fn test_compute_new_priority_p3_stays_p3_when_zero() {
        let (prio, degraded) = KnowledgeManager::compute_new_priority(&KnowledgePriority::P3, 0);
        assert_eq!(prio, KnowledgePriority::P3);
        assert!(!degraded, "P3→P3 is not a degradation (no change)");
    }

    #[test]
    fn test_entry_mentioned_in_text_title_match() {
        let entry = make_entry(
            "database_api.md",
            KnowledgePriority::P2,
            "when calling the api",
            &[],
        );
        assert!(KnowledgeManager::entry_mentioned_in_text(&entry, "Today we discussed the database access patterns"));
        assert!(KnowledgeManager::entry_mentioned_in_text(&entry, "api usage was high"));
    }

    #[test]
    fn test_entry_mentioned_in_text_when_useful_match() {
        let entry = make_entry(
            "build_config.md",
            KnowledgePriority::P2,
            "when setting up the build pipeline on CI",
            &[],
        );
        assert!(KnowledgeManager::entry_mentioned_in_text(&entry, "I was working on the CI build pipeline today"));
    }

    #[test]
    fn test_entry_mentioned_in_text_tag_match() {
        let entry = make_entry(
            "cargo_setup.md",
            KnowledgePriority::P2,
            "general reference",
            &["cargo", "rust", "build"],
        );
        assert!(KnowledgeManager::entry_mentioned_in_text(&entry, "We used cargo to compile the project"));
    }

    #[test]
    fn test_entry_mentioned_in_text_no_match() {
        let entry = make_entry(
            "database_api.md",
            KnowledgePriority::P2,
            "when calling the database API",
            &["database", "sql"],
        );
        assert!(!KnowledgeManager::entry_mentioned_in_text(&entry, "Today was quiet, nothing special happened"));
    }

    #[test]
    fn test_entry_mentioned_in_text_short_tokens_filtered() {
        let entry = make_entry(
            "db_api.md",
            KnowledgePriority::P2,
            "hi",
            &[],
        );
        // "db" and "hi" are both <= 2 chars, so no keywords
        assert!(!KnowledgeManager::entry_mentioned_in_text(&entry, "Today we talked about the DB and said hi"));
    }

    #[test]
    fn test_count_mentioned_days() {
        let entry = make_entry(
            "database_api.md",
            KnowledgePriority::P2,
            "when calling the api",
            &[],
        );
        let summaries = vec![
            make_summary("2026-06-11", "We discussed the database today"),
            make_summary("2026-06-10", "Just general chat"),
            make_summary("2026-06-09", "We used the api again"),
            make_summary("2026-06-08", "database migration planning"),
            make_summary("2026-06-07", "nothing relevant"),
            make_summary("2026-06-06", "api version 2 discussion"),
            make_summary("2026-06-05", "deployment went fine"),
        ];
        // Latest 3 days: 06-11 (database) ✓, 06-10 ✗, 06-09 (api) ✓ = 2 matches
        assert_eq!(KnowledgeManager::count_mentioned_days(&entry, &summaries), 2);
    }

    #[test]
    fn test_count_mentioned_days_only_latest_3() {
        let entry = make_entry(
            "database_api.md",
            KnowledgePriority::P1,
            "when calling the api",
            &[],
        );
        // 5 summaries, only latest 3 should be checked
        let mut summaries = Vec::new();
        for i in 0..5 {
            summaries.push(make_summary(
                &format!("2026-06-{:02}", 15 - i),
                if i == 0 { "database was discussed" } else { "irrelevant" },
            ));
        }
        // Only the first (i=0, date 06-15) should match, within latest 3
        assert_eq!(KnowledgeManager::count_mentioned_days(&entry, &summaries), 1);
    }

    #[test]
    fn test_can_degrade_none() {
        assert!(KnowledgeManager::can_degrade(&None));
    }

    #[test]
    fn test_can_degrade_yesterday() {
        assert!(KnowledgeManager::can_degrade(&Some("2020-01-01T00:00:00Z".to_string())));
    }

    #[test]
    fn test_can_degrade_today_blocked() {
        let today = crate::utils::today_iso_date();
        let today_iso = format!("{}T12:00:00Z", today);
        assert!(!KnowledgeManager::can_degrade(&Some(today_iso)));
    }

    #[test]
    fn test_default_priority_is_p1() {
        assert_eq!(KnowledgePriority::default(), KnowledgePriority::P1);
    }
}
