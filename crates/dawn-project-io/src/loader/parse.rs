pub(crate) fn parse_automation_curve(
    path: &Utf8Path,
    value: &Value,
) -> Result<Curve, LoadProjectError> {
    let curve = parse_curve(path, value)?;
    if curve
        .points
        .iter()
        .any(|point| !matches!(point.value, CurveValue::Float(_)))
    {
        return Err(LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_value(path, value),
            message: "automation curves must be float curves".to_string(),
        });
    }
    Ok(curve)
}

pub(crate) fn parse_sequence_layer(
    path: &Utf8Path,
    value: &Value,
) -> Result<SequenceLayer, LoadProjectError> {
    Ok(SequenceLayer {
        id: SequenceLayerId(u32_field(path, value, "id")?),
        name: string_field(path, value, "name")?.to_string(),
        color: parse_color(string_field(path, value, "color")?).map_err(|error| {
            with_yaml_location(
                error,
                path,
                source_range_for_field_value(path, value, "color"),
            )
        })?,
        enabled: optional_field(value, "enabled")
            .map(|enabled| {
                enabled
                    .as_bool()
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: source_range_for_field_value(path, value, "enabled"),
                        message: "layer enabled must be a bool".to_string(),
                    })
            })
            .transpose()?
            .unwrap_or(true),
    })
}

pub(crate) fn parse_automation_binding(
    path: &Utf8Path,
    value: &Value,
) -> Result<AutomationBinding, LoadProjectError> {
    let target = parse_automation_target(path, required_field(path, value, "target")?)?;
    Ok(AutomationBinding {
        target,
        mapping: parse_automation_mapping(path, required_field(path, value, "mapping")?)?,
    })
}

pub(crate) fn parse_automation_target(
    path: &Utf8Path,
    value: &Value,
) -> Result<AutomationTarget, LoadProjectError> {
    Ok(match string_field(path, value, "type")? {
        "effect_param" => AutomationTarget::EffectParam {
            effect_id: EffectInstId(u32_field(path, value, "effect_id")?),
            param: parse_identifier_field(path, value, "param")?,
        },
        "composition_node_param" => AutomationTarget::CompositionNodeParam {
            node_id: CompositionGraphNodeId(u32_field(path, value, "node_id")?),
            param: parse_identifier_field(path, value, "param")?,
        },
        other => {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: source_range_for_field_value(path, value, "type"),
                message: format!("unsupported automation target `{other}`"),
            });
        }
    })
}

