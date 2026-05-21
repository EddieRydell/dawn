use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;
use ts_rs::TS;

/// The set of built-in effect types shipped with VibeLights.
/// Separated from `EffectKind` so match sites can cleanly distinguish
/// built-in dispatch from script/plugin dispatch.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    TS,
    JsonSchema,
    strum::Display,
    strum::EnumString,
    strum::VariantArray,
    strum::VariantNames,
)]
#[ts(export)]
pub enum BuiltInEffect {
    Solid,
    Chase,
    Rainbow,
    Strobe,
    Gradient,
    Twinkle,
    Fade,
    Wipe,
    Meteor,
}

/// Which effect type an instance uses.
/// `BuiltIn` effects are evaluated via enum dispatch; `Script` effects
/// run through the DSL VM. This two-level structure keeps match sites clean
/// and prepares for future effect sources (WASM, external plugins, etc.).
///
/// Serializes with backward-compatible JSON: built-ins as `"Solid"`, `"Chase"`, etc.
/// and scripts as `{"Script": "name"}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectKind {
    BuiltIn(BuiltInEffect),
    /// A DSL-scripted effect. The string is the script name (key into Show::scripts).
    Script(String),
}

impl From<BuiltInEffect> for EffectKind {
    fn from(b: BuiltInEffect) -> Self {
        EffectKind::BuiltIn(b)
    }
}

impl EffectKind {
    /// All built-in effect kinds (excludes Script).
    pub fn all_builtin() -> &'static [BuiltInEffect] {
        <BuiltInEffect as strum::VariantArray>::VARIANTS
    }
}

// ── Custom serde for EffectKind (backward-compatible JSON) ─────────
//
// BuiltIn(Solid) → "Solid", Script("x") → {"Script":"x"}

impl Serialize for EffectKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            EffectKind::BuiltIn(b) => b.serialize(serializer),
            EffectKind::Script(name) => {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("Script", name)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for EffectKind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EffectKindVisitor;

        impl<'de> serde::de::Visitor<'de> for EffectKindVisitor {
            type Value = EffectKind;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a built-in effect name or {\"Script\": \"name\"}")
            }

            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<Self::Value, E> {
                s.parse::<BuiltInEffect>()
                    .map(EffectKind::BuiltIn)
                    .map_err(|_| {
                        E::unknown_variant(s, <BuiltInEffect as strum::VariantNames>::VARIANTS)
                    })
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let key: String = map
                    .next_key()?
                    .ok_or_else(|| serde::de::Error::missing_field("Script"))?;
                if key == "Script" {
                    let name: String = map.next_value()?;
                    Ok(EffectKind::Script(name))
                } else {
                    Err(serde::de::Error::unknown_field(&key, &["Script"]))
                }
            }
        }

        deserializer.deserialize_any(EffectKindVisitor)
    }
}

// ── Custom ts-rs impl for EffectKind ───────────────────────────────
//
// Produces the same TypeScript as before:
//   type EffectKind = "Solid" | "Chase" | ... | { "Script": string };

impl ts_rs::TS for EffectKind {
    type WithoutGenerics = Self;
    type OptionInnerType = Self;

    fn name(_cfg: &ts_rs::Config) -> String {
        "EffectKind".to_string()
    }

    fn inline(_cfg: &ts_rs::Config) -> String {
        let builtin_parts: Vec<String> = <BuiltInEffect as strum::VariantNames>::VARIANTS
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect();
        format!("{} | {{ \"Script\": string }}", builtin_parts.join(" | "))
    }

    fn decl(cfg: &ts_rs::Config) -> String {
        format!("type EffectKind = {};", Self::inline(cfg))
    }

    fn decl_concrete(cfg: &ts_rs::Config) -> String {
        Self::decl(cfg)
    }

    fn output_path() -> Option<std::path::PathBuf> {
        Some(std::path::PathBuf::from("EffectKind.ts"))
    }

    fn visit_dependencies(_: &mut impl ts_rs::TypeVisitor)
    where
        Self: 'static,
    {
    }
}

// ── Custom schemars impl for EffectKind ────────────────────────────

impl JsonSchema for EffectKind {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "EffectKind".into()
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let builtin_variants: Vec<serde_json::Value> =
            <BuiltInEffect as strum::VariantNames>::VARIANTS
                .iter()
                .map(|name| {
                    serde_json::json!({
                        "type": "string",
                        "const": name,
                    })
                })
                .collect();

        let script_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "Script": { "type": "string" }
            },
            "required": ["Script"],
            "additionalProperties": false,
        });

        let mut all = builtin_variants;
        all.push(script_schema);

        let mut map = serde_json::Map::new();
        map.insert("anyOf".to_string(), serde_json::Value::Array(all));
        schemars::Schema::from(map)
    }
}

// ── Display impl ───────────────────────────────────────────────────

impl fmt::Display for EffectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BuiltIn(b) => write!(f, "{b}"),
            Self::Script(name) => write!(f, "Script({name})"),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn effect_kind_builtin_roundtrip() {
        let kind = EffectKind::BuiltIn(BuiltInEffect::Solid);
        let json = serde_json::to_string(&kind).expect("serialize");
        assert_eq!(json, "\"Solid\"");
        let back: EffectKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, kind);
    }

    #[test]
    fn effect_kind_all_builtins_serialize_as_plain_strings() {
        for b in <BuiltInEffect as strum::VariantArray>::VARIANTS {
            let kind = EffectKind::BuiltIn(*b);
            let json = serde_json::to_string(&kind).expect("serialize");
            // Should be a plain JSON string, e.g. "\"Solid\""
            assert!(
                json.starts_with('"') && json.ends_with('"'),
                "expected string, got {json}"
            );
            let back: EffectKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn effect_kind_script_roundtrip() {
        let kind = EffectKind::Script("my_script".to_string());
        let json = serde_json::to_string(&kind).expect("serialize");
        assert_eq!(json, r#"{"Script":"my_script"}"#);
        let back: EffectKind = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, kind);
    }

    #[test]
    fn effect_kind_from_builtin() {
        let kind: EffectKind = BuiltInEffect::Chase.into();
        assert_eq!(kind, EffectKind::BuiltIn(BuiltInEffect::Chase));
    }
}
