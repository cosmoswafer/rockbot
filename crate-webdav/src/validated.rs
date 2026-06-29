use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::ops::Deref;

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

macro_rules! define_validated {
    ($name:ident, $doc:expr) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name(String);

        impl $name {
            pub fn try_new(value: String) -> Result<Self, ValidationError> {
                validate_not_empty(&value, stringify!($name))?;
                Ok(Self(value))
            }

            pub fn into_inner(self) -> String {
                self.0
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Deref for $name {
            type Target = String;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                self.0.serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let s = String::deserialize(deserializer)?;
                Self::try_new(s).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_validated!(DavUrl, "Non-empty WebDAV URL");
define_validated!(DavUsername, "Non-empty WebDAV username");
define_validated!(DavPassword, "Non-empty WebDAV password");
define_validated!(DavRoot, "Non-empty WebDAV root directory");
define_validated!(DavPath, "Non-empty WebDAV path");
