pub mod types {
    use std::sync::Arc;

    use indexmap::IndexMap;

    use crate::dsl::ast::ParamType as DslParamType;
    use crate::dsl::compiler::CompiledScript;
    use crate::effects;
    use crate::model::{
        Color, ColorGradient, Curve, EffectKind, ParamKey, ParamSchema, ParamType, ParamValue,
    };

    pub fn effect_schema_for_kind(
        kind: &EffectKind,
        script_cache: Option<&IndexMap<String, Arc<CompiledScript>>>,
    ) -> Result<Vec<ParamSchema>, String> {
        match kind {
            EffectKind::BuiltIn(effect) => Ok(effects::effect_schema(effect)),
            EffectKind::Script(name) => script_cache
                .and_then(|cache| cache.get(name))
                .map(|script| extract_script_schemas(script))
                .ok_or_else(|| format!("script effect '{name}' is not compiled")),
        }
    }

    pub fn extract_script_schemas(compiled: &CompiledScript) -> Vec<ParamSchema> {
        compiled
            .params
            .iter()
            .map(|param| {
                let param_type = match &param.ty {
                    DslParamType::Float(range) => {
                        let (min, max) = range.unwrap_or((0.0, 1.0));
                        ParamType::Float {
                            min,
                            max,
                            step: 0.01,
                        }
                    }
                    DslParamType::Int(range) => {
                        let (min, max) = range.unwrap_or((0, 100));
                        ParamType::Int { min, max }
                    }
                    DslParamType::Bool => ParamType::Bool,
                    DslParamType::Color => ParamType::Color,
                    DslParamType::Gradient => ParamType::ColorGradient {
                        min_stops: 2,
                        max_stops: 8,
                    },
                    DslParamType::Curve => ParamType::Curve,
                    DslParamType::Path => ParamType::Path,
                    DslParamType::Named(name) => compiled
                        .enums
                        .iter()
                        .find(|item| item.name == *name)
                        .map(|item| ParamType::Enum {
                            options: item.variants.clone(),
                        })
                        .or_else(|| {
                            compiled
                                .flags
                                .iter()
                                .find(|item| item.name == *name)
                                .map(|item| ParamType::Flags {
                                    options: item.flags.clone(),
                                })
                        })
                        .unwrap_or_else(|| ParamType::Text {
                            options: Vec::new(),
                        }),
                };
                let default = schema_placeholder_default(&param_type);
                ParamSchema {
                    key: ParamKey::Custom(param.name.clone()),
                    label: param.name.clone(),
                    param_type,
                    default,
                }
            })
            .collect()
    }

    fn schema_placeholder_default(param_type: &ParamType) -> ParamValue {
        match param_type {
            ParamType::Float { min, .. } => ParamValue::Float(*min),
            ParamType::Int { min, .. } => ParamValue::Int(*min),
            ParamType::Bool => ParamValue::Bool(false),
            ParamType::Color => ParamValue::Color(Color::WHITE),
            ParamType::ColorList { min_colors, .. } => {
                ParamValue::ColorList(vec![Color::WHITE; (*min_colors).max(1)])
            }
            ParamType::Curve => ParamValue::Curve(Curve::linear()),
            ParamType::ColorGradient { .. } => {
                ParamValue::ColorGradient(ColorGradient::solid(Color::WHITE))
            }
            ParamType::ColorMode { .. } => ParamValue::ColorMode(crate::model::ColorMode::Static),
            ParamType::WipeDirection { .. } => {
                ParamValue::WipeDirection(crate::model::WipeDirection::Horizontal)
            }
            ParamType::Text { options } => {
                ParamValue::Text(options.first().cloned().unwrap_or_default())
            }
            ParamType::Enum { options } => {
                ParamValue::EnumVariant(options.first().cloned().unwrap_or_default())
            }
            ParamType::Flags { .. } => ParamValue::FlagSet(Vec::new()),
            ParamType::Path => ParamValue::PathRef(String::new()),
        }
    }
}
