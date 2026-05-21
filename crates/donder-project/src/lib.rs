use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use donder_core::dsl::{compile_source, compiler::CompiledScript};
use donder_core::effects;
use donder_core::engine::Frame;
use donder_core::model::{
    BlendMode, BuiltInEffect, ChannelOrder, Color, ColorGradient, ColorModel, Controller,
    ControllerId, ControllerProtocol, EffectId, EffectInstance, EffectKind, EffectParams,
    FixtureDef, FixtureGroup, FixtureId, GroupId, GroupMember, Layout, LayoutShape, NodeId,
    NodeTimeline, OutputMapping, ParamSchema, ParamType, ParamValue, Patch, PixelType,
    Position2D, Sequence, Show, TimeRange, TrackItem,
};
use indexmap::IndexMap;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub path: PathBuf,
    pub message: String,
}

impl Diagnostic {
    fn error(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self {
            severity: DiagnosticSeverity::Error,
            path: path.into(),
            message: message.into(),
        }
    }

    fn warning(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self {
            severity: DiagnosticSeverity::Warning,
            path: path.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("project has validation errors")]
    Validation { diagnostics: Vec<Diagnostic> },
    #[error("{0}")]
    Message(String),
}

pub type Result<T> = std::result::Result<T, ProjectError>;

#[derive(Debug, Clone)]
pub struct CompiledProject {
    pub root: PathBuf,
    pub project_file: PathBuf,
    pub show: Show,
    pub script_cache: IndexMap<String, Arc<CompiledScript>>,
    pub symbol_table: SymbolTable,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Default, Clone)]
