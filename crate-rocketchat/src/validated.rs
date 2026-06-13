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
    ($name:ident) => {
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

define_validated!(ServerUrl);
define_validated!(Username);
define_validated!(Password);
