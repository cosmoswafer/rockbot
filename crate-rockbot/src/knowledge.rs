use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tracing::{debug, warn};
use webdav::{WebDavClient, WebDavPath};

use crate::error::Result;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub id: String,
    pub filename: String,
    pub category: KnowledgeCategory,
    pub title: String,
    pub when_useful: String,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
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
            Err(_) => Ok(KnowledgeIndex {
                version: "rockbot-knowledge/1".into(),
                room_id: webdav_dir.to_string(),
                entries: Vec::new(),
                updated: String::new(),
            }),
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
    ) -> Result<()> {
        let now = now_iso_string();
        let slug = format!("{}_{}", category, Self::slugify(topic));
        let filename = format!("{}.md", slug);
        let md_path = format!("{}{}", Self::knowledge_dir(webdav_dir), filename);

        let folder = Self::knowledge_dir(webdav_dir);
        let _ = webdav.ensure_directory_all(&folder).await;

        let md_body = format!(
            "# {}\n\n**Category:** {}\n**When Useful:** {}\n**Tags:** {}\n**Created:** {}\n**Updated:** {}\n\n{}",
            topic, category, when_useful, tags.join(", "), now, now, content
        );

        webdav
            .write_file_with_fallback(&md_path, md_body.as_bytes().to_vec())
            .await
            .map_err(|e| {
                crate::error::RockBotError::Provider(format!("Knowledge write failed: {e}"))
            })?;

        let mut index = Self::load_index(webdav, webdav_dir).await?;
        if let Some(existing) = index.entries.iter_mut().find(|e| e.id == slug) {
            existing.title = topic.to_string();
            existing.when_useful = when_useful.to_string();
            existing.tags = tags.to_vec();
            existing.updated_at = now.clone();
        } else {
            index.entries.push(IndexEntry {
                id: slug.clone(),
                filename,
                category: category.clone(),
                title: topic.to_string(),
                when_useful: when_useful.to_string(),
                tags: tags.to_vec(),
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

        debug!("Saved knowledge entry {} in room {}", slug, webdav_dir);
        Ok(())
    }

    pub async fn delete_entry(
        webdav: &WebDavClient,
        webdav_dir: &str,
        topic: &str,
    ) -> Result<()> {
        let slug = format!(
            "{}_{}",
            KnowledgeCategory::Note,
            Self::slugify(topic)
        );
        let filename = format!("{}.md", slug);
        let md_path = format!("{}{}", Self::knowledge_dir(webdav_dir), filename);

        let deleted = match webdav.delete(&md_path).await {
            Ok(()) => true,
            Err(e) => {
                warn!("Failed to delete knowledge file: {e}");
                false
            }
        };

        let mut index = Self::load_index(webdav, webdav_dir).await?;
        let len_before = index.entries.len();
        index.entries.retain(|e| {
            let topic_match = e.title.eq_ignore_ascii_case(topic);
            let slug_match = e.id.eq_ignore_ascii_case(&slug)
                || e.id.eq_ignore_ascii_case(&format!("skill_{}", Self::slugify(topic)))
                || e.id.eq_ignore_ascii_case(&format!("secret_{}", Self::slugify(topic)));
            !(topic_match || slug_match)
        });

        if len_before == index.entries.len() && !deleted {
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
                let title_lower = entry.title.to_lowercase();
                let tag_set: HashSet<String> = entry
                    .tags
                    .iter()
                    .map(|t| t.to_lowercase())
                    .collect();

                let mut score = 0usize;
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

                if score > 0 {
                    Some((score, entry.clone()))
                } else {
                    None
                }
            })
            .collect();

        scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
        scored.into_iter().take(5).map(|(_, e)| e).collect()
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
        let entry = if query_lower.is_empty() {
            let content_parts = Vec::new();
            let mut content_parts = content_parts;
            for e in &index.entries {
                if let Ok(body) =
                    Self::read_entry_md(webdav, webdav_dir, &e.filename).await
                {
                    content_parts.push(format!(
                        "[Knowledge: {}/{}]\n{}\nUse when: {}",
                        e.category, e.title, body, e.when_useful
                    ));
                }
            }
            return Ok(Some(content_parts.join("\n---\n")));
        } else {
            index.entries.iter().find(|e| {
                e.title.to_lowercase().contains(&query_lower)
                    || e.when_useful.to_lowercase().contains(&query_lower)
                    || e.tags.iter().any(|t| t.to_lowercase().contains(&query_lower))
            })
        };

        if let Some(e) = entry {
            let body = Self::read_entry_md(webdav, webdav_dir, &e.filename).await?;
            Ok(Some(format!(
                "[Knowledge: {}/{}]\n{}\nUse when: {}",
                e.category, e.title, body, e.when_useful
            )))
        } else {
            Ok(Some(format!("No knowledge entry found matching '{query}'.")))
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
}

fn now_iso_string() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86400;
    let time = secs % 86400;
    let h = time / 3600;
    let m = (time % 3600) / 60;
    let s = time % 60;
    let z = (days as i64) + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let yr = if mo <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", yr, mo, d, h, m, s)
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
                id: "skill_api".into(),
                filename: "skill_api.md".into(),
                category: KnowledgeCategory::Skill,
                title: "API Access".into(),
                when_useful: "when calling the database API".into(),
                tags: vec!["api".into(), "database".into()],
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
                id: "skill_api".into(),
                filename: "skill_api.md".into(),
                category: KnowledgeCategory::Skill,
                title: "API Access".into(),
                when_useful: "when calling the database API".into(),
                tags: vec![],
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
        assert_eq!(matches[0].title, "API Access");
    }

    #[test]
    fn test_match_relevant_finds_by_tag() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![IndexEntry {
                id: "note_build".into(),
                filename: "note_build.md".into(),
                category: KnowledgeCategory::Note,
                title: "Build Commands".into(),
                when_useful: "general reference".into(),
                tags: vec!["build".into(), "cargo".into()],
                created_at: String::new(),
                updated_at: String::new(),
            }],
            updated: String::new(),
        };

        let matches =
            KnowledgeManager::match_relevant(&index, &["how do I build this cargo project"]);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, "note_build");
    }

    #[test]
    fn test_match_relevant_no_match() {
        let index = KnowledgeIndex {
            version: "rockbot-knowledge/1".into(),
            room_id: "r-test".into(),
            entries: vec![IndexEntry {
                id: "skill_api".into(),
                filename: "skill_api.md".into(),
                category: KnowledgeCategory::Skill,
                title: "API Access".into(),
                when_useful: "when calling the database API".into(),
                tags: vec!["api".into()],
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
                    id: "a".into(),
                    filename: "a.md".into(),
                    category: KnowledgeCategory::Note,
                    title: "Entry A".into(),
                    when_useful: "when talking about weather".into(),
                    tags: vec![],
                    created_at: String::new(),
                    updated_at: String::new(),
                },
                IndexEntry {
                    id: "b".into(),
                    filename: "b.md".into(),
                    category: KnowledgeCategory::Note,
                    title: "Weather Report".into(),
                    when_useful: "general reference".into(),
                    tags: vec![],
                    created_at: String::new(),
                    updated_at: String::new(),
                },
            ],
            updated: String::new(),
        };

        let matches = KnowledgeManager::match_relevant(&index, &["what is the weather like"]);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].id, "a");
        assert_eq!(matches[1].id, "b");
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