pub struct SymbolTable {
    pub fixtures: BTreeMap<String, FixtureId>,
    pub groups: BTreeMap<String, GroupId>,
    pub controllers: BTreeMap<String, ControllerId>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectDoc {
    kind: String,
    version: u32,
    name: String,
    #[serde(default)]
    displays: Vec<String>,
    #[serde(default)]
    sequences: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DisplayDoc {
    kind: String,
    version: u32,
    id: String,
    #[serde(rename = "name")]
    _name: String,
    #[serde(default)]
    fixtures: Vec<FixtureSource>,
    #[serde(default)]
    groups: Vec<GroupSource>,
    #[serde(default)]
    controllers: Vec<String>,
    #[serde(default)]
    layouts: Vec<String>,
    #[serde(default)]
    patches: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ControllerDoc {
    kind: String,
    version: u32,
    #[serde(default)]
    controllers: Vec<ControllerSource>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LayoutDoc {
    kind: String,
    version: u32,
    #[serde(default)]
    fixtures: Vec<LayoutSource>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchDoc {
    kind: String,
    version: u32,
    #[serde(default)]
    patches: Vec<PatchSource>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SequenceDoc {
    kind: String,
    version: u32,
    id: String,
    name: String,
    display: String,
    duration: f64,
    #[serde(default = "default_frame_rate")]
    frame_rate: f64,
    #[serde(default)]
    audio: Option<String>,
    #[serde(default)]
    scripts: Vec<ScriptSource>,
    #[serde(default)]
    events: Vec<EventSource>,
}

fn default_frame_rate() -> f64 {
    40.0
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureSource {
    id: String,
    name: String,
    pixel_count: u32,
    #[serde(default = "default_color_model")]
    color_model: ColorModel,
    #[serde(default = "default_channel_order")]
    channel_order: ChannelOrder,
    #[serde(default)]
    pixel_type: Option<PixelType>,
}

fn default_color_model() -> ColorModel {
    ColorModel::Rgb
}

fn default_channel_order() -> ChannelOrder {
    ChannelOrder::Rgb
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GroupSource {
    id: String,
    name: String,
    #[serde(default)]
    members: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ControllerSource {
    id: String,
    name: String,
    #[serde(default)]
    address: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LayoutSource {
    fixture: String,
    #[serde(default)]
    shape: Option<LayoutShapeSource>,
    #[serde(default)]
    positions: Vec<Position2D>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
enum LayoutShapeSource {
    Line { start: Position2D, end: Position2D },
    Grid {
        top_left: Position2D,
        bottom_right: Position2D,
        columns: u32,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PatchSource {
    fixture: String,
    controller: String,
    #[serde(default)]
    port: u16,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScriptSource {
    id: String,
    path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventSource {
    target: String,
    effect: String,
    start: f64,
    duration: f64,
    #[serde(default)]
    params: serde_json::Map<String, serde_json::Value>,
    #[serde(default = "default_opacity")]
    opacity: f64,
    #[serde(default)]
    blend: Option<BlendMode>,
}

fn default_opacity() -> f64 {
    1.0
}

pub fn check_project(project: impl AsRef<Path>) -> Result<CompiledProject> {
    let project_file = normalize_project_path(project.as_ref());
    let root = project_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut diagnostics = Vec::new();
    let mut graph = GraphLoader::new(root.clone());
    let project_doc: ProjectDoc = graph.load_jsonc(&project_file, "project", &mut diagnostics)?;
    validate_header(
        &project_doc.kind,
        project_doc.version,
        "project",
        &project_file,
        &mut diagnostics,
    );

    let mut displays = Vec::new();
    for include in &project_doc.displays {
        let path = resolve_include(&project_file, include, &root, &mut diagnostics);
        let doc: DisplayDoc = graph.load_jsonc(&path, "display", &mut diagnostics)?;
        validate_header(&doc.kind, doc.version, "display", &path, &mut diagnostics);
        displays.push((path, doc));
    }

    if displays.is_empty() {
        diagnostics.push(Diagnostic::error(&project_file, "project must include at least one display"));
    }

    let mut symbol_table = SymbolTable::default();
    let mut fixtures = Vec::new();
    let mut groups = Vec::new();
    let mut controllers = Vec::new();
    let mut layout_fixtures = Vec::new();
    let mut patches = Vec::new();
    let mut display_ids = BTreeSet::new();

    for (display_path, display) in &displays {
        if !display_ids.insert(display.id.clone()) {
            diagnostics.push(Diagnostic::error(display_path, format!("duplicate display id '{}'", display.id)));
        }
        compile_display(
            display_path,
            display,
            &root,
            &mut graph,
            &mut symbol_table,
            &mut fixtures,
            &mut groups,
            &mut controllers,
            &mut layout_fixtures,
            &mut patches,
            &mut diagnostics,
        )?;
    }

    validate_group_members(&groups, &fixtures, &mut diagnostics, &project_file);
    validate_layouts(&layout_fixtures, &fixtures, &mut diagnostics, &project_file);
    validate_patches(&patches, &fixtures, &controllers, &mut diagnostics, &project_file);

    let mut sequences = Vec::new();
    let mut script_cache = IndexMap::new();
    for include in &project_doc.sequences {
        let path = resolve_include(&project_file, include, &root, &mut diagnostics);
        let doc: SequenceDoc = graph.load_jsonc(&path, "sequence", &mut diagnostics)?;
        validate_header(&doc.kind, doc.version, "sequence", &path, &mut diagnostics);
        let sequence = compile_sequence(
            &path,
            &doc,
            &root,
            &display_ids,
            &symbol_table,
            &mut script_cache,
            &mut diagnostics,
        )?;
        sequences.push(sequence);
    }

    let show = Show {
        name: project_doc.name,
        fixtures,
        groups,
        layout: Layout {
            fixtures: layout_fixtures,
        },
        sequences,
        patches,
        controllers,
    };

    detect_cycles(&graph.edges, &mut diagnostics);
    if diagnostics.iter().any(|d| d.severity == DiagnosticSeverity::Error) {
        return Err(ProjectError::Validation { diagnostics });
    }

    Ok(CompiledProject {
        root,
        project_file,
        show,
        script_cache,
        symbol_table,
        diagnostics,
    })
}

pub fn render_frame(sequence_file: impl AsRef<Path>, time: f64) -> Result<Frame> {
    let sequence_file = fs::canonicalize(sequence_file.as_ref())
        .map_err(|err| ProjectError::Message(format!("could not read sequence path: {err}")))?;
    let project_file = find_project_for_sequence(&sequence_file)?;
    let compiled = check_project(project_file)?;
    let sequence_index = compiled
        .show
        .sequences
        .iter()
        .position(|sequence| sequence.name == sequence_name_from_path(&sequence_file))
        .unwrap_or(0);
    Ok(donder_core::evaluate(
        &compiled.show,
        sequence_index,
        time,
        None,
        Some(&compiled.script_cache),
        &HashMap::new(),
    ))
}

fn find_project_for_sequence(sequence_file: &Path) -> Result<PathBuf> {
    for ancestor in sequence_file.ancestors() {
        let candidate = ancestor.join("project.jsonc");
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(ProjectError::Message(format!(
        "could not find project.jsonc above {}",
        sequence_file.display()
    )))
}

fn sequence_name_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("sequence")
        .to_string()
}

fn compile_display(
    display_path: &Path,
    display: &DisplayDoc,
    root: &Path,
    graph: &mut GraphLoader,
    symbols: &mut SymbolTable,
    fixtures: &mut Vec<FixtureDef>,
    groups: &mut Vec<FixtureGroup>,
    controllers: &mut Vec<Controller>,
    layout_fixtures: &mut Vec<donder_core::model::FixtureLayout>,
    patches: &mut Vec<Patch>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<()> {
    for source in &display.fixtures {
        if symbols.fixtures.contains_key(&source.id) {
            diagnostics.push(Diagnostic::error(display_path, format!("duplicate fixture id '{}'", source.id)));
            continue;
        }
        let id = FixtureId((symbols.fixtures.len() + 1) as u32);
        symbols.fixtures.insert(source.id.clone(), id);
        fixtures.push(FixtureDef {
            id,
            name: source.name.clone(),
            color_model: source.color_model,
            pixel_count: source.pixel_count,
            pixel_type: source.pixel_type.unwrap_or(PixelType::Smart),
            bulb_shape: donder_core::model::BulbShape::LED,
            requires_layout: true,
            requires_patch: true,
            display_radius_override: None,
            channel_order: source.channel_order,
        });
    }

    for source in &display.groups {
        if symbols.groups.contains_key(&source.id) {
            diagnostics.push(Diagnostic::error(display_path, format!("duplicate group id '{}'", source.id)));
            continue;
        }
        let id = GroupId((symbols.groups.len() + 1) as u32);
        symbols.groups.insert(source.id.clone(), id);
    }
    for source in &display.groups {
        let Some(group_id) = symbols.groups.get(&source.id).copied() else {
            continue;
        };
        let members = source
            .members
            .iter()
            .filter_map(|member| {
                if let Some(id) = symbols.fixtures.get(member) {
                    Some(GroupMember::Fixture(*id))
                } else if let Some(id) = symbols.groups.get(member) {
                    Some(GroupMember::Group(*id))
                } else {
                    diagnostics.push(Diagnostic::error(display_path, format!("group '{}' references unknown member '{}'", source.id, member)));
                    None
                }
            })
            .collect();
        groups.push(FixtureGroup {
            id: group_id,
            name: source.name.clone(),
            members,
        });
    }

    for include in &display.controllers {
        let path = resolve_include(display_path, include, root, diagnostics);
        let doc: ControllerDoc = graph.load_jsonc(&path, "controller", diagnostics)?;
        validate_header(&doc.kind, doc.version, "controller", &path, diagnostics);
        for source in doc.controllers {
            if symbols.controllers.contains_key(&source.id) {
                diagnostics.push(Diagnostic::error(&path, format!("duplicate controller id '{}'", source.id)));
                continue;
            }
            let id = ControllerId((symbols.controllers.len() + 1) as u32);
            symbols.controllers.insert(source.id.clone(), id);
            controllers.push(Controller {
                id,
                name: source.name,
                protocol: ControllerProtocol::E131 {
                    unicast_address: source.address,
                    universes: Vec::new(),
                    universe_sizes: Vec::new(),
                },
            });
        }
    }

    for include in &display.layouts {
        let path = resolve_include(display_path, include, root, diagnostics);
        let doc: LayoutDoc = graph.load_jsonc(&path, "layout", diagnostics)?;
        validate_header(&doc.kind, doc.version, "layout", &path, diagnostics);
        for source in doc.fixtures {
            let Some(fixture_id) = symbols.fixtures.get(&source.fixture).copied() else {
                diagnostics.push(Diagnostic::error(&path, format!("layout references unknown fixture '{}'", source.fixture)));
                continue;
            };
            let shape = source.shape.map_or(LayoutShape::Custom, |shape| match shape {
                LayoutShapeSource::Line { start, end } => LayoutShape::Line { start, end },
                LayoutShapeSource::Grid {
                    top_left,
                    bottom_right,
                    columns,
                } => LayoutShape::Grid {
                    top_left,
                    bottom_right,
                    columns,
                },
            });
            let pixel_count = fixtures
                .iter()
                .find(|fixture| fixture.id == fixture_id)
                .map(|fixture| fixture.pixel_count as usize)
                .unwrap_or(0);
            let positions = if source.positions.is_empty() {
                shape.generate_positions(pixel_count).unwrap_or_default()
            } else {
                source.positions
            };
            layout_fixtures.push(donder_core::model::FixtureLayout {
                fixture_id,
                pixel_positions: positions,
                shape,
            });
        }
    }

    for include in &display.patches {
        let path = resolve_include(display_path, include, root, diagnostics);
        let doc: PatchDoc = graph.load_jsonc(&path, "patch", diagnostics)?;
        validate_header(&doc.kind, doc.version, "patch", &path, diagnostics);
        for source in doc.patches {
            let Some(fixture_id) = symbols.fixtures.get(&source.fixture).copied() else {
                diagnostics.push(Diagnostic::error(&path, format!("patch references unknown fixture '{}'", source.fixture)));
                continue;
            };
            let Some(controller_id) = symbols.controllers.get(&source.controller).copied() else {
                diagnostics.push(Diagnostic::error(&path, format!("patch references unknown controller '{}'", source.controller)));
                continue;
            };
            patches.push(Patch {
                fixture_id,
                fixture_channel_start: 0,
                channel_count: None,
                output: OutputMapping::PixelPort {
                    controller_id,
                    port: source.port,
                    channel_order: ChannelOrder::Rgb,
                },
            });
        }
    }

    Ok(())
}

fn compile_sequence(
    sequence_path: &Path,
    doc: &SequenceDoc,
    root: &Path,
    display_ids: &BTreeSet<String>,
    symbols: &SymbolTable,
    script_cache: &mut IndexMap<String, Arc<CompiledScript>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Sequence> {
    if !display_ids.contains(&doc.display) {
        diagnostics.push(Diagnostic::error(
            sequence_path,
            format!("sequence references unknown display '{}'", doc.display),
        ));
    }
    for script in &doc.scripts {
        let script_path = resolve_include(sequence_path, &script.path, root, diagnostics);
        match fs::read_to_string(&script_path) {
            Ok(source) => match compile_source(&source) {
                Ok(compiled) => {
                    script_cache.insert(script.id.clone(), Arc::new(compiled));
                }
                Err(errors) => diagnostics.push(Diagnostic::error(
                    &script_path,
                    format!("script '{}' failed to compile: {errors:?}", script.id),
                )),
            },
            Err(err) => diagnostics.push(Diagnostic::error(
                &script_path,
                format!("could not read script '{}': {err}", script.id),
            )),
        }
    }

    let mut node_timelines: HashMap<NodeId, NodeTimeline> = HashMap::new();
    for (index, event) in doc.events.iter().enumerate() {
        let Some(node_id) = resolve_target(&event.target, symbols) else {
            diagnostics.push(Diagnostic::error(sequence_path, format!("event {} references unknown target '{}'", index + 1, event.target)));
            continue;
        };
        let Some(kind) = resolve_effect_kind(&event.effect, script_cache) else {
            diagnostics.push(Diagnostic::error(sequence_path, format!("event {} references unknown effect '{}'", index + 1, event.effect)));
            continue;
        };
        if !event.start.is_finite() || event.start < 0.0 || !event.duration.is_finite() || event.duration <= 0.0 {
            diagnostics.push(Diagnostic::error(sequence_path, format!("event {} has invalid start/duration", index + 1)));
            continue;
        }
        let schemas = match &kind {
            EffectKind::BuiltIn(effect) => effects::effect_schema(effect),
            EffectKind::Script(name) => script_cache
                .get(name)
                .map(|script| donder_core::registry::types::extract_script_schemas(script))
                .unwrap_or_default(),
        };
        let params = convert_params(sequence_path, &event.params, &schemas, diagnostics);
        let item = TrackItem::Effect(EffectInstance {
            id: EffectId(format!("{}:{}", doc.id, index + 1)),
            kind,
            params,
            time_range: TimeRange::new(event.start, event.start + event.duration)
                .ok_or_else(|| ProjectError::Message("invalid event time range".to_string()))?,
            blend_mode: event.blend.unwrap_or(BlendMode::Override),
            opacity: event.opacity,
            param_links: HashMap::new(),
        });
        node_timelines.entry(node_id).or_default().add_item(item);
    }

    Sequence {
        name: doc.name.clone(),
        duration: doc.duration,
        frame_rate: doc.frame_rate,
        audio_file: doc.audio.clone(),
        node_timelines,
        motion_paths: HashMap::new(),
    }
    .validated()
    .map_err(ProjectError::Message)
}

fn resolve_effect_kind(
    effect: &str,
    script_cache: &IndexMap<String, Arc<CompiledScript>>,
) -> Option<EffectKind> {
    if let Ok(builtin) = effect.parse::<BuiltInEffect>() {
        return Some(EffectKind::BuiltIn(builtin));
    }
    script_cache
        .contains_key(effect)
        .then(|| EffectKind::Script(effect.to_string()))
}

fn convert_params(
    path: &Path,
    values: &serde_json::Map<String, serde_json::Value>,
    schemas: &[ParamSchema],
    diagnostics: &mut Vec<Diagnostic>,
) -> EffectParams {
    let mut params = EffectParams::new();
    for schema in schemas {
        let key_name = schema.key.to_string();
        let value = values.get(&key_name).or_else(|| values.get(&to_camel(&key_name)));
        let Some(value) = value else {
            params.set_mut(schema.key.clone(), schema.default.clone());
            continue;
        };
        match convert_param_value(value, &schema.param_type) {
            Some(converted) => params.set_mut(schema.key.clone(), converted),
            None => {
                diagnostics.push(Diagnostic::error(path, format!("invalid value for parameter '{key_name}'")));
                params.set_mut(schema.key.clone(), schema.default.clone());
            }
        }
    }
    params
}

fn to_camel(input: &str) -> String {
    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    first.to_lowercase().chain(chars).collect()
}

fn convert_param_value(value: &serde_json::Value, param_type: &ParamType) -> Option<ParamValue> {
    match param_type {
        ParamType::Float { .. } => value.as_f64().map(ParamValue::Float),
        ParamType::Int { .. } => value.as_i64().and_then(|v| i32::try_from(v).ok()).map(ParamValue::Int),
        ParamType::Bool => value.as_bool().map(ParamValue::Bool),
        ParamType::Color => parse_color(value).map(ParamValue::Color),
        ParamType::ColorList { .. } => value
            .as_array()
            .map(|items| items.iter().filter_map(parse_color).collect::<Vec<_>>())
            .map(ParamValue::ColorList),
        ParamType::ColorGradient { .. } => {
            if let Some(name) = value.as_str() {
                return Some(ParamValue::GradientRef(name.to_string()));
            }
            let stops = value.as_array()?.iter().filter_map(|item| {
                Some(donder_core::model::ColorStop {
                    position: item.get("position")?.as_f64()?,
                    color: parse_color(item.get("color")?)?,
                })
            }).collect::<Vec<_>>();
            ColorGradient::new(stops).map(ParamValue::ColorGradient)
        }
        ParamType::ColorMode { .. } => serde_json::from_value(value.clone()).ok().map(ParamValue::ColorMode),
        ParamType::WipeDirection { .. } => serde_json::from_value(value.clone()).ok().map(ParamValue::WipeDirection),
        ParamType::Text { .. } | ParamType::Enum { .. } => value.as_str().map(|s| ParamValue::Text(s.to_string())),
        ParamType::Flags { .. } => value.as_array().map(|items| {
            ParamValue::FlagSet(items.iter().filter_map(|item| item.as_str().map(ToOwned::to_owned)).collect())
        }),
        ParamType::Curve => Some(ParamValue::Curve(donder_core::model::Curve::linear())),
        ParamType::Path => value.as_str().map(|s| ParamValue::PathRef(s.to_string())),
    }
}

fn parse_color(value: &serde_json::Value) -> Option<Color> {
    if let Some(hex) = value.as_str() {
        let hex = hex.trim_start_matches('#');
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Color::rgb(r, g, b));
        }
    }
    let r = value.get("r")?.as_u64().and_then(|v| u8::try_from(v).ok())?;
    let g = value.get("g")?.as_u64().and_then(|v| u8::try_from(v).ok())?;
    let b = value.get("b")?.as_u64().and_then(|v| u8::try_from(v).ok())?;
    let a = value
        .get("a")
        .and_then(|v| v.as_u64())
        .and_then(|v| u8::try_from(v).ok())
        .unwrap_or(255);
    Some(Color::rgba(r, g, b, a))
}

fn resolve_target(target: &str, symbols: &SymbolTable) -> Option<NodeId> {
    symbols
        .fixtures
        .get(target)
        .copied()
        .map(NodeId::Fixture)
        .or_else(|| symbols.groups.get(target).copied().map(NodeId::Group))
}

fn validate_group_members(
    groups: &[FixtureGroup],
    fixtures: &[FixtureDef],
    diagnostics: &mut Vec<Diagnostic>,
    path: &Path,
) {
    let fixture_ids = fixtures.iter().map(|f| f.id).collect::<HashSet<_>>();
    let group_ids = groups.iter().map(|g| g.id).collect::<HashSet<_>>();
    for group in groups {
        for member in &group.members {
            match member {
                GroupMember::Fixture(id) if !fixture_ids.contains(id) => diagnostics.push(Diagnostic::error(path, "group has unresolved fixture member")),
                GroupMember::Group(id) if !group_ids.contains(id) => diagnostics.push(Diagnostic::error(path, "group has unresolved group member")),
                _ => {}
            }
        }
    }
}

fn validate_layouts(
    layouts: &[donder_core::model::FixtureLayout],
    fixtures: &[FixtureDef],
    diagnostics: &mut Vec<Diagnostic>,
    path: &Path,
) {
    for layout in layouts {
        if let Some(fixture) = fixtures.iter().find(|fixture| fixture.id == layout.fixture_id) {
            if layout.pixel_positions.len() != fixture.pixel_count as usize {
                diagnostics.push(Diagnostic::error(path, format!("layout for fixture {} has {} positions, expected {}", fixture.name, layout.pixel_positions.len(), fixture.pixel_count)));
            }
        }
    }
}

fn validate_patches(
    patches: &[Patch],
    fixtures: &[FixtureDef],
    controllers: &[Controller],
    diagnostics: &mut Vec<Diagnostic>,
    path: &Path,
) {
    let fixture_ids = fixtures.iter().map(|f| f.id).collect::<HashSet<_>>();
    let controller_ids = controllers.iter().map(|c| c.id).collect::<HashSet<_>>();
    for patch in patches {
        if !fixture_ids.contains(&patch.fixture_id) {
            diagnostics.push(Diagnostic::error(path, "patch has unresolved fixture"));
        }
        if let OutputMapping::PixelPort { controller_id, .. } = patch.output {
            if !controller_ids.contains(&controller_id) {
                diagnostics.push(Diagnostic::error(path, "patch has unresolved controller"));
            }
        }
    }
}

struct GraphLoader {
    root: PathBuf,
    visiting: HashSet<PathBuf>,
    edges: Vec<(PathBuf, PathBuf)>,
}

impl GraphLoader {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            visiting: HashSet::new(),
            edges: Vec::new(),
        }
    }

    fn load_jsonc<T: for<'de> Deserialize<'de>>(
        &mut self,
        path: &Path,
        expected_kind: &str,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<T> {
        let path = canonical_or_original(path);
        if !path.starts_with(&self.root) {
            diagnostics.push(Diagnostic::warning(&path, "external reference leaves the project folder"));
        }
        if !self.visiting.insert(path.clone()) {
            diagnostics.push(Diagnostic::error(&path, "include cycle detected"));
        }
        let raw = fs::read_to_string(&path).map_err(|err| {
            diagnostics.push(Diagnostic::error(&path, format!("could not read {expected_kind} document: {err}")));
            ProjectError::Validation {
                diagnostics: diagnostics.clone(),
            }
        })?;
        let stripped = strip_json_comments(&raw);
        let parsed = serde_json::from_str(&stripped).map_err(|err| {
            diagnostics.push(Diagnostic::error(&path, format!("invalid {expected_kind} JSONC: {err}")));
            ProjectError::Validation {
                diagnostics: diagnostics.clone(),
            }
        })?;
        self.visiting.remove(&path);
        Ok(parsed)
    }
}

fn validate_header(
    kind: &str,
    version: u32,
    expected: &str,
    path: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if kind != expected {
        diagnostics.push(Diagnostic::error(path, format!("expected kind '{expected}', found '{kind}'")));
    }
    if version != 1 {
        diagnostics.push(Diagnostic::error(path, format!("unsupported {expected} version {version}")));
    }
}

fn resolve_include(
    declaring_file: &Path,
    include: &str,
    root: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) -> PathBuf {
    let base = declaring_file.parent().unwrap_or(root);
    let path = canonical_or_original(&base.join(include));
    if !path.starts_with(root) {
        diagnostics.push(Diagnostic::warning(&path, "external reference leaves the project folder"));
    }
    path
}

fn detect_cycles(edges: &[(PathBuf, PathBuf)], diagnostics: &mut Vec<Diagnostic>) {
    let mut stack = Vec::<PathBuf>::new();
    let mut visiting = HashSet::<PathBuf>::new();
    let mut visited = HashSet::<PathBuf>::new();
    let adjacency = edges.iter().fold(HashMap::<&PathBuf, Vec<&PathBuf>>::new(), |mut acc, (from, to)| {
        acc.entry(from).or_default().push(to);
        acc
    });
    for node in adjacency.keys() {
        visit_cycle(node, &adjacency, &mut stack, &mut visiting, &mut visited, diagnostics);
    }
}

fn visit_cycle<'a>(
    node: &'a PathBuf,
    adjacency: &HashMap<&'a PathBuf, Vec<&'a PathBuf>>,
    stack: &mut Vec<PathBuf>,
    visiting: &mut HashSet<PathBuf>,
    visited: &mut HashSet<PathBuf>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if visited.contains(node) {
        return;
    }
    if !visiting.insert(node.clone()) {
        diagnostics.push(Diagnostic::error(node, format!("include cycle: {}", stack.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(" -> "))));
        return;
    }
    stack.push(node.clone());
    if let Some(next) = adjacency.get(node) {
        for child in next {
            visit_cycle(child, adjacency, stack, visiting, visited, diagnostics);
        }
    }
    stack.pop();
    visiting.remove(node);
    visited.insert(node.clone());
}

fn normalize_project_path(path: &Path) -> PathBuf {
    let candidate = if path.is_dir() {
        path.join("project.jsonc")
    } else {
        path.to_path_buf()
    };
    canonical_or_original(&candidate)
}

fn canonical_or_original(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn strip_json_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if in_string {
            escaped = ch == '\\' && !escaped;
            if ch == '"' && !escaped {
                in_string = false;
            }
            out.push(ch);
            if ch != '\\' {
                escaped = false;
            }
            continue;
        }
        if ch == '"' {
            in_string = true;
            out.push(ch);
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for next in chars.by_ref() {
                if next == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut prev = '\0';
            for next in chars.by_ref() {
                if next == '\n' {
                    out.push('\n');
                }
                if prev == '*' && next == '/' {
                    break;
                }
                prev = next;
            }
            continue;
        }
        out.push(ch);
    }
    out
}

pub fn create_starter_project(path: impl AsRef<Path>, name: &str) -> std::io::Result<()> {
    let root = path.as_ref();
    fs::create_dir_all(root.join("displays"))?;
    fs::create_dir_all(root.join("sequences"))?;
    fs::create_dir_all(root.join("effects"))?;
    fs::write(
        root.join("project.jsonc"),
        format!(
            r#"{{
  "kind": "project",
  "version": 1,
  "name": "{name}",
  "displays": ["displays/main.display.jsonc"],
  "sequences": ["sequences/demo.sequence.jsonc"]
}}
"#
        ),
    )?;
    fs::write(
        root.join("displays/main.display.jsonc"),
        r#"{
  "kind": "display",
  "version": 1,
  "id": "main",
  "name": "Main Display",
  "fixtures": [
    { "id": "roofline", "name": "Roofline", "pixelCount": 50 }
  ],
  "groups": [
    { "id": "all", "name": "All Lights", "members": ["roofline"] }
  ],
  "controllers": ["controllers.jsonc"],
  "layouts": ["layout.jsonc"],
  "patches": ["patch.jsonc"]
}
"#,
    )?;
    fs::write(
        root.join("displays/controllers.jsonc"),
        r#"{
  "kind": "controller",
  "version": 1,
  "controllers": [
    { "id": "falcon_main", "name": "Falcon Main", "address": "192.168.1.50" }
  ]
}
"#,
    )?;
    fs::write(
        root.join("displays/layout.jsonc"),
        r#"{
  "kind": "layout",
  "version": 1,
  "fixtures": [
    {
      "fixture": "roofline",
      "shape": {
        "type": "line",
        "start": { "x": 0.1, "y": 0.45 },
        "end": { "x": 0.9, "y": 0.45 }
      }
    }
  ]
}
"#,
    )?;
    fs::write(
        root.join("displays/patch.jsonc"),
        r#"{
  "kind": "patch",
  "version": 1,
  "patches": [
    { "fixture": "roofline", "controller": "falcon_main", "port": 1 }
  ]
}
"#,
    )?;
    fs::write(
        root.join("sequences/demo.sequence.jsonc"),
        r##"{
  "kind": "sequence",
  "version": 1,
  "id": "demo",
  "name": "demo",
  "display": "main",
  "duration": 10,
  "frameRate": 40,
  "events": [
    {
      "target": "roofline",
      "effect": "Solid",
      "start": 0,
      "duration": 10,
      "params": { "Color": "#40c4ff" }
    }
  ]
}
"##,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_project_checks_and_renders() {
        let dir = tempfile::tempdir().expect("temp dir");
        create_starter_project(dir.path(), "Test Show").expect("starter project");

        let compiled = check_project(dir.path()).expect("project should check");
        assert_eq!(compiled.show.fixtures.len(), 1);
        assert_eq!(compiled.show.sequences.len(), 1);

        let frame = render_frame(dir.path().join("sequences/demo.sequence.jsonc"), 1.0)
            .expect("frame should render");
        assert_eq!(frame.pixels.len(), 50 * 4);
        assert_eq!(frame.fixture_spans.len(), 1);
    }

    #[test]
    fn missing_event_target_is_validation_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        create_starter_project(dir.path(), "Broken Show").expect("starter project");
        let sequence = dir.path().join("sequences/demo.sequence.jsonc");
        let content = fs::read_to_string(&sequence)
            .expect("read sequence")
            .replace("\"target\": \"roofline\"", "\"target\": \"missing\"");
        fs::write(&sequence, content).expect("write sequence");

        let Err(ProjectError::Validation { diagnostics }) = check_project(dir.path()) else {
            panic!("expected validation error");
        };
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("unknown target")));
    }
}
