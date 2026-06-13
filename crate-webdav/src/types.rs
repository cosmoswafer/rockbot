use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::ops::Deref;

use crate::validated::ValidationError;

fn validate_action(value: &str) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Err(ValidationError {
            type_name: "ReminderAction",
            message: "must not be empty".into(),
        });
    }
    if value.len() > 64 {
        return Err(ValidationError {
            type_name: "ReminderAction",
            message: "must be at most 64 characters".into(),
        });
    }
    Ok(())
}

// ----- ReminderAction -----

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderAction(String);

impl ReminderAction {
    pub fn try_new(value: String) -> Result<Self, ValidationError> {
        validate_action(&value)?;
        Ok(Self(value))
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for ReminderAction {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for ReminderAction {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ReminderAction {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::try_new(s).map_err(serde::de::Error::custom)
    }
}

// ----- ReminderTrigger -----

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderTrigger(String);

impl ReminderTrigger {
    pub fn try_new(value: String) -> Result<Self, ValidationError> {
        if value.is_empty() {
            return Err(ValidationError {
                type_name: "ReminderTrigger",
                message: "must not be empty".into(),
            });
        }
        Ok(Self(value))
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for ReminderTrigger {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for ReminderTrigger {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ReminderTrigger {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::try_new(s).map_err(serde::de::Error::custom)
    }
}

// ----- NonEmptyString -----

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NonEmptyString(String);

impl NonEmptyString {
    pub fn try_new(value: String) -> Result<Self, ValidationError> {
        if value.is_empty() {
            return Err(ValidationError {
                type_name: "NonEmptyString",
                message: "must not be empty".into(),
            });
        }
        Ok(Self(value))
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for NonEmptyString {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for NonEmptyString {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for NonEmptyString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::try_new(s).map_err(serde::de::Error::custom)
    }
}

// ----- Domain types -----

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
    pub action: ReminderAction,
    pub trigger: ReminderTrigger,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CaldavTodo {
    pub uid: NonEmptyString,
    pub href: String,
    pub summary: NonEmptyString,
    pub description: Option<String>,
    pub priority: Option<u8>,
    pub status: String,
    pub due: Option<String>,
    pub completed: Option<String>,
    pub created: String,
}
