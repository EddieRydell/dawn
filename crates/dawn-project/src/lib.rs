use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dawn_core::dsl::{compile_source, compiler::CompiledScript};
use dawn_core::effects;
use dawn_core::engine::Frame;
use dawn_core::model::{
    BlendMode, BuiltInEffect, ChannelOrder, Color, ColorModel, Controller, ControllerId,
    ControllerProtocol, EffectId, EffectInstance, EffectKind, EffectParams, FixtureDef,
    FixtureGroup, FixtureId, GroupId, GroupMember, Layout, LayoutShape, NodeId, NodeTimeline,
    OutputMapping, ParamSchema, ParamType, ParamValue, Patch, PixelType, Position2D, Sequence,
    Show, TimeRange, TrackItem,
};
use dawn_lang::project as lang;
use dawn_lang::Value;
use indexmap::IndexMap;

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

pub fn check_project(project: impl AsRef<Path>) -> Result<CompiledProject> {
    let project_file = normalize_project_path(project.as_ref())?;
    let root = project_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut diagnostics = Vec::new();
    reject_legacy_suffix(&project_file, &mut diagnostics);

    let mut graph = GraphLoader::new(root.clone());
    let project_doc = match graph.load_dawn(&project_file, &mut diagnostics)? {
        lang::ProjectDocument::Project(doc) => doc,
        _ => {
            diagnostics.push(Diagnostic::error(
                &project_file,
                "expected project document",
            ));
            return Err(ProjectError::Validation { diagnostics });
        }
    };
    validate_version(
        project_doc.version,
        "project",
        &project_file,
        &mut diagnostics,
    );

    let mut displays = Vec::new();
    for include in &project_doc.displays {
        let path = resolve_include(&project_file, &include.path, &root, &mut diagnostics);
        let doc = match graph.load_dawn(&path, &mut diagnostics)? {
            lang::ProjectDocument::Display(doc) => doc,
            _ => {
                diagnostics.push(Diagnostic::error(&path, "expected display document"));
                continue;
            }
        };
        validate_version(doc.version, "display", &path, &mut diagnostics);
        displays.push((path, include.name.name.clone(), doc));
    }

    if displays.is_empty() {
        diagnostics.push(Diagnostic::error(
            &project_file,
            "project must include at least one display",
        ));
    }

    let mut symbol_table = SymbolTable::default();
    let mut fixtures = Vec::new();
    let mut groups = Vec::new();
    let mut controllers = Vec::new();
    let mut layout_fixtures = Vec::new();
    let mut patches = Vec::new();
    let mut display_ids = BTreeSet::new();

    for (display_path, display_symbol, display) in &displays {
        if display.name.name != *display_symbol {
            diagnostics.push(Diagnostic::error(
                display_path,
                format!(
                    "display include name '{}' does not match display declaration '{}'",
                    display_symbol, display.name.name
                ),
            ));
        }
        if !display_ids.insert(display.name.name.clone()) {
            diagnostics.push(Diagnostic::error(
                display_path,
                format!("duplicate display '{}'", display.name.name),
            ));
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
    validate_patches(
        &patches,
        &fixtures,
        &controllers,
        &mut diagnostics,
        &project_file,
    );

    let mut sequences = Vec::new();
    let mut script_cache = IndexMap::new();
    for include in &project_doc.sequences {
        let path = resolve_include(&project_file, &include.path, &root, &mut diagnostics);
        let doc = match graph.load_dawn(&path, &mut diagnostics)? {
            lang::ProjectDocument::Sequence(doc) => doc,
            _ => {
                diagnostics.push(Diagnostic::error(&path, "expected sequence document"));
                continue;
            }
        };
        validate_version(doc.version, "sequence", &path, &mut diagnostics);
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
        name: project_doc.name.name,
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
    if diagnostics
        .iter()
        .any(|d| d.severity == DiagnosticSeverity::Error)
    {
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
    reject_legacy_path(&sequence_file)?;
    let project_file = find_project_for_sequence(&sequence_file)?;
    let compiled = check_project(project_file)?;
    let sequence_index = compiled
        .show
        .sequences
        .iter()
        .position(|sequence| sequence.name == sequence_name_from_path(&sequence_file))
        .unwrap_or(0);
    Ok(dawn_core::evaluate(
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
        let candidate = ancestor.join("project.dawn");
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(ProjectError::Message(format!(
        "could not find project.dawn above {}",
        sequence_file.display()
    )))
}

fn sequence_name_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|stem| stem.split('.').next())
        .unwrap_or("sequence")
        .to_string()
}

#[allow(clippy::too_many_arguments)]
fn compile_display(
    display_path: &Path,
    display: &lang::DisplayDoc,
    root: &Path,
    graph: &mut GraphLoader,
    symbols: &mut SymbolTable,
    fixtures: &mut Vec<FixtureDef>,
    groups: &mut Vec<FixtureGroup>,
    controllers: &mut Vec<Controller>,
    layout_fixtures: &mut Vec<dawn_core::model::FixtureLayout>,
    patches: &mut Vec<Patch>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<()> {
    let consts = const_table(display_path, &display.consts, diagnostics);

    for source in &display.fixtures {
        if symbols.fixtures.contains_key(&source.name.name) {
            diagnostics.push(Diagnostic::error(
                display_path,
                format!("duplicate fixture '{}'", source.name.name),
            ));
            continue;
        }
        let Some(pixel_count) =
            const_expr_to_u32(display_path, &source.pixel_count, &consts, diagnostics)
        else {
            continue;
        };
        let id = FixtureId((symbols.fixtures.len() + 1) as u32);
        symbols.fixtures.insert(source.name.name.clone(), id);
        fixtures.push(FixtureDef {
            id,
            name: source.name.name.clone(),
            color_model: source
                .color_model
                .as_ref()
                .and_then(|id| parse_color_model(&id.name))
                .unwrap_or(ColorModel::Rgb),
            pixel_count,
            pixel_type: PixelType::Smart,
            bulb_shape: dawn_core::model::BulbShape::LED,
            requires_layout: true,
            requires_patch: true,
            display_radius_override: None,
            channel_order: source
                .channel_order
                .as_ref()
                .and_then(|id| parse_channel_order(&id.name))
                .unwrap_or(ChannelOrder::Rgb),
        });
    }

    for source in &display.groups {
        if symbols.groups.contains_key(&source.name.name) {
            diagnostics.push(Diagnostic::error(
                display_path,
                format!("duplicate group '{}'", source.name.name),
            ));
            continue;
        }
        let id = GroupId((symbols.groups.len() + 1) as u32);
        symbols.groups.insert(source.name.name.clone(), id);
    }
    for source in &display.groups {
        let Some(group_id) = symbols.groups.get(&source.name.name).copied() else {
            continue;
        };
        let members = source
            .members
            .iter()
            .filter_map(|member| {
                if let Some(id) = symbols.fixtures.get(&member.name) {
                    Some(GroupMember::Fixture(*id))
                } else if let Some(id) = symbols.groups.get(&member.name) {
                    Some(GroupMember::Group(*id))
                } else {
                    diagnostics.push(Diagnostic::error(
                        display_path,
                        format!(
                            "group '{}' references unknown member '{}'",
                            source.name.name, member.name
                        ),
                    ));
                    None
                }
            })
            .collect();
        groups.push(FixtureGroup {
            id: group_id,
            name: source.name.name.clone(),
            members,
        });
    }

    for include in &display.includes {
        let path = resolve_include(display_path, &include.path, root, diagnostics);
        match include.section.name.as_str() {
            "controllers" => {
                let doc = match graph.load_dawn(&path, diagnostics)? {
                    lang::ProjectDocument::Controllers(doc) => doc,
                    _ => {
                        diagnostics.push(Diagnostic::error(&path, "expected controllers document"));
                        continue;
                    }
                };
                validate_version(doc.version, "controllers", &path, diagnostics);
                for source in doc.controllers {
                    if symbols.controllers.contains_key(&source.name.name) {
                        diagnostics.push(Diagnostic::error(
                            &path,
                            format!("duplicate controller '{}'", source.name.name),
                        ));
                        continue;
                    }
                    let id = ControllerId((symbols.controllers.len() + 1) as u32);
                    symbols.controllers.insert(source.name.name.clone(), id);
                    controllers.push(Controller {
                        id,
                        name: source.name.name,
                        protocol: ControllerProtocol::E131 {
                            unicast_address: source.address,
                            universes: Vec::new(),
                            universe_sizes: Vec::new(),
                        },
                    });
                }
            }
            "layout" => {
                let doc = match graph.load_dawn(&path, diagnostics)? {
                    lang::ProjectDocument::Layout(doc) => doc,
                    _ => {
                        diagnostics.push(Diagnostic::error(&path, "expected layout document"));
                        continue;
                    }
                };
                validate_version(doc.version, "layout", &path, diagnostics);
                for source in doc.fixtures {
                    let Some(fixture_id) = symbols.fixtures.get(&source.fixture.name).copied()
                    else {
                        diagnostics.push(Diagnostic::error(
                            &path,
                            format!(
                                "layout references unknown fixture '{}'",
                                source.fixture.name
                            ),
                        ));
                        continue;
                    };
                    let shape = source
                        .shape
                        .map_or(LayoutShape::Custom, |shape| match shape {
                            lang::LayoutShapeSource::Line { start, end } => LayoutShape::Line {
                                start: to_position(start),
                                end: to_position(end),
                            },
                            lang::LayoutShapeSource::Grid {
                                top_left,
                                bottom_right,
                                columns,
                            } => LayoutShape::Grid {
                                top_left: to_position(top_left),
                                bottom_right: to_position(bottom_right),
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
                        source.positions.into_iter().map(to_position).collect()
                    };
                    layout_fixtures.push(dawn_core::model::FixtureLayout {
                        fixture_id,
                        pixel_positions: positions,
                        shape,
                    });
                }
            }
            "patch" => {
                let doc = match graph.load_dawn(&path, diagnostics)? {
                    lang::ProjectDocument::Patch(doc) => doc,
                    _ => {
                        diagnostics.push(Diagnostic::error(&path, "expected patch document"));
                        continue;
                    }
                };
                validate_version(doc.version, "patch", &path, diagnostics);
                for source in doc.patches {
                    let Some(fixture_id) = symbols.fixtures.get(&source.fixture.name).copied()
                    else {
                        diagnostics.push(Diagnostic::error(
                            &path,
                            format!("patch references unknown fixture '{}'", source.fixture.name),
                        ));
                        continue;
                    };
                    let Some(controller_id) =
                        symbols.controllers.get(&source.controller.name).copied()
                    else {
                        diagnostics.push(Diagnostic::error(
                            &path,
                            format!(
                                "patch references unknown controller '{}'",
                                source.controller.name
                            ),
                        ));
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
            other => diagnostics.push(Diagnostic::error(
                display_path,
                format!("unknown include section '{other}'"),
            )),
        }
    }

    Ok(())
}

fn compile_sequence(
    sequence_path: &Path,
    doc: &lang::SequenceDoc,
    root: &Path,
    display_ids: &BTreeSet<String>,
    symbols: &SymbolTable,
    script_cache: &mut IndexMap<String, Arc<CompiledScript>>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Sequence> {
    if !display_ids.contains(&doc.display.name) {
        diagnostics.push(Diagnostic::error(
            sequence_path,
            format!("sequence references unknown display '{}'", doc.display.name),
        ));
    }
    for script in &doc.scripts {
        let script_path = resolve_include(sequence_path, &script.path, root, diagnostics);
        match fs::read_to_string(&script_path) {
            Ok(source) => match compile_source(&source) {
                Ok(compiled) => {
                    script_cache.insert(script.name.name.clone(), Arc::new(compiled));
                }
                Err(errors) => diagnostics.push(Diagnostic::error(
                    &script_path,
                    format!(
                        "script '{}' failed to compile: {errors:?}",
                        script.name.name
                    ),
                )),
            },
            Err(err) => diagnostics.push(Diagnostic::error(
                &script_path,
                format!("could not read script '{}': {err}", script.name.name),
            )),
        }
    }

    let mut node_timelines: HashMap<NodeId, NodeTimeline> = HashMap::new();
    for (index, event) in doc.events.iter().enumerate() {
        let Some(node_id) = resolve_target(&event.target.name, symbols) else {
            diagnostics.push(Diagnostic::error(
                sequence_path,
                format!(
                    "event {} references unknown target '{}'",
                    index + 1,
                    event.target.name
                ),
            ));
            continue;
        };
        let Some(kind) = resolve_effect_kind(&event.effect.name, script_cache) else {
            diagnostics.push(Diagnostic::error(
                sequence_path,
                format!(
                    "event {} references unknown effect '{}'",
                    index + 1,
                    event.effect.name
                ),
            ));
            continue;
        };
        if !event.start.is_finite()
            || event.start < 0.0
            || !event.duration.is_finite()
            || event.duration <= 0.0
        {
            diagnostics.push(Diagnostic::error(
                sequence_path,
                format!("event {} has invalid start/duration", index + 1),
            ));
            continue;
        }
        let schemas = match &kind {
            EffectKind::BuiltIn(effect) => effects::effect_schema(effect),
            EffectKind::Script(name) => script_cache
                .get(name)
                .map(|script| dawn_core::registry::types::extract_script_schemas(script))
                .unwrap_or_default(),
        };
        let params = convert_params(sequence_path, &event.params, &schemas, diagnostics);
        let item = TrackItem::Effect(EffectInstance {
            id: EffectId(format!("{}:{}", doc.name.name, index + 1)),
            kind,
            params,
            time_range: TimeRange::new(event.start, event.start + event.duration)
                .ok_or_else(|| ProjectError::Message("invalid event time range".to_string()))?,
            blend_mode: BlendMode::Override,
            opacity: 1.0,
            param_links: HashMap::new(),
        });
        node_timelines.entry(node_id).or_default().add_item(item);
    }

    Sequence {
        name: doc.name.name.clone(),
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
    values: &[lang::ParamAssignment],
    schemas: &[ParamSchema],
    diagnostics: &mut Vec<Diagnostic>,
) -> EffectParams {
    let mut params = EffectParams::new();
    let mut seen = HashSet::new();
    let by_name = schemas
        .iter()
        .map(|schema| (schema.key.to_string(), schema))
        .collect::<HashMap<_, _>>();

    for assignment in values {
        if !seen.insert(assignment.name.name.clone()) {
            diagnostics.push(Diagnostic::error(
                path,
                format!("duplicate parameter '{}'", assignment.name.name),
            ));
            continue;
        }
        let Some(schema) = by_name.get(&assignment.name.name) else {
            diagnostics.push(Diagnostic::error(
                path,
                format!("unknown parameter '{}'", assignment.name.name),
            ));
            continue;
        };
        match const_expr_to_param_value(&assignment.value, &schema.param_type) {
            Some(converted) => params.set_mut(schema.key.clone(), converted),
            None => diagnostics.push(Diagnostic::error(
                path,
                format!("invalid value for parameter '{}'", assignment.name.name),
            )),
        }
    }

    for schema in schemas {
        if !params.inner().contains_key(&schema.key) {
            params.set_mut(schema.key.clone(), schema.default.clone());
        }
    }
    params
}

fn const_expr_to_param_value(expr: &lang::ConstExpr, param_type: &ParamType) -> Option<ParamValue> {
    let value = eval_project_const(expr, &HashMap::new()).ok()?;
    match param_type {
        ParamType::Float { .. } => match value {
            Value::Float(value) => Some(ParamValue::Float(value)),
            Value::Int(value) => Some(ParamValue::Float(value as f64)),
            _ => None,
        },
        ParamType::Int { .. } => match value {
            Value::Int(value) => i32::try_from(value).ok().map(ParamValue::Int),
            _ => None,
        },
        ParamType::Bool => match value {
            Value::Bool(value) => Some(ParamValue::Bool(value)),
            _ => None,
        },
        ParamType::Color => match value {
            Value::Color(r, g, b) => Some(ParamValue::Color(Color::rgb(r, g, b))),
            _ => None,
        },
        ParamType::ColorList { .. } => match value {
            Value::Array(items) => items
                .into_iter()
                .map(|item| match item {
                    Value::Color(r, g, b) => Some(Color::rgb(r, g, b)),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
                .map(ParamValue::ColorList),
            _ => None,
        },
        ParamType::ColorGradient { .. } => None,
        ParamType::ColorMode { .. } => match value {
            Value::Ref(name) => parse_color_mode(&name).map(ParamValue::ColorMode),
            _ => None,
        },
        ParamType::WipeDirection { .. } => match value {
            Value::Ref(name) => parse_wipe_direction(&name).map(ParamValue::WipeDirection),
            _ => None,
        },
        ParamType::Text { .. } => match value {
            Value::String(value) => Some(ParamValue::Text(value)),
            Value::Ref(value) => Some(ParamValue::Text(value)),
            _ => None,
        },
        ParamType::Enum { options } => match value {
            Value::Ref(name) if options.contains(&name) => Some(ParamValue::EnumVariant(name)),
            _ => None,
        },
        ParamType::Flags { options } => match value {
            Value::Array(items) => items
                .into_iter()
                .map(|item| match item {
                    Value::Ref(name) if options.contains(&name) => Some(name),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
                .map(ParamValue::FlagSet),
            _ => None,
        },
        ParamType::Curve | ParamType::Path => None,
    }
}

fn const_table(
    path: &Path,
    consts: &[lang::ConstDecl],
    diagnostics: &mut Vec<Diagnostic>,
) -> HashMap<String, Value> {
    let mut table = HashMap::new();
    for decl in consts {
        if table.contains_key(&decl.name.name) {
            diagnostics.push(Diagnostic::error(
                path,
                format!("duplicate const '{}'", decl.name.name),
            ));
            continue;
        }
        match eval_project_const(&decl.value, &table) {
            Ok(value) => {
                table.insert(decl.name.name.clone(), value);
            }
            Err(message) => diagnostics.push(Diagnostic::error(
                path,
                format!("invalid const '{}': {message}", decl.name.name),
            )),
        }
    }
    table
}

fn const_expr_to_u32(
    path: &Path,
    expr: &lang::ConstExpr,
    consts: &HashMap<String, Value>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<u32> {
    match eval_project_const(expr, consts) {
        Ok(Value::Int(value)) => u32::try_from(value).ok(),
        Ok(_) => {
            diagnostics.push(Diagnostic::error(path, "expected integer const expression"));
            None
        }
        Err(message) => {
            diagnostics.push(Diagnostic::error(path, message));
            None
        }
    }
}

fn eval_project_const(
    expr: &lang::ConstExpr,
    consts: &HashMap<String, Value>,
) -> std::result::Result<Value, String> {
    match &expr.kind {
        lang::ConstExprKind::Value(value) => Ok(value.clone()),
        lang::ConstExprKind::Ref(name) => Ok(consts
            .get(name)
            .cloned()
            .unwrap_or_else(|| Value::Ref(name.clone()))),
        lang::ConstExprKind::Unary {
            op: lang::ConstUnaryOp::Neg,
            expr,
        } => match eval_project_const(expr, consts)? {
            Value::Int(value) => Ok(Value::Int(-value)),
            Value::Float(value) => Ok(Value::Float(-value)),
            _ => Err("unary '-' requires a number".to_string()),
        },
        lang::ConstExprKind::Binary { op, left, right } => {
            let left = eval_project_const(left, consts)?;
            let right = eval_project_const(right, consts)?;
            eval_const_binary(*op, left, right)
                .ok_or_else(|| "binary const operator requires numbers".to_string())
        }
    }
}

fn eval_const_binary(op: lang::ConstBinaryOp, left: Value, right: Value) -> Option<Value> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => match op {
            lang::ConstBinaryOp::Add => Some(Value::Int(left + right)),
            lang::ConstBinaryOp::Sub => Some(Value::Int(left - right)),
            lang::ConstBinaryOp::Mul => Some(Value::Int(left * right)),
            lang::ConstBinaryOp::Div => Some(Value::Float(left as f64 / right as f64)),
        },
        (left, right) => {
            let left = number_value(left)?;
            let right = number_value(right)?;
            match op {
                lang::ConstBinaryOp::Add => Some(Value::Float(left + right)),
                lang::ConstBinaryOp::Sub => Some(Value::Float(left - right)),
                lang::ConstBinaryOp::Mul => Some(Value::Float(left * right)),
                lang::ConstBinaryOp::Div => Some(Value::Float(left / right)),
            }
        }
    }
}

fn number_value(value: Value) -> Option<f64> {
    match value {
        Value::Int(value) => Some(value as f64),
        Value::Float(value) => Some(value),
        _ => None,
    }
}

fn to_position(source: lang::PositionSource) -> Position2D {
    Position2D {
        x: source.x as f32,
        y: source.y as f32,
    }
}

fn parse_color_model(name: &str) -> Option<ColorModel> {
    match name {
        "Single" => Some(ColorModel::Single),
        "Rgb" => Some(ColorModel::Rgb),
        "Rgbw" => Some(ColorModel::Rgbw),
        _ => None,
    }
}

fn parse_channel_order(name: &str) -> Option<ChannelOrder> {
    match name {
        "Rgb" => Some(ChannelOrder::Rgb),
        "Grb" => Some(ChannelOrder::Grb),
        "Brg" => Some(ChannelOrder::Brg),
        "Rbg" => Some(ChannelOrder::Rbg),
        "Gbr" => Some(ChannelOrder::Gbr),
        "Bgr" => Some(ChannelOrder::Bgr),
        _ => None,
    }
}

fn parse_color_mode(name: &str) -> Option<dawn_core::model::ColorMode> {
    match name {
        "Static" => Some(dawn_core::model::ColorMode::Static),
        "GradientPerPulse" => Some(dawn_core::model::ColorMode::GradientPerPulse),
        "GradientThroughEffect" => Some(dawn_core::model::ColorMode::GradientThroughEffect),
        "GradientAcrossItems" => Some(dawn_core::model::ColorMode::GradientAcrossItems),
        _ => None,
    }
}

fn parse_wipe_direction(name: &str) -> Option<dawn_core::model::WipeDirection> {
    match name {
        "Horizontal" => Some(dawn_core::model::WipeDirection::Horizontal),
        "Vertical" => Some(dawn_core::model::WipeDirection::Vertical),
        "DiagonalUp" => Some(dawn_core::model::WipeDirection::DiagonalUp),
        "DiagonalDown" => Some(dawn_core::model::WipeDirection::DiagonalDown),
        "Burst" => Some(dawn_core::model::WipeDirection::Burst),
        "Circle" => Some(dawn_core::model::WipeDirection::Circle),
        "Diamond" => Some(dawn_core::model::WipeDirection::Diamond),
        _ => None,
    }
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
                GroupMember::Fixture(id) if !fixture_ids.contains(id) => diagnostics.push(
                    Diagnostic::error(path, "group has unresolved fixture member"),
                ),
                GroupMember::Group(id) if !group_ids.contains(id) => {
                    diagnostics.push(Diagnostic::error(path, "group has unresolved group member"))
                }
                _ => {}
            }
        }
    }
}

fn validate_layouts(
    layouts: &[dawn_core::model::FixtureLayout],
    fixtures: &[FixtureDef],
    diagnostics: &mut Vec<Diagnostic>,
    path: &Path,
) {
    for layout in layouts {
        if let Some(fixture) = fixtures
            .iter()
            .find(|fixture| fixture.id == layout.fixture_id)
        {
            if layout.pixel_positions.len() != fixture.pixel_count as usize {
                diagnostics.push(Diagnostic::error(
                    path,
                    format!(
                        "layout for fixture {} has {} positions, expected {}",
                        fixture.name,
                        layout.pixel_positions.len(),
                        fixture.pixel_count
                    ),
                ));
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

    fn load_dawn(
        &mut self,
        path: &Path,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Result<lang::ProjectDocument> {
        let path = canonical_or_original(path);
        reject_legacy_suffix(&path, diagnostics);
        if !path.starts_with(&self.root) {
            diagnostics.push(Diagnostic::warning(
                &path,
                "external reference leaves the project folder",
            ));
        }
        if !self.visiting.insert(path.clone()) {
            diagnostics.push(Diagnostic::error(&path, "include cycle detected"));
        }
        let raw = fs::read_to_string(&path).map_err(|err| {
            diagnostics.push(Diagnostic::error(
                &path,
                format!("could not read Dawn document: {err}"),
            ));
            ProjectError::Validation {
                diagnostics: diagnostics.clone(),
            }
        })?;
        let parsed = lang::parse_document(&path, &raw).map_err(|errors| {
            diagnostics.extend(
                errors
                    .into_iter()
                    .map(|diagnostic| Diagnostic::error(diagnostic.path, diagnostic.message)),
            );
            ProjectError::Validation {
                diagnostics: diagnostics.clone(),
            }
        })?;
        self.visiting.remove(&path);
        Ok(parsed)
    }
}

fn validate_version(version: u32, expected: &str, path: &Path, diagnostics: &mut Vec<Diagnostic>) {
    if version != 1 {
        diagnostics.push(Diagnostic::error(
            path,
            format!("unsupported {expected} version {version}"),
        ));
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
    reject_legacy_suffix(&path, diagnostics);
    if !path.starts_with(root) {
        diagnostics.push(Diagnostic::warning(
            &path,
            "external reference leaves the project folder",
        ));
    }
    path
}

fn detect_cycles(edges: &[(PathBuf, PathBuf)], diagnostics: &mut Vec<Diagnostic>) {
    let mut stack = Vec::<PathBuf>::new();
    let mut visiting = HashSet::<PathBuf>::new();
    let mut visited = HashSet::<PathBuf>::new();
    let adjacency = edges.iter().fold(
        HashMap::<&PathBuf, Vec<&PathBuf>>::new(),
        |mut acc, (from, to)| {
            acc.entry(from).or_default().push(to);
            acc
        },
    );
    for node in adjacency.keys() {
        visit_cycle(
            node,
            &adjacency,
            &mut stack,
            &mut visiting,
            &mut visited,
            diagnostics,
        );
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
        diagnostics.push(Diagnostic::error(
            node,
            format!(
                "include cycle: {}",
                stack
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" -> ")
            ),
        ));
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

fn normalize_project_path(path: &Path) -> Result<PathBuf> {
    reject_legacy_path(path)?;
    let candidate = if path.is_dir() {
        path.join("project.dawn")
    } else {
        path.to_path_buf()
    };
    Ok(canonical_or_original(&candidate))
}

fn canonical_or_original(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn reject_legacy_path(path: &Path) -> Result<()> {
    if is_legacy_suffix(path) {
        return Err(ProjectError::Message(
            "legacy .jsonc and .vibe files are no longer accepted; use .dawn".to_string(),
        ));
    }
    Ok(())
}

fn reject_legacy_suffix(path: &Path, diagnostics: &mut Vec<Diagnostic>) {
    if is_legacy_suffix(path) {
        diagnostics.push(Diagnostic::error(
            path,
            "legacy .jsonc and .vibe files are no longer accepted; use .dawn",
        ));
    }
}

fn is_legacy_suffix(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "jsonc" | "vibe"))
}

pub fn create_starter_project(path: impl AsRef<Path>, name: &str) -> std::io::Result<()> {
    let root = path.as_ref();
    let project_name = to_identifier(name);
    fs::create_dir_all(root.join("displays"))?;
    fs::create_dir_all(root.join("sequences"))?;
    fs::create_dir_all(root.join("effects"))?;
    fs::write(
        root.join("project.dawn"),
        format!(
            r#"project {project_name} {{
  version 1;
  display Main from "displays/Main.display.dawn";
  sequence Demo from "sequences/Demo.sequence.dawn";
}}
"#
        ),
    )?;
    fs::write(
        root.join("displays/Main.display.dawn"),
        r#"display Main {
  version 1;
  const RoofPixels: Int = 50;

  fixture Roofline {
    pixel_count RoofPixels;
    color_model Rgb;
    channel_order Rgb;
  }

  group All {
    members [Roofline];
  }

  include controllers from "controllers.dawn";
  include layout from "layout.dawn";
  include patch from "patch.dawn";
}
"#,
    )?;
    fs::write(
        root.join("displays/controllers.dawn"),
        r#"controllers {
  version 1;

  controller FalconMain {
    address "192.168.1.50";
  }
}
"#,
    )?;
    fs::write(
        root.join("displays/layout.dawn"),
        r#"layout {
  version 1;

  fixture Roofline {
    shape line {
      start { x 0.1; y 0.45; };
      end { x 0.9; y 0.45; };
    }
  }
}
"#,
    )?;
    fs::write(
        root.join("displays/patch.dawn"),
        r#"patch {
  version 1;

  fixture Roofline {
    controller FalconMain;
    port 1;
  }
}
"#,
    )?;
    fs::write(
        root.join("sequences/Demo.sequence.dawn"),
        r##"sequence Demo {
  version 1;
  display Main;
  duration 10s;
  frame_rate 40;

  event All {
    effect Solid;
    at 0s for 10s;
    params {
      Color #40c4ff;
    }
  }
}
"##,
    )?;
    Ok(())
}

fn to_identifier(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "Show".to_string()
    } else if out.as_bytes()[0].is_ascii_digit() {
        format!("Show_{out}")
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_project_checks_and_renders() {
        let dir = tempfile::tempdir().expect("temp dir");
        create_starter_project(dir.path(), "TestShow").expect("starter project");

        let compiled = check_project(dir.path()).expect("project should check");
        assert_eq!(compiled.show.fixtures.len(), 1);
        assert_eq!(compiled.show.sequences.len(), 1);

        let frame = render_frame(dir.path().join("sequences/Demo.sequence.dawn"), 1.0)
            .expect("frame should render");
        assert_eq!(frame.pixels.len(), 50 * 4);
        assert_eq!(frame.fixture_spans.len(), 1);
    }

    #[test]
    fn missing_event_target_is_validation_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        create_starter_project(dir.path(), "BrokenShow").expect("starter project");
        let sequence = dir.path().join("sequences/Demo.sequence.dawn");
        let content = fs::read_to_string(&sequence)
            .expect("read sequence")
            .replace("event All", "event Missing");
        fs::write(&sequence, content).expect("write sequence");

        let Err(ProjectError::Validation { diagnostics }) = check_project(dir.path()) else {
            panic!("expected validation error");
        };
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("unknown target")));
    }

    #[test]
    fn legacy_jsonc_is_rejected() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join("project.jsonc"), "{}").expect("write jsonc");
        let err =
            check_project(dir.path().join("project.jsonc")).expect_err("legacy path should fail");
        assert!(err.to_string().contains("no longer accepted"));
    }

    #[test]
    fn unknown_param_is_validation_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        create_starter_project(dir.path(), "BrokenShow").expect("starter project");
        let sequence = dir.path().join("sequences/Demo.sequence.dawn");
        let content = fs::read_to_string(&sequence)
            .expect("read sequence")
            .replace("Color #40c4ff;", "NotAParam 1;");
        fs::write(&sequence, content).expect("write sequence");

        let Err(ProjectError::Validation { diagnostics }) = check_project(dir.path()) else {
            panic!("expected validation error");
        };
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("unknown parameter")));
    }
}
