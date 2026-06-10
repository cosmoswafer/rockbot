use serde::{Deserialize, Deserializer};

#[derive(Debug, Clone, PartialEq)]
pub struct WebDavEntry {
    pub name: String,
    pub href: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename = "multistatus")]
pub(crate) struct MultiStatus {
    #[serde(rename = "response", default)]
    pub responses: Vec<Response>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Response {
    pub href: String,
    #[serde(rename = "propstat")]
    pub propstats: Vec<PropStat>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PropStat {
    pub prop: Prop,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub(crate) struct Prop {
    #[serde(
        rename = "getlastmodified",
        default,
        deserialize_with = "deserialize_opt_string"
    )]
    pub getlastmodified: Option<String>,

    #[serde(
        rename = "getcontentlength",
        default,
        deserialize_with = "deserialize_opt_u64"
    )]
    pub getcontentlength: Option<u64>,

    #[serde(
        rename = "getcontenttype",
        default,
        deserialize_with = "deserialize_opt_string"
    )]
    pub getcontenttype: Option<String>,

    #[serde(rename = "resourcetype", default)]
    pub resourcetype: ResourceType,

    #[serde(
        rename = "getetag",
        default,
        deserialize_with = "deserialize_opt_string"
    )]
    pub getetag: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct ResourceType {
    #[serde(rename = "collection", default)]
    pub collection: Option<Collection>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Collection {}

fn deserialize_opt_string<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer).unwrap_or_default();
    Ok(s.filter(|v| !v.is_empty()))
}

fn deserialize_opt_u64<'de, D>(deserializer: D) -> std::result::Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer).unwrap_or_default();
    match s {
        Some(v) if !v.is_empty() => v
            .parse::<u64>()
            .map(Some)
            .map_err(serde::de::Error::custom),
        _ => Ok(None),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaldavEvent {
    pub uid: String,
    pub href: String,
    pub etag: String,
    pub summary: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub dtstart: String,
    pub dtend: String,
    pub rrule: Option<String>,
    pub reminders: Vec<Reminder>,
    pub created: String,
    pub last_modified: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reminder {
    pub action: String,
    pub trigger: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaldavTodo {
    pub uid: String,
    pub href: String,
    pub summary: String,
    pub description: Option<String>,
    pub priority: Option<u8>,
    pub status: String,
    pub due: Option<String>,
    pub completed: Option<String>,
    pub created: String,
}
