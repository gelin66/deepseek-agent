//! Feature flags and metadata for dse.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use dse_localization::{MessageId, tr};
use serde::{Deserialize, Deserializer, Serialize, de};

/// Unique features toggled via configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Feature {
    /// Enable background sub-agent tooling.
    Subagents,
}

/// Holds the effective set of enabled features.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Features {
    enabled: BTreeSet<Feature>,
}

impl Features {
    /// Starts with built-in defaults.
    pub fn with_defaults() -> Self {
        let mut set = BTreeSet::new();
        for spec in FEATURES {
            if spec.default_enabled {
                set.insert(spec.id);
            }
        }
        Self { enabled: set }
    }

    pub fn enabled(&self, feature: Feature) -> bool {
        self.enabled.contains(&feature)
    }

    pub fn enable(&mut self, feature: Feature) -> &mut Self {
        self.enabled.insert(feature);
        self
    }

    pub fn disable(&mut self, feature: Feature) -> &mut Self {
        self.enabled.remove(&feature);
        self
    }

    pub fn apply_map(&mut self, entries: &BTreeMap<String, bool>) {
        for (key, enabled) in entries {
            if let Some(feature) = feature_from_key(key) {
                if *enabled {
                    self.enable(feature);
                } else {
                    self.disable(feature);
                }
            }
        }
    }
}

/// Keys accepted in `[features]` tables.
pub fn is_known_feature_key(key: &str) -> bool {
    FEATURES.iter().any(|spec| spec.key == key)
}

pub fn feature_from_key(key: &str) -> Option<Feature> {
    FEATURES
        .iter()
        .find(|spec| spec.key == key)
        .map(|spec| spec.id)
}

pub fn render_feature_table(features: &Features) -> String {
    let mut output = tr(MessageId::FeaturesHeader).into_owned();
    for spec in FEATURES {
        let _ = writeln!(output, "{}\t{}", spec.key, features.enabled(spec.id));
    }
    output
}

/// Deserializable features table for TOML.
#[derive(Serialize, Debug, Clone, Default, PartialEq)]
pub struct FeaturesToml {
    #[serde(flatten)]
    pub entries: BTreeMap<String, bool>,
}

impl<'de> Deserialize<'de> for FeaturesToml {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = BTreeMap::<String, toml::Value>::deserialize(deserializer)?;
        let mut entries = BTreeMap::new();

        for (key, value) in raw {
            match value {
                toml::Value::Boolean(enabled) => {
                    entries.insert(key, enabled);
                }
                toml::Value::Table(table) if key == "enabled" => {
                    for (feature_key, feature_value) in table {
                        match feature_value {
                            toml::Value::Boolean(enabled) => {
                                entries.insert(feature_key, enabled);
                            }
                            other => {
                                return Err(de::Error::custom(format!(
                                    "features.enabled.{feature_key} must be a boolean, got {other:?}"
                                )));
                            }
                        }
                    }
                }
                other if is_known_feature_key(&key) => {
                    return Err(de::Error::custom(format!(
                        "features.{key} must be a boolean, got {other:?}"
                    )));
                }
                _ => {}
            }
        }

        Ok(Self { entries })
    }
}

/// Single registry of all feature definitions.
#[derive(Debug, Clone, Copy)]
struct FeatureSpec {
    id: Feature,
    key: &'static str,
    default_enabled: bool,
}

const FEATURES: &[FeatureSpec] = &[FeatureSpec {
    id: Feature::Subagents,
    key: "subagents",
    default_enabled: true,
}];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_map_toggles_known_features_and_ignores_unknown_keys() {
        let mut features = Features::with_defaults();
        let entries = BTreeMap::from([
            ("subagents".to_string(), false),
            ("not_real".to_string(), false),
        ]);

        features.apply_map(&entries);

        assert!(!features.enabled(Feature::Subagents));
        assert_eq!(feature_from_key("not_real"), None);
    }

    #[test]
    fn render_feature_table_uses_registry_order_and_effective_state() {
        let mut features = Features::with_defaults();
        features.disable(Feature::Subagents);

        let table = render_feature_table(&features);
        let lines = table.lines().collect::<Vec<_>>();

        assert_eq!(lines.first(), Some(&"功能\t启用"));
        assert!(lines.contains(&"subagents\tfalse"));
    }
}
