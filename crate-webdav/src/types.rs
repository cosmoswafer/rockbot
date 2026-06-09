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

    #[serde(rename = "getcontentlength", default)]
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
    use serde::de;
    struct StringOrEmpty;
    impl<'de> de::Visitor<'de> for StringOrEmpty {
        type Value = Option<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a string")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
            if v.is_empty() {
                Ok(None)
            } else {
                Ok(Some(v.to_string()))
            }
        }
        fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Self::Value, E> {
            if v.is_empty() { Ok(None) } else { Ok(Some(v)) }
        }
    }
    deserializer.deserialize_any(StringOrEmpty)
}
