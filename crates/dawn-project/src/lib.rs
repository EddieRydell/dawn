use std::collections::HashSet;
use std::ops::Range;
use std::path::Path;

mod model;

pub use model::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentRole {
    Yaml,
    Events,
    Effect,
}

pub fn detect_role(path: &Path) -> DocumentRole {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if name.ends_with(".effect.dawn") {
        DocumentRole::Effect
    } else if name.ends_with(".events.dawn") {
        DocumentRole::Events
    } else {
        DocumentRole::Yaml
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedDocument {
    pub role: DocumentRole,
    pub diagnostics: Vec<ProjectDiagnostic>,
    pub path_refs: Vec<PathRef>,
    pub symbols: Vec<ProjectSymbol>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDiagnostic {
    pub message: String,
    pub range: Option<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathRef {
    pub label: String,
    pub raw_path: String,
    pub target: Option<String>,
    pub range: Option<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSymbol {
    pub name: String,
    pub kind: ProjectSymbolKind,
    pub range: Range<usize>,
    pub selection_range: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSymbolKind {
    Document,
    Group,
    Fixture,
    Controller,
    Event,
    Effect,
}

pub fn parse_document(path: &Path, text: &str) -> ParsedDocument {
    match detect_role(path) {
        DocumentRole::Yaml => parse_yaml_document(text),
        DocumentRole::Events => parse_events_document(text),
        DocumentRole::Effect => parse_effect_document(text),
    }
}

pub fn parse_yaml_document(text: &str) -> ParsedDocument {
    let mut parsed = ParsedDocument {
        role: DocumentRole::Yaml,
        diagnostics: Vec::new(),
        path_refs: Vec::new(),
        symbols: Vec::new(),
    };

    let file = match serde_yaml::from_str::<DawnFile>(text) {
        Ok(file) => file,
        Err(error) => {
            parsed.diagnostics.push(ProjectDiagnostic {
                message: error.to_string(),
                range: None,
            });
            return parsed;
        }
    };

    if file.is_empty() {
        parsed.diagnostics.push(ProjectDiagnostic {
            message: "YAML .dawn files must contain at least one named Dawn object".to_string(),
            range: Some(0..text.len().min(1)),
        });
        return parsed;
    }

    for (name, object) in file {
        collect_object(text, name, object, &mut parsed);
    }
    parsed
}

fn parse_events_document(text: &str) -> ParsedDocument {
    let mut parsed = ParsedDocument {
        role: DocumentRole::Events,
        diagnostics: Vec::new(),
        path_refs: Vec::new(),
        symbols: Vec::new(),
    };
    let mut ids = HashSet::new();
    let mut offset = 0;

    for line in text.split_inclusive('\n') {
        let raw_line = line.trim_end_matches(['\r', '\n']);
        let line_start = offset;
        offset += line.len();
        if raw_line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<SequenceEvent>(raw_line) {
            Ok(event) => {
                let id = event.id().to_string();
                if !ids.insert(id.clone()) {
                    parsed.diagnostics.push(ProjectDiagnostic {
                        message: format!("duplicate event id `{id}`"),
                        range: line_value_range(raw_line, line_start, "id", &id),
                    });
                }
                let selection = line_value_range(raw_line, line_start, "id", &id)
                    .unwrap_or(line_start..line_start + raw_line.len());
                parsed.symbols.push(ProjectSymbol {
                    name: id,
                    kind: ProjectSymbolKind::Event,
                    range: line_start..line_start + raw_line.len(),
                    selection_range: selection,
                });
                if let SequenceEvent::Effect { effect, .. } = event {
                    parsed.path_refs.push(path_ref_from_import(
                        "effect",
                        &effect,
                        line_string_field_range(raw_line, line_start, "effect"),
                    ));
                }
            }
            Err(error) => parsed.diagnostics.push(ProjectDiagnostic {
                message: error.to_string(),
                range: Some(line_start..line_start + raw_line.len()),
            }),
        }
    }

    parsed
}

fn parse_effect_document(text: &str) -> ParsedDocument {
    let mut parsed = ParsedDocument {
        role: DocumentRole::Effect,
        diagnostics: Vec::new(),
        path_refs: Vec::new(),
        symbols: Vec::new(),
    };
    if let Some((name, selection)) = find_effect_name(text) {
        parsed.symbols.push(ProjectSymbol {
            name,
            kind: ProjectSymbolKind::Effect,
            range: 0..text.len(),
            selection_range: selection,
        });
    }
    parsed
}

fn collect_object(text: &str, name: String, object: DawnObject, parsed: &mut ParsedDocument) {
    let kind = match &object {
        DawnObject::Project(_)
        | DawnObject::Display(_)
        | DawnObject::Patch(_)
        | DawnObject::Sequence(_) => ProjectSymbolKind::Document,
        DawnObject::Controller(_) => ProjectSymbolKind::Controller,
        DawnObject::Layout(_) => ProjectSymbolKind::Document,
        DawnObject::Fixture(_) => ProjectSymbolKind::Fixture,
    };
    push_named_object_symbol(text, parsed, &name, kind);

    match object {
        DawnObject::Project(project) => collect_project(text, project, parsed),
        DawnObject::Display(display) => collect_display(text, display, parsed),
        DawnObject::Controller(_) => {}
        DawnObject::Layout(layout) => collect_layout(text, layout, parsed),
        DawnObject::Fixture(fixture) => collect_fixture(text, &fixture, parsed),
        DawnObject::Patch(patch) => collect_patch(text, patch, parsed),
        DawnObject::Sequence(sequence) => collect_sequence(text, sequence, parsed),
    }
}

fn collect_project(text: &str, project: Project, parsed: &mut ParsedDocument) {
    push_inline_import_ref(text, parsed, "display", &project.display);
    for sequence in &project.sequences {
        push_inline_import_ref(text, parsed, "sequences", sequence);
    }
}

fn collect_display(text: &str, display: Display, parsed: &mut ParsedDocument) {
    push_inline_import_ref(text, parsed, "layout", &display.layout);
    push_inline_import_ref(text, parsed, "patch", &display.patch);
    for controller in display.controllers {
        push_inline_import_ref(text, parsed, "controllers", &controller);
        if let Some(controller) = controller.inline() {
            push_symbol(
                text,
                parsed,
                controller.name.to_string(),
                "name",
                ProjectSymbolKind::Controller,
            );
        }
    }
}

fn collect_layout(text: &str, document: Layout, parsed: &mut ParsedDocument) {
    for placement in document.fixtures {
        push_symbol(text, parsed, placement.id, "id", ProjectSymbolKind::Fixture);
        push_inline_import_ref(text, parsed, "fixture", &placement.fixture);
        if let Some(fixture) = placement.fixture.inline() {
            collect_fixture(text, fixture, parsed);
        }
    }
    for group in document.groups {
        push_symbol(text, parsed, group.name, "name", ProjectSymbolKind::Group);
    }
}

fn collect_fixture(text: &str, fixture: &Fixture, parsed: &mut ParsedDocument) {
    match &fixture.geometry {
        Geometry::Points { points } if points.is_empty() => {
            parsed.diagnostics.push(ProjectDiagnostic {
                message: "fixture points geometry must contain at least one point".to_string(),
                range: find_key_range(text, "points"),
            });
        }
        Geometry::Line { pixels, .. } | Geometry::Arc { pixels, .. } if *pixels == 0 => {
            parsed.diagnostics.push(ProjectDiagnostic {
                message: "fixture geometry pixels must be positive".to_string(),
                range: find_key_range(text, "pixels"),
            });
        }
        Geometry::Lines { points, lines } => {
            if points.is_empty() {
                parsed.diagnostics.push(ProjectDiagnostic {
                    message: "fixture lines geometry must contain at least one point".to_string(),
                    range: find_key_range(text, "points"),
                });
            }
            if lines.is_empty() {
                parsed.diagnostics.push(ProjectDiagnostic {
                    message: "fixture lines geometry must contain at least one line".to_string(),
                    range: find_key_range(text, "lines"),
                });
            }
            let point_count = points.len();
            for line in lines {
                if line.from >= point_count || line.to >= point_count {
                    parsed.diagnostics.push(ProjectDiagnostic {
                        message: "fixture line index must refer to an existing point".to_string(),
                        range: find_key_range(text, "lines"),
                    });
                }
            }
        }
        Geometry::Arc { radius, .. } if *radius <= 0.0 => {
            parsed.diagnostics.push(ProjectDiagnostic {
                message: "fixture arc radius must be positive".to_string(),
                range: find_key_range(text, "radius"),
            });
        }
        _ => {}
    }
}

fn collect_patch(text: &str, patch: Patch, parsed: &mut ParsedDocument) {
    for route in patch.routes {
        push_symbol(
            text,
            parsed,
            route.group.as_str().to_string(),
            "group",
            ProjectSymbolKind::Group,
        );
    }
}

fn collect_sequence(text: &str, sequence: Sequence, parsed: &mut ParsedDocument) {
    push_import_ref(text, parsed, "events", &sequence.events);
}

fn push_named_object_symbol(
    text: &str,
    parsed: &mut ParsedDocument,
    name: &str,
    kind: ProjectSymbolKind,
) {
    let selection = find_key_range(text, name).unwrap_or(0..name.len().min(text.len()));
    parsed.symbols.push(ProjectSymbol {
        name: name.to_string(),
        kind,
        range: 0..text.len(),
        selection_range: selection,
    });
}

fn push_symbol(
    text: &str,
    parsed: &mut ParsedDocument,
    name: String,
    key: &'static str,
    kind: ProjectSymbolKind,
) {
    let selection = find_scalar_range(text, key, &name).unwrap_or(0..name.len().min(text.len()));
    parsed.symbols.push(ProjectSymbol {
        name,
        kind,
        range: 0..text.len(),
        selection_range: selection,
    });
}

fn push_inline_import_ref<T>(
    text: &str,
    parsed: &mut ParsedDocument,
    label: &'static str,
    value: &InlineOrImport<T>,
) {
    if let Some(import) = value.import_ref() {
        push_import_ref(text, parsed, label, import);
    }
}

fn push_import_ref(
    text: &str,
    parsed: &mut ParsedDocument,
    label: &'static str,
    import: &ImportRef,
) {
    let range = find_scalar_range(text, label, import.raw())
        .or_else(|| find_scalar_range(text, "import", import.raw()));
    parsed
        .path_refs
        .push(path_ref_from_import(label, import, range));
}

fn path_ref_from_import(
    label: &'static str,
    import: &ImportRef,
    range: Option<Range<usize>>,
) -> PathRef {
    let path = import.path().as_str();
    let target = import.object().map(|object| object.as_str());
    let range = range.map(|range| {
        let path_len = path.len().min(range.end.saturating_sub(range.start));
        range.start..range.start + path_len
    });
    PathRef {
        label: label.to_string(),
        raw_path: path.to_string(),
        target: target.map(str::to_string),
        range,
    }
}

fn find_key_range(text: &str, key: &str) -> Option<Range<usize>> {
    let needle = format!("{key}:");
    text.find(&needle).map(|start| start..start + key.len())
}

fn find_scalar_range(text: &str, key: &str, value: &str) -> Option<Range<usize>> {
    let key_start = text.find(&format!("{key}:"))?;
    let after_key = key_start + key.len() + 1;
    let search = &text[after_key..];
    let value_start = search.find(value)?;
    let start = after_key + value_start;
    Some(start..start + value.len())
}

fn line_value_range(line: &str, line_start: usize, key: &str, value: &str) -> Option<Range<usize>> {
    let field = format!("\"{key}\"");
    let after_key = line.find(&field)? + field.len();
    let relative = line[after_key..].find(value)?;
    let start = line_start + after_key + relative;
    Some(start..start + value.len())
}

fn line_string_field_range(line: &str, line_start: usize, key: &str) -> Option<Range<usize>> {
    let field = format!("\"{key}\"");
    let after_key = line.find(&field)? + field.len();
    let colon = line[after_key..].find(':')? + after_key;
    let value_quote = line[colon + 1..].find('"')? + colon + 1;
    let value_start = value_quote + 1;
    let value_end = line[value_start..].find('"')? + value_start;
    Some(line_start + value_start..line_start + value_end)
}

fn find_effect_name(text: &str) -> Option<(String, Range<usize>)> {
    let effect = text.find("effect")?;
    let after = effect + "effect".len();
    let rest = &text[after..];
    let ws = rest.find(|c: char| !c.is_whitespace())?;
    let start = after + ws;
    let name_len = text[start..]
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(text.len() - start);
    let end = start + name_len;
    (start < end).then(|| (text[start..end].to_string(), start..end))
}
