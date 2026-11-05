//! Canonical `image_gen` model manifest (issues #111, #112).
//!
//! `models/image.toml` is the single source of truth for the alias roster: it
//! defines aliases, labels, metadata, and the ordered provider routes of each
//! alias. The manifest is embedded at build time so the bot can never start
//! with a missing roster; `[image_model] manifest_path` overrides it with a
//! file read from disk.
//!
//! Parsing happens once at startup ([`load_catalog`]); the resulting
//! [`ImageModelCatalog`] is the domain type consumed by `ImageGenTool`.
//! Manifest lints (non-empty roster, ≥1 route per model, unique aliases,
//! exactly one default alias, ≤1 default route per alias, deprecated ≠
//! default) run in [`ImageManifest::into_catalog`] so a malformed manifest
//! fails loudly at boot instead of producing a silent 404 at call time.

use std::collections::HashSet;

use serde::Deserialize;

use crate::error::{Result, RockBotError};
use crate::types::{ImageModelCatalog, ImageModelEntry, ImageRoute, ImageTier};
use crate::validated::{ModelAlias, NonEmptyString, ProviderName};

/// Embedded copy of the checked-in manifest.
pub const BUILTIN_IMAGE_MANIFEST: &str = include_str!("../../models/image.toml");

/// Parsed `models/image.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct ImageManifest {
    /// Manifest version string (cache-busting hint for UI consumers).
    #[serde(default)]
    pub version: Option<String>,
    #[serde(rename = "model", default)]
    pub models: Vec<ManifestModel>,
}

/// One `[[model]]` block.
#[derive(Debug, Clone, Deserialize)]
pub struct ManifestModel {
    pub alias: ModelAlias,
    pub label: String,
    pub family: String,
    pub version: String,
    /// Global default alias (exactly one across the manifest).
    #[serde(default)]
    pub default: bool,
    /// Deprecated aliases stay selectable for one release cycle.
    #[serde(default)]
    pub deprecated: bool,
    /// Aspect ratios the model accepts (metadata for UI pickers).
    #[serde(default)]
    pub ratios: Vec<String>,
    /// Free-form capability tags (metadata for UI pickers).
    #[serde(default)]
    pub tags: Vec<String>,
    /// One-line guidance included in the generated tool description.
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(rename = "route", default)]
    pub routes: Vec<ManifestRoute>,
}

/// One `[[model.route]]` block: provider + slugs + tier.
#[derive(Debug, Clone, Deserialize)]
pub struct ManifestRoute {
    pub provider: ProviderName,
    pub generate: NonEmptyString,
    /// Dedicated edit slug; absent/empty ⇒ generate-only route.
    #[serde(default)]
    pub edit: Option<String>,
    #[serde(default)]
    pub tier: ImageTier,
    /// Preferred route of the alias (at most one per model).
    #[serde(default)]
    pub default: bool,
}

impl ImageManifest {
    /// Parse a manifest from TOML text. `source` only appears in errors.
    pub fn parse(raw: &str, source: &str) -> Result<Self> {
        toml::from_str(raw).map_err(|e| {
            RockBotError::Config(format!(
                "image manifest '{source}' is not valid TOML / manifest schema: {e}"
            ))
        })
    }

    /// The embedded manifest shipped with this build.
    pub fn builtin() -> Result<Self> {
        Self::parse(BUILTIN_IMAGE_MANIFEST, "models/image.toml (embedded)")
    }

    /// Read and parse a manifest from disk.
    pub fn load(path: &str) -> Result<Self> {
        let raw = std::fs::read_to_string(path).map_err(|e| {
            RockBotError::Config(format!("failed to read image manifest '{path}': {e}"))
        })?;
        Self::parse(&raw, path)
    }