pub(crate) fn parse_automation_mapping(
    path: &Utf8Path,
    value: &Value,
) -> Result<AutomationMapping, LoadProjectError> {
    Ok(match string_field(path, value, "type")? {
        "float" => AutomationMapping::Float {
            min: f64_field(path, value, "min")?,
            max: f64_field(path, value, "max")?,
        },
        "int" => AutomationMapping::Int {
            min: i64_field(path, value, "min")?,
            max: i64_field(path, value, "max")?,
        },
        "bool" => AutomationMapping::Bool,
        "enum" => AutomationMapping::Enum {
            values: sequence_field(path, value, "values")?
                .into_iter()
                .map(|enum_value| {
                    Identifier::new(enum_value).map_err(|_| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: source_range_for_field_value(path, value, "values"),
                        message: "enum automation values must be identifiers".to_string(),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
        },
        "float_curve" => AutomationMapping::FloatCurve {
            min: f64_field(path, value, "min")?,
            max: f64_field(path, value, "max")?,
        },
        other => {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: source_range_for_field_value(path, value, "type"),
                message: format!("unsupported automation mapping `{other}`"),
            });
        }
    })
}

pub(crate) fn parse_identifier_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<Identifier, LoadProjectError> {
    let raw = string_field(path, value, key)?;
    Identifier::new(raw.to_string()).map_err(|_| LoadProjectError::InvalidDocument {
        path: path.to_path_buf(),
        range: source_range_for_field_value(path, value, key),
        message: format!("invalid identifier `{raw}`"),
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct AliasObjectKey {
    pub(crate) alias: Option<String>,
    pub(crate) object: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ResolvedObject {
    Project(ProjectId),
    Setup(SetupId),
    Controller(ControllerId),
    Layout(LayoutId),
    Patch(PatchId),
    FixtureDefinition(FixtureDefinitionId),
    Curve(CurveId),
    Sequence(SequenceId),
    EffectDefinition(EffectDefinitionId),
    OperatorDefinition(OperatorDefinitionId),
}

impl ResolvedObject {
    pub(crate) fn source_identity(&self) -> &SourceIdentity {
        match self {
            Self::Project(id) => &id.0,
            Self::Setup(id) => &id.0,
            Self::Controller(id) => &id.0,
            Self::Layout(id) => &id.0,
            Self::Patch(id) => &id.0,
            Self::FixtureDefinition(id) => &id.0,
            Self::Curve(id) => &id.0,
            Self::Sequence(id) => &id.0,
            Self::EffectDefinition(id) => &id.0,
            Self::OperatorDefinition(id) => &id.0,
        }
    }

    pub(crate) fn source_kind(&self) -> SourceObjectKind {
        match self {
            Self::Project(_) => SourceObjectKind::Project,
            Self::Setup(_) => SourceObjectKind::Setup,
            Self::Controller(_) => SourceObjectKind::Controller,
            Self::Layout(_) => SourceObjectKind::Layout,
            Self::Patch(_) => SourceObjectKind::Patch,
            Self::FixtureDefinition(_) => SourceObjectKind::FixtureDefinition,
            Self::Curve(_) => SourceObjectKind::Curve,
            Self::Sequence(_) => SourceObjectKind::Sequence,
            Self::EffectDefinition(_) => SourceObjectKind::EffectDefinition,
            Self::OperatorDefinition(_) => SourceObjectKind::OperatorDefinition,
        }
    }

    pub(crate) fn id_string(&self) -> String {
        match self {
            Self::Project(id) => id.0.object().to_string(),
            Self::Setup(id) => id.0.object().to_string(),
            Self::Controller(id) => id.0.object().to_string(),
            Self::Layout(id) => id.0.object().to_string(),
            Self::Patch(id) => id.0.object().to_string(),
            Self::FixtureDefinition(id) => id.0.object().to_string(),
            Self::Curve(id) => id.0.object().to_string(),
            Self::Sequence(id) => id.0.object().to_string(),
            Self::EffectDefinition(id) => id.0.object().to_string(),
            Self::OperatorDefinition(id) => id.0.object().to_string(),
        }
    }
}

pub(crate) struct SourceObjectValue<'a> {
    pub(crate) key: String,
    pub(crate) value: &'a Value,
}

#[derive(Clone, Debug)]
pub(crate) struct ParsedImport {
    pub(crate) from: Utf8PathBuf,
    pub(crate) alias: String,
}

pub(crate) fn parse_imports(
    path: &Utf8Path,
    map: &Mapping,
) -> Result<Vec<ParsedImport>, LoadProjectError> {
    let Some(imports) = map.get(Value::String("imports".to_string())) else {
        return Ok(Vec::new());
    };
    let imports = imports
        .as_sequence()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: None,
            message: "imports must be a sequence".to_string(),
        })?;
    imports
        .iter()
        .map(|import| {
            Ok(ParsedImport {
                from: Utf8PathBuf::from(string_field(path, import, "from")?),
                alias: string_field(path, import, "as")?.to_string(),
            })
        })
        .collect()
}

pub(crate) fn parse_layout_target(
    path: &Utf8Path,
    value: &Value,
) -> Result<LayoutTarget, LoadProjectError> {
    match string_field(path, value, "type")? {
        "fixture" => Ok(LayoutTarget::Fixture(FixtureInstanceId(u32_field(
            path, value, "id",
        )?))),
        "group" => Ok(LayoutTarget::Group(FixtureGroupId(u32_field(
            path, value, "id",
        )?))),
        other => Err(LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, "type"),
            message: format!("invalid layout target type `{other}`"),
        }),
    }
}

pub(crate) fn parse_fixture_group(
    path: &Utf8Path,
    value: &Value,
) -> Result<FixtureGroup, LoadProjectError> {
    Ok(FixtureGroup {
        id: FixtureGroupId(u32_field(path, value, "id")?),
        name: string_field(path, value, "name")?.to_string(),
        fixtures: sequence_values(path, value, "members")?
            .iter()
            .map(|member| {
                member
                    .as_u64()
                    .map(|value| FixtureInstanceId(value as u32))
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: None,
                        message: "group members must be fixture ids".to_string(),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?,
    })
}

pub(crate) fn parse_mark_collection(
    path: &Utf8Path,
    value: &Value,
) -> Result<MarkCollection, LoadProjectError> {
    Ok(MarkCollection {
        key: MarkCollectionKey {
            name: string_field(path, value, "key")?.to_string(),
        },
        name: string_field(path, value, "name")?.to_string(),
        display_color: parse_color(string_field(path, value, "color")?).map_err(|error| {
            with_yaml_location(
                error,
                path,
                source_range_for_field_value(path, value, "color"),
            )
        })?,
        marks: sequence_values(path, value, "marks")?
            .iter()
            .map(|mark| {
                mark.as_str()
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: None,
                        message: "marks must be duration strings".to_string(),
                    })
                    .and_then(|duration| {
                        parse_duration_as_time(duration).map_err(|error| {
                            with_yaml_location(error, path, source_range_for_value(path, mark))
                        })
                    })
            })
            .collect::<Result<Vec<_>, _>>()?,
    })
}

