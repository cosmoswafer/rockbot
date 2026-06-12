use serde::{Deserialize, Serialize};
use serde_valid::Validate;
use std::collections::HashSet;
use tracing::{debug, warn};
use webdav::{WebDavClient, WebDavError, WebDavPath};

use crate::error::Result;
use crate::memory::DailySummary;
use crate::utils::now_iso_string;
use crate::validated::NonEmptyString;

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
    pub tags: Vec<String>,
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

/// Parsed tool arguments for save_knowledge — typed boundary for "parse, don't validate".
#[derive(Debug, Clone, Deserialize)]
pub struct SaveKnowledgeParams {
    pub category: KnowledgeCategory,
    pub topic: NonEmptyString,
    pub content: NonEmptyString,
    pub when_useful: String,
    #[serde(default)]
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
                    updated: String::new(),
                })
            }
            Err(e) => {
                Err(crate::error::RockBotError::Provider(format!(
                    "Failed to read knowledge index: {e}"
                )))
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
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

        // Update index first — the index is the source of truth
        let mut index = Self::load_index(webdav, webdav_dir).await?;
        if let Some(existing) = index.entries.iter_mut().find(|e| e.filename == filename) {
            existing.when_useful = when_useful.to_string();
            existing.tags = tags.to_vec();
        } else {
            index.entries.push(IndexEntry {
                filename: filename.clone(),
                when_useful: when_useful.to_string(),
                tags: tags.to_vec(),
            });
        }
        index.updated = now.clone();

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
            "# {}\n\n**Category:** {}\n**Priority:** {}\n**When Useful:** {}\n**Tags:** {}\n**Created:** {}\n**Updated:** {}\n\n{}",
            topic, category, priority, when_useful, tags.join(", "), now, now, content
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
                    for tag in &entry.tags {
                        if tag.to_lowercase().contains(kw.as_str()) {
                            score += 1;
                        }
                    }
                }
                if score > 0 {
                    Some((score, entry.clone()))
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
        _webdav: &WebDavClient,
        _webdav_dir: &str,
        _summaries: &[DailySummary],
    ) -> Result<bool> {
        Ok(false)
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
    fn test_match_relevant_finds_by_title() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![IndexEntry {
                filename: "note_build.md".into(),
                when_useful: "When building cargo projects".into(),
                tags: vec!["rust".into()],
            }],
            updated: String::new(),
        };

        let matches =
            KnowledgeManager::match_relevant(&index, &["how do I build this cargo project"]);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].filename, "note_build.md");
    }

    #[test]
    fn test_match_relevant_finds_by_tag() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![IndexEntry {
                filename: "skill_api.md".into(),
                when_useful: "random situation".into(),
                tags: vec!["database".into(), "api".into()],
            }],
            updated: String::new(),
        };

        let matches =
            KnowledgeManager::match_relevant(&index, &["how do I connect to the database?"]);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].filename, "skill_api.md");
    }

    #[test]
    fn test_match_relevant_finds_by_when_useful() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![IndexEntry {
                filename: "note_contact.md".into(),
                when_useful: "When someone asks about office hours or support phone numbers".into(),
                tags: vec![],
            }],
            updated: String::new(),
        };

        let matches =
            KnowledgeManager::match_relevant(&index, &["what are your office hours?"]);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].filename, "note_contact.md");
    }

    #[test]
    fn test_match_relevant_no_match() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![IndexEntry {
                filename: "skill_api.md".into(),
                when_useful: "When working with REST APIs".into(),
                tags: vec!["http".into()],
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

}
