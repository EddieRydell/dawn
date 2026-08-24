use std::collections::{BTreeMap, HashSet};
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::dsl::Identifier;
use indexmap::IndexMap;
use yaml_serde::Value;

use crate::diagnostics::{
    load_error_diagnostic, parse_yaml_value, push_diagnostic, source_range_for_field_value,
    source_range_for_value, with_yaml_location,
};
use crate::loader::parse::{
    bool_field, f64_field, mapping, optional_sequence, parse_color, parse_duration,
    parse_duration_as_time, parse_imports, required_field, sequence_values, string_field,
    u32_field,
};
use crate::{
    IoDiagnostic, IoDiagnosticCode, IoDiagnosticSeverity, LoadProjectError, SourceObjectKind,
    TextPosition, TextRange,
};

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectRecovery {
    pub root: Utf8PathBuf,
    pub manifest: Option<dawn_package::PackageManifest>,
    pub documents: IndexMap<Utf8PathBuf, RecoveryDocument>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryDocument {
    pub kind: RecoveryDocumentKind,
    pub objects: Vec<RecoveryObject>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecoveryDocumentKind {
    Dawn,
    Effect,
    Operator,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryObject {
    pub key: String,
    pub kind: SourceObjectKind,
    pub sequence: Option<RecoverySequence>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecoverySequence {
    pub duration_seconds: f64,
    pub frame_rate: f64,
    pub layers: Vec<RecoverySequenceLayer>,
    pub mark_collections: Vec<RecoveryMarkCollection>,
    pub items: Vec<RecoverySequenceItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecoverySequenceLayer {
    pub id: u32,
    pub name: String,
    pub color: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryMarkCollection {
    pub key: String,
    pub name: String,
    pub color: String,
    pub marks_seconds: Vec<f64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecoverySequenceItemKind {
    Effect,
    AutomationClip,
    ControlClip,
    GraphNode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecoverySequenceItem {
    pub kind: RecoverySequenceItemKind,
    pub id: String,
    pub placement: RecoverySequencePlacement,
    pub valid: bool,
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RecoverySequencePlacement {
    Timeline {
        start_seconds: f64,
        duration_seconds: f64,
        lane: RecoveryTimelineLane,
    },
    Graph {
        x: f64,
        y: f64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecoveryTimelineLane {
    Layer(u32),
    Lane(u32),
}

pub(crate) fn analyze_project_documents(
    root: &Utf8Path,
    manifest: Option<dawn_package::PackageManifest>,
    diagnostics: &mut Vec<IoDiagnostic>,
) -> ProjectRecovery {
    let mut documents = IndexMap::new();
    for path in project_file_inventory(root) {
        let absolute = root.join(&path);
        let kind = document_kind(&path);
        let document = match kind {
            RecoveryDocumentKind::Effect => {
                analyze_dsl_document(&path, &absolute, true, diagnostics)
            }
            RecoveryDocumentKind::Operator => {
                analyze_dsl_document(&path, &absolute, false, diagnostics)
            }
            RecoveryDocumentKind::Dawn => analyze_dawn_document(&path, &absolute, diagnostics),
            RecoveryDocumentKind::Other => RecoveryDocument {
                kind,
                objects: Vec::new(),
            },
        };
        documents.insert(path, document);
    }
    ProjectRecovery {
        root: root.to_path_buf(),
        manifest,
        documents,
    }
}

fn project_file_inventory(root: &Utf8Path) -> Vec<Utf8PathBuf> {
    let mut pending = vec![Utf8PathBuf::new()];
    let mut files = Vec::new();
    while let Some(relative) = pending.pop() {
        let Ok(entries) = fs::read_dir(root.join(&relative)) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if name == ".git" || name == "target" || name == "node_modules" {
                continue;
            }
            let path = relative.join(name);
            if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
                pending.push(path);
            } else {
                files.push(Utf8PathBuf::from(path.as_str().replace('\\', "/")));
            }
        }
    }
    files.sort();
    files
}

fn document_kind(path: &Utf8Path) -> RecoveryDocumentKind {
    let file_name = path.file_name().unwrap_or_default();
    if file_name.ends_with(".effect.dawn") {
        RecoveryDocumentKind::Effect
    } else if file_name.ends_with(".operator.dawn") {
        RecoveryDocumentKind::Operator
    } else if file_name.ends_with(".dawn") {
        RecoveryDocumentKind::Dawn
    } else {
        RecoveryDocumentKind::Other
    }
}

fn analyze_dsl_document(
    path: &Utf8Path,
    absolute: &Utf8Path,
    effect: bool,
    diagnostics: &mut Vec<IoDiagnostic>,
) -> RecoveryDocument {
    let text = match fs::read_to_string(absolute) {
        Ok(text) => text,
        Err(error) => {
            push_diagnostic(
                diagnostics,
                IoDiagnostic {
                    path: path.to_path_buf(),
                    range: None,
                    severity: IoDiagnosticSeverity::Error,
                    code: IoDiagnosticCode::IoRead,
                    message: error.to_string(),
                    detail: None,
                    related: Vec::new(),
                },
            );
            String::new()
        }
    };
    let local = if effect {
        crate::diagnostics::effect_diagnostics(path, &text)
    } else {
        crate::diagnostics::operator_diagnostics(path, &text)
    };
    for diagnostic in local {
        push_diagnostic(diagnostics, diagnostic);
    }
    RecoveryDocument {
        kind: if effect {
            RecoveryDocumentKind::Effect
        } else {
            RecoveryDocumentKind::Operator
        },
        objects: Vec::new(),
    }
}

fn analyze_dawn_document(
    path: &Utf8Path,
    absolute: &Utf8Path,
    diagnostics: &mut Vec<IoDiagnostic>,
) -> RecoveryDocument {
    let text = match fs::read_to_string(absolute) {
        Ok(text) => text,
        Err(error) => {
            push_diagnostic(
                diagnostics,
                IoDiagnostic {
                    path: path.to_path_buf(),
                    range: None,
                    severity: IoDiagnosticSeverity::Error,
                    code: IoDiagnosticCode::IoRead,
                    message: error.to_string(),
                    detail: None,
                    related: Vec::new(),
                },
            );
            return RecoveryDocument {
                kind: RecoveryDocumentKind::Dawn,
                objects: Vec::new(),
            };
        }
    };
    analyze_dawn_text(path, &text, diagnostics)
}

fn analyze_dawn_text(
    path: &Utf8Path,
    text: &str,
    diagnostics: &mut Vec<IoDiagnostic>,
) -> RecoveryDocument {
    let value = match parse_yaml_value(path, text) {
        Ok(value) => value,
        Err(error) => {
            push_diagnostic(diagnostics, load_error_diagnostic(error));
            return RecoveryDocument {
                kind: RecoveryDocumentKind::Dawn,
                objects: Vec::new(),
            };
        }
    };
    let Some(root) = mapping(&value) else {
        push_schema_error(
            diagnostics,
            path,
            source_range_for_value(path, &value),
            "document root must be a mapping",
            IoDiagnosticCode::DawnLoad,
        );
        return RecoveryDocument {
            kind: RecoveryDocumentKind::Dawn,
            objects: Vec::new(),
        };
    };
    analyze_imports(path, root, diagnostics);

    let mut objects = Vec::new();
    for (key, object_value) in root {
        let Some(key) = key.as_str() else {
            push_schema_error(
                diagnostics,
                path,
                source_range_for_value(path, object_value),
                "object keys must be strings",
                IoDiagnosticCode::DawnLoad,
            );
            continue;
        };
        if key == "imports" {
            continue;
        }
        if Identifier::new(key.to_string()).is_err() {
            push_schema_error(
                diagnostics,
                path,
                source_range_for_value(path, object_value),
                format!("invalid object identifier `{key}`"),
                IoDiagnosticCode::DawnLoad,
            );
            continue;
        }
        let object_type = match string_field(path, object_value, "type") {
            Ok(object_type) => object_type,
            Err(error) => {
                push_load_error(diagnostics, error, IoDiagnosticCode::DawnLoad);
                continue;
            }
        };
        let Some(kind) = source_object_kind(object_type) else {
            push_schema_error(
                diagnostics,
                path,
                source_range_for_field_value(path, object_value, "type"),
                format!("unsupported object type `{object_type}`"),
                IoDiagnosticCode::DawnLoad,
            );
            continue;
        };
        let sequence = (kind == SourceObjectKind::Sequence)
            .then(|| analyze_sequence(path, object_value, diagnostics))
            .flatten();
        objects.push(RecoveryObject {
            key: key.to_string(),
            kind,
            sequence,
        });
    }

    RecoveryDocument {
        kind: RecoveryDocumentKind::Dawn,
        objects,
    }
}

fn analyze_imports(
    path: &Utf8Path,
    root: &yaml_serde::Mapping,
    diagnostics: &mut Vec<IoDiagnostic>,
) {
    let Some(imports) = root.get(Value::String("imports".to_string())) else {
        return;
    };
    let Some(imports) = imports.as_sequence() else {
        push_schema_error(
            diagnostics,
            path,
            source_range_for_value(path, imports),
            "imports must be a sequence",
            IoDiagnosticCode::DawnLoad,
        );
        return;
    };
    let mut aliases = BTreeMap::new();
    for import in imports {
        let mut isolated = yaml_serde::Mapping::new();
        isolated.insert(
            Value::String("imports".to_string()),
            Value::Sequence(vec![import.clone()]),
        );
        match parse_imports(path, &isolated) {
            Ok(parsed) => {
                if let Some(parsed) = parsed.into_iter().next()
                    && let Some(previous_range) =
                        aliases.insert(parsed.alias.clone(), source_range_for_value(path, import))
                {
                    let mut diagnostic = schema_diagnostic(
                        path,
                        source_range_for_value(path, import),
                        format!("duplicate import alias `{}`", parsed.alias),
                        IoDiagnosticCode::DawnLoad,
                    );
                    diagnostic.related.push(crate::IoRelatedLocation {
                        path: path.to_path_buf(),
                        range: previous_range,
                        message: "first import with this alias".to_string(),
                    });
                    push_diagnostic(diagnostics, diagnostic);
                }
            }
            Err(error) => push_load_error(diagnostics, error, IoDiagnosticCode::DawnLoad),
        }
    }
}

pub(crate) fn check_dawn_document_text(path: &Utf8Path, text: &str) -> Vec<IoDiagnostic> {
    let mut diagnostics = Vec::new();
    let _ = analyze_dawn_text(path, text, &mut diagnostics);
    sort_diagnostics(&mut diagnostics);
    diagnostics
}

fn source_object_kind(value: &str) -> Option<SourceObjectKind> {
    Some(match value {
        "project" => SourceObjectKind::Project,
        "setup" => SourceObjectKind::Setup,
        "controller" => SourceObjectKind::Controller,
        "element_tree" => SourceObjectKind::ElementTree,
        "preview_layout" => SourceObjectKind::PreviewLayout,
        "patch" => SourceObjectKind::Patch,
        "prop" => SourceObjectKind::PropDefinition,
        "fixture_profile" => SourceObjectKind::FixtureProfile,
        "curve" => SourceObjectKind::Curve,
        "gradient" => SourceObjectKind::Gradient,
        "sequence" => SourceObjectKind::Sequence,
        _ => return None,
    })
}

fn analyze_sequence(
    path: &Utf8Path,
    value: &Value,
    diagnostics: &mut Vec<IoDiagnostic>,
) -> Option<RecoverySequence> {
    let duration_seconds = parse_field(
        diagnostics,
        string_field(path, value, "duration")
            .and_then(|duration| {
                parse_duration(duration).map_err(|error| {
                    with_yaml_location(
                        error,
                        path,
                        source_range_for_field_value(path, value, "duration"),
                    )
                })
            })
            .and_then(|duration| {
                (duration.as_seconds_f64() > 0.0)
                    .then_some(duration.as_seconds_f64())
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: source_range_for_field_value(path, value, "duration"),
                        message: "field `duration` must be greater than zero".to_string(),
                    })
            }),
        IoDiagnosticCode::SequenceField,
    );
    let frame_rate = parse_field(
        diagnostics,
        f64_field(path, value, "frame_rate").and_then(|frame_rate| {
            if frame_rate.is_finite() && frame_rate > 0.0 {
                Ok(frame_rate)
            } else {
                Err(LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_field_value(path, value, "frame_rate"),
                    message: "field `frame_rate` must be finite and greater than zero".to_string(),
                })
            }
        }),
        IoDiagnosticCode::SequenceField,
    );
    let (Some(duration_seconds), Some(frame_rate)) = (duration_seconds, frame_rate) else {
        return None;
    };

    let layers = analyze_layers(path, value, diagnostics);
    let mark_collections = analyze_mark_collections(path, value, diagnostics);
    let mut items = Vec::new();
    analyze_timeline_items(
        path,
        value,
        TimelineItemSchema::Effect,
        diagnostics,
        &mut items,
    );
    analyze_timeline_items(
        path,
        value,
        TimelineItemSchema::AutomationClip,
        diagnostics,
        &mut items,
    );
    analyze_control_items(path, value, diagnostics, &mut items);
    analyze_graph_items(path, value, diagnostics, &mut items);
    Some(RecoverySequence {
        duration_seconds,
        frame_rate,
        layers,
        mark_collections,
        items,
    })
}

fn analyze_layers(
    path: &Utf8Path,
    value: &Value,
    diagnostics: &mut Vec<IoDiagnostic>,
) -> Vec<RecoverySequenceLayer> {
    let values = match sequence_values(path, value, "layers") {
        Ok(values) => values,
        Err(error) => {
            push_load_error(diagnostics, error, IoDiagnosticCode::SequenceField);
            return Vec::new();
        }
    };
    let mut layers = Vec::new();
    let mut ids = BTreeMap::new();
    for layer in values {
        let Some(id) = collect_item_field(diagnostics, u32_field(path, layer, "id")) else {
            continue;
        };
        let Some(name) = collect_item_field(
            diagnostics,
            string_field(path, layer, "name").map(str::to_string),
        ) else {
            continue;
        };
        let Some(color) = collect_item_field(
            diagnostics,
            string_field(path, layer, "color").and_then(|color| {
                parse_color(color)
                    .map(|_| color.to_string())
                    .map_err(|error| {
                        with_yaml_location(
                            error,
                            path,
                            source_range_for_field_value(path, layer, "color"),
                        )
                    })
            }),
        ) else {
            continue;
        };
        let Some(enabled) = collect_item_field(diagnostics, bool_field(path, layer, "enabled"))
        else {
            continue;
        };
        if let Some(previous_range) = ids.insert(id, source_range_for_value(path, layer)) {
            let mut diagnostic = schema_diagnostic(
                path,
                source_range_for_field_value(path, layer, "id"),
                format!("duplicate sequence layer id `{id}`"),
                IoDiagnosticCode::SequenceItem,
            );
            diagnostic.related.push(crate::IoRelatedLocation {
                path: path.to_path_buf(),
                range: previous_range,
                message: "first layer with this id".to_string(),
            });
            push_diagnostic(diagnostics, diagnostic);
            continue;
        }
        layers.push(RecoverySequenceLayer {
            id,
            name,
            color,
            enabled,
        });
    }
    layers
}

fn analyze_mark_collections(
    path: &Utf8Path,
    value: &Value,
    diagnostics: &mut Vec<IoDiagnostic>,
) -> Vec<RecoveryMarkCollection> {
    let values = match optional_sequence(path, value, "mark_collections") {
        Ok(Some(values)) => values,
        Ok(None) => return Vec::new(),
        Err(error) => {
            push_load_error(diagnostics, error, IoDiagnosticCode::SequenceField);
            return Vec::new();
        }
    };
    let mut collections = Vec::new();
    for collection in values {
        let Some(key) = collect_item_field(
            diagnostics,
            string_field(path, collection, "key").map(str::to_string),
        ) else {
            continue;
        };
        let Some(name) = collect_item_field(
            diagnostics,
            string_field(path, collection, "name").map(str::to_string),
        ) else {
            continue;
        };
        let Some(color) = collect_item_field(
            diagnostics,
            string_field(path, collection, "color").and_then(|color| {
                parse_color(color)
                    .map(|_| color.to_string())
                    .map_err(|error| {
                        with_yaml_location(
                            error,
                            path,
                            source_range_for_field_value(path, collection, "color"),
                        )
                    })
            }),
        ) else {
            continue;
        };
        let marks = match sequence_values(path, collection, "marks") {
            Ok(marks) => marks,
            Err(error) => {
                push_load_error(diagnostics, error, IoDiagnosticCode::SequenceItem);
                continue;
            }
        };
        let mut marks_seconds = Vec::new();
        for mark in marks {
            let result = mark
                .as_str()
                .ok_or_else(|| LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_value(path, mark),
                    message: "marks must be duration strings".to_string(),
                })
                .and_then(|value| {
                    parse_duration_as_time(value).map_err(|error| {
                        with_yaml_location(error, path, source_range_for_value(path, mark))
                    })
                })
                .map(|time| time.as_seconds_f64());
            if let Some(mark) = collect_item_field(diagnostics, result) {
                marks_seconds.push(mark);
            }
        }
        collections.push(RecoveryMarkCollection {
            key,
            name,
            color,
            marks_seconds,
        });
    }
    collections
}

#[derive(Clone, Copy)]
enum TimelineItemSchema {
    Effect,
    AutomationClip,
}

impl TimelineItemSchema {
    fn field(self) -> &'static str {
        match self {
            Self::Effect => "effects",
            Self::AutomationClip => "automation_clips",
        }
    }

    fn kind(self) -> RecoverySequenceItemKind {
        match self {
            Self::Effect => RecoverySequenceItemKind::Effect,
            Self::AutomationClip => RecoverySequenceItemKind::AutomationClip,
        }
    }

    fn lane(self) -> TimelineLaneField {
        match self {
            Self::Effect => TimelineLaneField::Layer,
            Self::AutomationClip => TimelineLaneField::Lane,
        }
    }

    fn required(self) -> bool {
        matches!(self, Self::Effect)
    }
}

#[derive(Clone, Copy)]
enum TimelineLaneField {
    Layer,
    Lane,
}

impl TimelineLaneField {
    fn field(self) -> &'static str {
        match self {
            Self::Layer => "layer_id",
            Self::Lane => "lane_index",
        }
    }
}

fn analyze_timeline_items(
    path: &Utf8Path,
    sequence: &Value,
    schema: TimelineItemSchema,
    diagnostics: &mut Vec<IoDiagnostic>,
    items: &mut Vec<RecoverySequenceItem>,
) {
    let values = match if schema.required() {
        sequence_values(path, sequence, schema.field()).map(Some)
    } else {
        optional_sequence(path, sequence, schema.field())
    } {
        Ok(Some(values)) => values,
        Ok(None) => return,
        Err(error) => {
            push_load_error(diagnostics, error, IoDiagnosticCode::SequenceField);
            return;
        }
    };
    for value in values {
        let id = u32_field(path, value, "id");
        let start = string_field(path, value, "start").and_then(parse_duration_as_time);
        let duration = string_field(path, value, "duration").and_then(parse_duration);
        let lane = u32_field(path, value, schema.lane().field());
        let (Ok(id), Ok(start), Ok(duration), Ok(lane)) = (id, start, duration, lane) else {
            push_item_shape_errors(path, value, schema.lane(), diagnostics);
            continue;
        };
        items.push(RecoverySequenceItem {
            kind: schema.kind(),
            id: id.to_string(),
            placement: RecoverySequencePlacement::Timeline {
                start_seconds: start.as_seconds_f64(),
                duration_seconds: duration.as_seconds_f64(),
                lane: match schema.lane() {
                    TimelineLaneField::Layer => RecoveryTimelineLane::Layer(lane),
                    TimelineLaneField::Lane => RecoveryTimelineLane::Lane(lane),
                },
            },
            valid: true,
            message: None,
        });
    }
}

fn push_item_shape_errors(
    path: &Utf8Path,
    value: &Value,
    lane: TimelineLaneField,
    diagnostics: &mut Vec<IoDiagnostic>,
) {
    let checks = [
        u32_field(path, value, "id").map(|_| ()),
        string_field(path, value, "start")
            .and_then(parse_duration_as_time)
            .map(|_| ()),
        string_field(path, value, "duration")
            .and_then(parse_duration)
            .map(|_| ()),
        u32_field(path, value, lane.field()).map(|_| ()),
    ];
    for error in checks.into_iter().filter_map(Result::err) {
        push_load_error(diagnostics, error, IoDiagnosticCode::SequenceItem);
    }
}

fn analyze_control_items(
    path: &Utf8Path,
    sequence: &Value,
    diagnostics: &mut Vec<IoDiagnostic>,
    _items: &mut Vec<RecoverySequenceItem>,
) {
    let values = match optional_sequence(path, sequence, "control_clips") {
        Ok(Some(values)) => values,
        Ok(None) => return,
        Err(error) => {
            push_load_error(diagnostics, error, IoDiagnosticCode::SequenceField);
            return;
        }
    };
    for value in values {
        let id = collect_item_field(diagnostics, u32_field(path, value, "id"));
        let start = collect_item_field(
            diagnostics,
            string_field(path, value, "start").and_then(parse_duration_as_time),
        );
        let duration = collect_item_field(
            diagnostics,
            string_field(path, value, "duration").and_then(parse_duration),
        );
        // Control lanes are resolved from semantic targets. Without a complete project
        // model there is no trustworthy lane coordinate, so these remain Problems-only.
        let _ = (id, start, duration);
    }
}

fn analyze_graph_items(
    path: &Utf8Path,
    sequence: &Value,
    diagnostics: &mut Vec<IoDiagnostic>,
    items: &mut Vec<RecoverySequenceItem>,
) {
    let graph = match required_field(path, sequence, "composition_graph") {
        Ok(graph) => graph,
        Err(error) => {
            push_load_error(diagnostics, error, IoDiagnosticCode::SequenceField);
            return;
        }
    };
    let nodes = match sequence_values(path, graph, "nodes") {
        Ok(nodes) => nodes,
        Err(error) => {
            push_load_error(diagnostics, error, IoDiagnosticCode::SequenceField);
            return;
        }
    };
    for node in nodes {
        let id = u32_field(path, node, "id");
        let position = required_field(path, node, "position");
        let parsed = position.and_then(|position| {
            Ok((
                f64_field(path, position, "x")?,
                f64_field(path, position, "y")?,
            ))
        });
        match (id, parsed) {
            (Ok(id), Ok((x, y))) => items.push(RecoverySequenceItem {
                kind: RecoverySequenceItemKind::GraphNode,
                id: id.to_string(),
                placement: RecoverySequencePlacement::Graph { x, y },
                valid: true,
                message: None,
            }),
            (id, parsed) => {
                if let Err(error) = id {
                    push_load_error(diagnostics, error, IoDiagnosticCode::SequenceItem);
                }
                if let Err(error) = parsed {
                    push_load_error(diagnostics, error, IoDiagnosticCode::SequenceItem);
                }
            }
        }
    }
}

fn parse_field<T>(
    diagnostics: &mut Vec<IoDiagnostic>,
    result: Result<T, LoadProjectError>,
    code: IoDiagnosticCode,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            push_load_error(diagnostics, error, code);
            None
        }
    }
}

fn collect_item_field<T>(
    diagnostics: &mut Vec<IoDiagnostic>,
    result: Result<T, LoadProjectError>,
) -> Option<T> {
    parse_field(diagnostics, result, IoDiagnosticCode::SequenceItem)
}

fn push_load_error(
    diagnostics: &mut Vec<IoDiagnostic>,
    error: LoadProjectError,
    code: IoDiagnosticCode,
) {
    let mut diagnostic = load_error_diagnostic(error);
    diagnostic.code = code;
    push_diagnostic(diagnostics, diagnostic);
}

fn push_schema_error(
    diagnostics: &mut Vec<IoDiagnostic>,
    path: &Utf8Path,
    range: Option<TextRange>,
    message: impl Into<String>,
    code: IoDiagnosticCode,
) {
    push_diagnostic(diagnostics, schema_diagnostic(path, range, message, code));
}

fn schema_diagnostic(
    path: &Utf8Path,
    range: Option<TextRange>,
    message: impl Into<String>,
    code: IoDiagnosticCode,
) -> IoDiagnostic {
    IoDiagnostic {
        path: path.to_path_buf(),
        range,
        severity: IoDiagnosticSeverity::Error,
        code,
        message: message.into(),
        detail: None,
        related: Vec::new(),
    }
}

pub(crate) fn package_parse_diagnostic(
    path: &str,
    error: dawn_package::PackageFileParseError,
    code: IoDiagnosticCode,
) -> IoDiagnostic {
    let position = TextPosition {
        line: error.line,
        character: error.column,
    };
    IoDiagnostic {
        path: Utf8PathBuf::from(path),
        range: Some(TextRange {
            start: position.clone(),
            end: TextPosition {
                line: position.line,
                character: position.character.saturating_add(1),
            },
        }),
        severity: IoDiagnosticSeverity::Error,
        code,
        message: error.message,
        detail: error.field_path.map(|path| format!("JSON field: {path}")),
        related: Vec::new(),
    }
}

pub(crate) fn package_validation_diagnostics(
    path: &str,
    issues: Vec<dawn_package::PackageValidationIssue>,
    code: IoDiagnosticCode,
) -> Vec<IoDiagnostic> {
    issues
        .into_iter()
        .map(|issue| IoDiagnostic {
            path: Utf8PathBuf::from(path),
            range: None,
            severity: IoDiagnosticSeverity::Error,
            code: code.clone(),
            message: issue.message,
            detail: Some(format!("JSON field: {}", issue.field_path)),
            related: Vec::new(),
        })
        .collect()
}

pub(crate) fn sort_diagnostics(diagnostics: &mut Vec<IoDiagnostic>) {
    diagnostics.sort_by(|left, right| {
        let position = |diagnostic: &IoDiagnostic| {
            diagnostic
                .range
                .as_ref()
                .map(|range| (range.start.line, range.start.character))
                .unwrap_or((u32::MAX, u32::MAX))
        };
        left.path
            .cmp(&right.path)
            .then_with(|| position(left).cmp(&position(right)))
            .then_with(|| left.message.cmp(&right.message))
    });
    let mut seen = HashSet::new();
    diagnostics.retain(|diagnostic| {
        let range = matches!(
            diagnostic.code,
            IoDiagnosticCode::EffectCompile | IoDiagnosticCode::OperatorCompile
        )
        .then_some(None)
        .unwrap_or_else(|| diagnostic.range.clone());
        seen.insert((diagnostic.path.clone(), range, diagnostic.message.clone()))
    });
}