pub(crate) fn parse_effect_target(
    path: &Utf8Path,
    value: &Value,
) -> Result<EffectTarget, LoadProjectError> {
    match string_field(path, value, "type")? {
        "fixture" => Ok(EffectTarget::Fixture(FixtureInstanceId(u32_field(
            path, value, "id",
        )?))),
        "group" => Ok(EffectTarget::Group(FixtureGroupId(u32_field(
            path, value, "id",
        )?))),
        other => Err(LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, "type"),
            message: format!("invalid effect target type `{other}`"),
        }),
    }
}

pub(crate) fn parse_effect_scope(
    path: &Utf8Path,
    value: &Value,
) -> Result<EffectScope, LoadProjectError> {
    match string_field(path, value, "scope")? {
        "per_fixture" => Ok(EffectScope::PerFixture),
        "whole_target" => Ok(EffectScope::WholeTarget),
        other => Err(LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, "scope"),
            message: format!("invalid effect scope `{other}`"),
        }),
    }
}

pub(crate) fn parse_graph_position(
    path: &Utf8Path,
    value: &Value,
) -> Result<GraphNodePosition, LoadProjectError> {
    Ok(GraphNodePosition {
        x: f64_field(path, value, "x")?,
        y: f64_field(path, value, "y")?,
    })
}

pub(crate) fn parse_graph_edge(
    path: &Utf8Path,
    value: &Value,
) -> Result<EffectGraphEdge, LoadProjectError> {
    Ok(EffectGraphEdge {
        from: CompositionGraphNodeId(u32_field(path, value, "from")?),
        from_port: GraphPortId(string_field(path, value, "from_port")?.to_string()),
        to: CompositionGraphNodeId(u32_field(path, value, "to")?),
        to_port: GraphPortId(string_field(path, value, "to_port")?.to_string()),
    })
}

pub(crate) fn parse_fixture_definition(
    path: &Utf8Path,
    value: &Value,
) -> Result<FixtureDefinition, LoadProjectError> {
    let bulb_diameter = f64_field(path, value, "bulb_diameter")?;
    let geometry_value = required_field(path, value, "geometry")?;
    let geometry = match string_field(path, geometry_value, "type")? {
        "points" => Geometry::Points {
            points: sequence_values(path, geometry_value, "points")?
                .iter()
                .map(parse_point3)
                .collect::<Result<Vec<_>, _>>()?,
        },
        "lines" => Geometry::Lines {
            points: sequence_values(path, geometry_value, "points")?
                .iter()
                .map(parse_point3)
                .collect::<Result<Vec<_>, _>>()?,
            pixels: u32_field(path, geometry_value, "pixels")?,
        },
        "arc" => Geometry::Arc {
            center: parse_point3(required_field(path, geometry_value, "center")?)?,
            radius: DistanceSpan::from_meters(f64_field(path, geometry_value, "radius")?),
            start_degrees: f64_field(path, geometry_value, "startDegrees")?,
            end_degrees: f64_field(path, geometry_value, "endDegrees")?,
            pixels: u32_field(path, geometry_value, "pixels")?,
        },
        other => {
            return Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: source_range_for_field_value(path, geometry_value, "type"),
                message: format!("unsupported fixture geometry `{other}`"),
            });
        }
    };
    Ok(FixtureDefinition {
        bulb_radius: DistanceSpan::from_meters(bulb_diameter / 2.0),
        geometry,
    })
}

pub(crate) fn parse_curve(path: &Utf8Path, value: &Value) -> Result<Curve, LoadProjectError> {
    let value_type = string_field(path, value, "value_type")?;
    let points = sequence_values(path, value, "points")?
        .iter()
        .map(|point| {
            let position = f64_field(path, point, "time")?;
            let value = required_field(path, point, "value")?;
            let value = match value_type {
                "float" => CurveValue::Float(value.as_f64().ok_or_else(|| {
                    LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: None,
                        message: "curve float point must be numeric".to_string(),
                    }
                })?),
                "color" => CurveValue::Color(
                    parse_color(value.as_str().ok_or_else(|| {
                        LoadProjectError::InvalidDocument {
                            path: path.to_path_buf(),
                            range: None,
                            message: "curve color point must be a color string".to_string(),
                        }
                    })?)
                    .map_err(|error| {
                        with_yaml_location(error, path, source_range_for_value(path, value))
                    })?,
                ),
                other => {
                    return Err(LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: source_range_for_field_value(path, value, "value_type"),
                        message: format!("unsupported curve value type `{other}`"),
                    });
                }
            };
            Ok(CurvePoint { position, value })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Curve { points })
}

