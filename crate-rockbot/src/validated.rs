use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Borrow;
use std::fmt;
use std::hash::Hash;
use std::ops::Deref;

const MAX_BOUNDED: usize = 100_000_000;

/// Error returned when validation fails for a validated newtype.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub type_name: &'static str,
    pub message: String,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} validation failed: {}", self.type_name, self.message)
    }
}

impl std::error::Error for ValidationError {}

fn validate_not_empty(value: &str, type_name: &'static str) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Err(ValidationError {
            type_name,
            message: "must not be empty".into(),
        });
    }
    Ok(())
}

fn validate_bounded(value: usize, type_name: &'static str) -> Result<(), ValidationError> {
    if value < 1 || value > MAX_BOUNDED {
        return Err(ValidationError {
            type_name,
            message: format!("must be in range 1..={}", MAX_BOUNDED),
        });
    }
    Ok(())
}

// ----- NonEmptyString -----

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NonEmptyString(String);

impl NonEmptyString {
    pub fn try_new(value: String) -> Result<Self, ValidationError> {
        validate_not_empty(&value, "NonEmptyString")?;
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

impl Borrow<str> for NonEmptyString {
    fn borrow(&self) -> &str {
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

// ----- ProviderName -----

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProviderName(String);

impl ProviderName {
    pub fn try_new(value: String) -> Result<Self, ValidationError> {
        validate_not_empty(&value, "ProviderName")?;
        Ok(Self(value))
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for ProviderName {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for ProviderName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ProviderName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::try_new(s).map_err(serde::de::Error::custom)
    }
}

// ----- ModelAlias -----

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelAlias(String);

impl ModelAlias {
    pub fn try_new(value: String) -> Result<Self, ValidationError> {
        validate_not_empty(&value, "ModelAlias")?;
        Ok(Self(value))
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for ModelAlias {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for ModelAlias {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ModelAlias {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::try_new(s).map_err(serde::de::Error::custom)
    }
}

// ----- ApiKey -----

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn try_new(value: String) -> Result<Self, ValidationError> {
        validate_not_empty(&value, "ApiKey")?;
        Ok(Self(value))
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for ApiKey {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for ApiKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ApiKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::try_new(s).map_err(serde::de::Error::custom)
    }
}

// ----- ConfigUrl -----

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConfigUrl(String);

impl ConfigUrl {
    pub fn try_new(value: String) -> Result<Self, ValidationError> {
        validate_not_empty(&value, "ConfigUrl")?;
        Ok(Self(value))
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for ConfigUrl {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for ConfigUrl {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ConfigUrl {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::try_new(s).map_err(serde::de::Error::custom)
    }
}

// ----- ConfigUsername -----

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConfigUsername(String);

impl ConfigUsername {
    pub fn try_new(value: String) -> Result<Self, ValidationError> {
        validate_not_empty(&value, "ConfigUsername")?;
        Ok(Self(value))
    }

    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for ConfigUsername {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for ConfigUsername {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ConfigUsername {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::try_new(s).map_err(serde::de::Error::custom)
    }
}

// ----- BoundedUsize -----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BoundedUsize(usize);

impl BoundedUsize {
    pub fn try_new(value: usize) -> Result<Self, ValidationError> {
        validate_bounded(value, "BoundedUsize")?;
        Ok(Self(value))
    }

    pub fn into_inner(self) -> usize {
        self.0
    }

    pub fn as_usize(&self) -> usize {
        self.0
    }
}

impl Deref for BoundedUsize {
    type Target = usize;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for BoundedUsize {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BoundedUsize {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = usize::deserialize(deserializer)?;
        Self::try_new(v).map_err(serde::de::Error::custom)
    }
}

// ----- ValidationError impl for converting to crate error types -----

impl From<ValidationError> for crate::error::RockBotError {
    fn from(e: ValidationError) -> Self {
        crate::error::RockBotError::Config(e.to_string())
    }
}