    /// Validate the manifest and convert it into the shared catalog type.
    pub fn into_catalog(self) -> Result<ImageModelCatalog> {
        if self.models.is_empty() {
            return Err(RockBotError::Config(
                "image manifest defines no [[model]] entries".into(),
            ));
        }

        let mut seen_aliases: HashSet<String> = HashSet::new();
        let mut default_alias: Option<String> = None;
        let mut entries = Vec::with_capacity(self.models.len());

        for model in self.models {
            let alias = model.alias.as_str().to_string();
            if !seen_aliases.insert(alias.clone()) {
                return Err(RockBotError::Config(format!(
                    "image manifest defines alias '{alias}' more than once"
                )));
            }
            if model.label.trim().is_empty() {
                return Err(RockBotError::Config(format!(
                    "image manifest alias '{alias}' has an empty label"
                )));
            }
            if model.routes.is_empty() {
                return Err(RockBotError::Config(format!(
                    "image manifest alias '{alias}' has no [[model.route]] entries"
                )));
            }
            if model.default && model.deprecated {
                return Err(RockBotError::Config(format!(
                    "image manifest alias '{alias}' cannot be both default and deprecated"
                )));
            }
            if model.default {
                if let Some(existing) = &default_alias {
                    return Err(RockBotError::Config(format!(
                        "image manifest marks both '{existing}' and '{alias}' as default"
                    )));
                }
                default_alias = Some(alias.clone());
            }

            let mut default_routes = 0;
            let routes: Vec<ImageRoute> = model
                .routes
                .into_iter()
                .map(|route| {
                    if route.default {
                        default_routes += 1;
                    }
                    ImageRoute {
                        provider_name: route.provider.into_inner(),
                        generate_model_id: route.generate.into_inner(),
                        edit_model_id: route
                            .edit
                            .as_deref()
                            .map(str::trim)
                            .filter(|slug| !slug.is_empty())
                            .map(String::from),
                        tier: route.tier,
                        is_default: route.default,
                    }
                })
                .collect();
            if default_routes > 1 {
                return Err(RockBotError::Config(format!(
                    "image manifest alias '{alias}' marks more than one route as default"
                )));
            }

            entries.push(ImageModelEntry {
                alias,
                label: model.label,
                family: model.family,
                version: model.version,
                deprecated: model.deprecated,
                routes,
            });
        }

        let default_alias = default_alias.ok_or_else(|| {
            RockBotError::Config(
                "image manifest marks no alias as default (add `default = true` to one [[model]])"
                    .into(),
            )
        })?;

        Ok(ImageModelCatalog::new(entries, default_alias))
    }
}