pub(crate) fn parse_point3(value: &Value) -> Result<Point3, LoadProjectError> {
    Ok(Point3 {
        x: Distance::from_meters(f64_field(Utf8Path::new("<inline>"), value, "x")?),
        y: Distance::from_meters(f64_field(Utf8Path::new("<inline>"), value, "y")?),
        z: Distance::from_meters(f64_field(Utf8Path::new("<inline>"), value, "z")?),
    })
}

pub(crate) fn parse_rotation3(value: &Value) -> Result<Rotation3, LoadProjectError> {
    Ok(Rotation3 {
        x: f64_field(Utf8Path::new("<inline>"), value, "x")?,
        y: f64_field(Utf8Path::new("<inline>"), value, "y")?,
        z: f64_field(Utf8Path::new("<inline>"), value, "z")?,
    })
}

pub(crate) fn parse_scale3(value: &Value) -> Result<Scale3, LoadProjectError> {
    Ok(Scale3 {
        x: f64_field(Utf8Path::new("<inline>"), value, "x")?,
        y: f64_field(Utf8Path::new("<inline>"), value, "y")?,
        z: f64_field(Utf8Path::new("<inline>"), value, "z")?,
    })
}

pub(crate) fn parse_controller_address(value: &str) -> Result<ControllerAddress, String> {
    let (ip, port) = value
        .split_once(':')
        .ok_or_else(|| "controller destination must be ip:port".to_string())?;
    let ip = ip
        .parse()
        .map_err(|_| "controller destination ip is invalid".to_string())?;
    let port = port
        .parse()
        .map_err(|_| "controller destination port is invalid".to_string())?;
    Ok(ControllerAddress { ip, port })
}

pub(crate) fn parse_channel_order(value: &str) -> Option<RgbChannelOrder> {
    match value {
        "rgb" => Some(RgbChannelOrder::Rgb),
        "rbg" => Some(RgbChannelOrder::Rbg),
        "grb" => Some(RgbChannelOrder::Grb),
        "gbr" => Some(RgbChannelOrder::Gbr),
        "brg" => Some(RgbChannelOrder::Brg),
        "bgr" => Some(RgbChannelOrder::Bgr),
        _ => None,
    }
}

pub(crate) fn parse_slot_range(value: &str) -> Option<usize> {
    let (start, end) = value.split_once("..")?;
    let start = start.parse::<usize>().ok()?;
    let end = end.parse::<usize>().ok()?;
    end.checked_sub(start).map(|slots| slots + 1)
}

pub(crate) fn parse_duration(value: &str) -> Result<DawnDuration, LoadProjectError> {
    Ok(DawnDuration::from_seconds_f64(parse_seconds(value)?))
}

pub(crate) fn parse_duration_as_time(value: &str) -> Result<DawnTime, LoadProjectError> {
    Ok(DawnTime::from_seconds_f64(parse_seconds(value)?))
}

pub(crate) fn parse_seconds(value: &str) -> Result<f64, LoadProjectError> {
    let seconds = value
        .strip_suffix('s')
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: Utf8PathBuf::from("<duration>"),
            range: None,
            message: format!("duration must end in `s`: {value}"),
        })?;
    seconds
        .parse()
        .map_err(|_| LoadProjectError::InvalidDocument {
            path: Utf8PathBuf::from("<duration>"),
            range: None,
            message: format!("invalid duration: {value}"),
        })
}

pub(crate) fn parse_color(value: &str) -> Result<Color, LoadProjectError> {
    Color::from_hex(value).ok_or_else(|| LoadProjectError::InvalidDocument {
        path: Utf8PathBuf::from("<color>"),
        range: None,
        message: format!("invalid color: {value}"),
    })
}

pub(crate) fn mapping(value: &Value) -> Option<&Mapping> {
    match value {
        Value::Mapping(mapping) => Some(mapping),
        _ => None,
    }
}

pub(crate) fn required_field<'a>(
    path: &Utf8Path,
    value: &'a Value,
    key: &str,
) -> Result<&'a Value, LoadProjectError> {
    mapping(value)
        .and_then(|mapping| mapping.get(Value::String(key.to_string())))
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_value(path, value),
            message: format!("missing field `{key}`"),
        })
}

pub(crate) fn optional_field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    mapping(value).and_then(|mapping| mapping.get(Value::String(key.to_string())))
}

pub(crate) fn optional_mapping<'a>(value: &'a Value, key: &str) -> Option<&'a Mapping> {
    optional_field(value, key).and_then(mapping)
}

pub(crate) fn optional_mapping_ref<'a>(value: &'a Value, key: &str) -> Option<&'a Mapping> {
    optional_field(value, key).and_then(mapping)
}

pub(crate) fn optional_sequence<'a>(value: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    optional_field(value, key).and_then(Value::as_sequence)
}

pub(crate) fn sequence_values<'a>(
    path: &Utf8Path,
    value: &'a Value,
    key: &str,
) -> Result<&'a Vec<Value>, LoadProjectError> {
    required_field(path, value, key)?
        .as_sequence()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a sequence"),
        })
}

pub(crate) fn sequence_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<Vec<String>, LoadProjectError> {
    sequence_values(path, value, key)?
        .iter()
        .map(|value| {
            value.as_str().map(ToString::to_string).ok_or_else(|| {
                LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_value(path, value),
                    message: format!("field `{key}` values must be strings"),
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()
}

pub(crate) fn string_field<'a>(
    path: &Utf8Path,
    value: &'a Value,
    key: &str,
) -> Result<&'a str, LoadProjectError> {
    required_field(path, value, key)?
        .as_str()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a string"),
        })
}

pub(crate) fn optional_string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    optional_field(value, key).and_then(Value::as_str)
}

pub(crate) fn u32_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<u32, LoadProjectError> {
    required_field(path, value, key)?
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a u32"),
        })
}

pub(crate) fn usize_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<usize, LoadProjectError> {
    required_field(path, value, key)?
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a usize"),
        })
}

pub(crate) fn i64_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<i64, LoadProjectError> {
    required_field(path, value, key)?
        .as_i64()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be an integer"),
        })
}

pub(crate) fn f64_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<f64, LoadProjectError> {
    required_field(path, value, key)?
        .as_f64()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a number"),
        })
}

pub(crate) fn bool_field(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Result<bool, LoadProjectError> {
    required_field(path, value, key)?
        .as_bool()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_field_value(path, value, key),
            message: format!("field `{key}` must be a bool"),
        })
}

pub(crate) fn relative_path(
    root: &Utf8Path,
    path: &Utf8Path,
) -> Result<Utf8PathBuf, LoadProjectError> {
    path.strip_prefix(root)
        .map(Utf8Path::to_path_buf)
        .map_err(|_| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: None,
            message: format!("path is outside source root {root}"),
        })
}

pub(crate) fn normalize_relative(path: Utf8PathBuf) -> Utf8PathBuf {
    let original = path.clone();
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            camino::Utf8Component::CurDir => {}
            camino::Utf8Component::ParentDir => {
                if parts.last().is_some_and(|part| part != "..") {
                    let _ = parts.pop();
                } else {
                    parts.push("..".to_string());
                }
            }
            camino::Utf8Component::Normal(part) => parts.push(part.to_string()),
            camino::Utf8Component::RootDir | camino::Utf8Component::Prefix(_) => {
                return original;
            }
        }
    }
    parts.into_iter().collect()
}
use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::dsl::Identifier;
use dawn_language::effect::{CurveId, EffectDefinitionId, EffectInstId, EffectScope, EffectTarget};
use dawn_language::identity::SourceIdentity;
use dawn_language::model::ProjectId;
use dawn_language::operator::OperatorDefinitionId;
use dawn_language::sequence::{
    AutomationBinding, AutomationMapping, AutomationTarget, CompositionGraphNodeId,
    EffectGraphEdge, GraphNodePosition, GraphPortId, MarkCollection, MarkCollectionKey, SequenceId,
    SequenceLayer, SequenceLayerId,
};
use dawn_language::setup::{
    ControllerAddress, ControllerId, FixtureDefinition, FixtureDefinitionId, FixtureGroup,
    FixtureGroupId, FixtureInstanceId, Geometry, LayoutId, LayoutTarget, PatchId, RgbChannelOrder,
    SetupId,
};
use dawn_language::values::{
    Color, Curve, CurvePoint, CurveValue, DawnDuration, DawnTime, Distance, DistanceSpan, Point3,
    Rotation3, Scale3,
};
use yaml_serde::{Mapping, Value};

use crate::diagnostics::{
    source_range_for_field_value, source_range_for_value, with_yaml_location,
};
use crate::{LoadProjectError, SourceObjectKind};