/// Load the catalog from `manifest_path` when set, else from the embedded
/// manifest. An explicitly configured path that cannot be read is a hard
/// error — a typo must not silently fall back to the built-in roster.
pub fn load_catalog(manifest_path: Option<&str>) -> Result<ImageModelCatalog> {
    match manifest_path.map(str::trim).filter(|p| !p.is_empty()) {
        Some(path) => ImageManifest::load(path)?.into_catalog(),
        None => ImageManifest::builtin()?.into_catalog(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_manifest_is_valid() {
        let catalog = ImageManifest::builtin()
            .expect("embedded manifest parses")
            .into_catalog()
            .expect("embedded manifest validates");
        assert!(!catalog.is_empty());
        assert_eq!(catalog.default_alias(), "gptimage");
        for required in [
            "gptimage",
            "sunburst",
            "banana",
            "qwenimage",
            "qwen3",
            "seedream5flash",
            "grok2",
        ] {
            assert!(
                catalog.entry(required).is_some(),
                "manifest must define '{required}'"
            );
        }
        assert_eq!(catalog.allowed_aliases().len(), 7, "roster is trimmed to 7 aliases");
        assert!(
            catalog.entry("seedream5pro").is_none(),
            "Seedream 5 Pro was dropped in #119"
        );
    }

    #[test]
    fn test_builtin_manifest_routes_are_mode_aware() {
        let catalog = ImageManifest::builtin().unwrap().into_catalog().unwrap();
        let gptimage = catalog.entry("gptimage").unwrap();
        // fal-only default: every route exposes an edit slug, so editing
        // never falls back to a generate-only route.
        assert!(gptimage.routes.iter().all(|r| r.edit_model_id.is_some()));
        assert!(
            gptimage
                .routes
                .iter()
                .all(|r| r.provider_name == "fal"),
            "gptimage is fal-only (#114)"
        );
        // qwenimage: turbo only, no pro default (#118).
        let qwen = catalog.entry("qwenimage").unwrap();
        assert_eq!(qwen.default_route().tier, ImageTier::Turbo);
        assert_eq!(qwen.tiers(), vec![ImageTier::Turbo]);
        assert_eq!(
            qwen.default_route().generate_model_id,
            "qwen/qwen-image-2.1-turbo"
        );
        assert!(qwen.routes.iter().all(|r| r.provider_name == "openrouter"));
    }

    #[test]
    fn test_builtin_manifest_has_no_cross_provider_routes() {
        let catalog = ImageManifest::builtin().unwrap().into_catalog().unwrap();
        for entry in catalog.entries() {
            let providers: HashSet<&str> =
                entry.routes.iter().map(|r| r.provider_name.as_str()).collect();
            assert_eq!(
                providers.len(),
                1,
                "alias '{}' must have exactly one provider route (#114)",
                entry.alias
            );
        }
        let fal_aliases: Vec<&str> = catalog
            .entries()
            .iter()
            .filter(|e| e.default_route().provider_name == "fal")
            .map(|e| e.alias.as_str())
            .collect();
        assert_eq!(fal_aliases, vec!["gptimage", "sunburst"]);
        let openrouter_only = ["banana", "qwenimage", "qwen3", "seedream5flash", "grok2"];
        for alias in openrouter_only {
            assert_eq!(
                catalog.entry(alias).unwrap().default_route().provider_name,
                "openrouter",
                "'{alias}' is OpenRouter-only (#114)"
            );
        }
    }

    #[test]
    fn test_default_alias_is_deprecated_guard() {
        let raw = r#"
[[model]]
alias = "old"
label = "Old"
family = "x"
version = "1"
default = true
deprecated = true
  [[model.route]]
  provider = "fal"
  generate = "x/old"
"#;
        let err = ImageManifest::parse(raw, "test")
            .unwrap()
            .into_catalog()
            .unwrap_err();
        assert!(err.to_string().contains("both default and deprecated"), "{err}");
    }

    #[test]
    fn test_zero_route_model_rejected() {
        let raw = r#"
[[model]]
alias = "broken"
label = "Broken"
family = "x"
version = "1"
default = true
"#;
        let err = ImageManifest::parse(raw, "test")
            .unwrap()
            .into_catalog()
            .unwrap_err();
        assert!(err.to_string().contains("no [[model.route]]"), "{err}");
    }

    #[test]
    fn test_duplicate_alias_rejected() {
        let raw = r#"
[[model]]
alias = "dup"
label = "Dup"
family = "x"
version = "1"
default = true
  [[model.route]]
  provider = "fal"
  generate = "x/one"

[[model]]
alias = "dup"
label = "Dup 2"
family = "x"
version = "1"
  [[model.route]]
  provider = "fal"
  generate = "x/two"
"#;
        let err = ImageManifest::parse(raw, "test")
            .unwrap()
            .into_catalog()
            .unwrap_err();
        assert!(err.to_string().contains("more than once"), "{err}");
    }

    #[test]
    fn test_empty_edit_slug_means_generate_only() {
        let raw = r#"
[[model]]
alias = "m"
label = "M"
family = "x"
version = "1"
default = true
  [[model.route]]
  provider = "fal"
  generate = "x/t2i"
  edit = ""
"#;
        let catalog = ImageManifest::parse(raw, "test")
            .unwrap()
            .into_catalog()
            .unwrap();
        assert_eq!(catalog.entry("m").unwrap().default_route().edit_model_id, None);
    }

    #[test]
    fn test_missing_default_alias_rejected() {
        let raw = r#"
[[model]]
alias = "m"
label = "M"
family = "x"
version = "1"
  [[model.route]]
  provider = "fal"
  generate = "x/t2i"
"#;
        let err = ImageManifest::parse(raw, "test")
            .unwrap()
            .into_catalog()
            .unwrap_err();
        assert!(err.to_string().contains("marks no alias as default"), "{err}");
    }
}
