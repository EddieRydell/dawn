use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::Marker;

use crate::analysis::{TextPosition, TextRange};
use crate::model::{
    Authored, Controller, Curve, DawnFile, DawnImport, DawnObject, Display, Fixture, Layout, Patch,
    Project, Sequence,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum YamlPathSegment {
    Field(String),
    Index(usize),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct YamlPath(Vec<YamlPathSegment>);

impl YamlPath {
    pub fn root() -> Self {
        Self(Vec::new())
    }

    pub fn field(mut self, field: impl Into<String>) -> Self {
        self.0.push(YamlPathSegment::Field(field.into()));
        self
    }

    pub fn index(mut self, index: usize) -> Self {
        self.0.push(YamlPathSegment::Index(index));
        self
    }

    fn push_field(&self, field: String) -> Self {
        let mut next = self.clone();
        next.0.push(YamlPathSegment::Field(field));
        next
    }

    fn push_index(&self, index: usize) -> Self {
        let mut next = self.clone();
        next.0.push(YamlPathSegment::Index(index));
        next
    }
}

#[derive(Debug, Clone)]
pub struct YamlSourceRange {
    pub key_range: Option<TextRange>,
    pub value_range: TextRange,
}

#[derive(Debug, Clone, Default)]
pub struct YamlSourceMap {
    ranges: HashMap<YamlPath, YamlSourceRange>,
}

impl YamlSourceMap {
    pub fn value_range(&self, path: YamlPath) -> Option<TextRange> {
        self.ranges.get(&path).map(|range| range.value_range)
    }

    pub fn key_range(&self, path: YamlPath) -> Option<TextRange> {
        self.ranges.get(&path).and_then(|range| range.key_range)
    }

    pub fn entry_range(&self, path: YamlPath) -> Option<TextRange> {
        self.value_range(path.clone())
            .or_else(|| self.key_range(path))
    }

    fn insert_value(&mut self, path: YamlPath, value_range: TextRange) {
        self.ranges
            .entry(path)
            .and_modify(|range| range.value_range = value_range)
            .or_insert(YamlSourceRange {
                key_range: None,
                value_range,
            });
    }

    fn insert_key(&mut self, path: YamlPath, key_range: TextRange) {
        if let Some(range) = self.ranges.get_mut(&path) {
            range.key_range = Some(key_range);
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParsedDawnFile {
    pub file: DawnFile,
    pub source_map: YamlSourceMap,
}

#[derive(Debug, Clone)]
pub struct DawnParseDiagnostic {
    pub message: String,
    pub range: Option<TextRange>,
}

impl fmt::Display for DawnParseDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for DawnParseDiagnostic {}

pub fn parse_dawn_file_with_source_map(text: &str) -> Result<ParsedDawnFile, DawnParseDiagnostic> {
    let source_map = build_source_map(text)?;
    let raw =
        serde_yaml::from_str::<IndexMap<String, serde_yaml::Value>>(text).map_err(|error| {
            DawnParseDiagnostic {
                message: error.to_string(),
                range: yaml_error_range(&error),
            }
        })?;

    let mut raw = raw;
    let imports = match raw.shift_remove("imports") {
        Some(value) => deserialize_at_path::<Vec<DawnImport>>(
            value,
            &source_map,
            YamlPath::root().field("imports"),
        )?,
        None => Vec::new(),
    };

    let mut objects = IndexMap::with_capacity(raw.len());
    for (key, value) in raw {
        let object =
            deserialize_object_at_path(value, &source_map, YamlPath::root().field(key.clone()))
                .map_err(|mut diagnostic| {
                    if !diagnostic.message.starts_with(&format!("{key}:")) {
                        diagnostic.message = format!("{key}: {}", diagnostic.message);
                    }
                    diagnostic
                })?;
        objects.insert(key, object);
    }

    Ok(ParsedDawnFile {
        file: DawnFile { imports, objects },
        source_map,
    })
}

fn deserialize_object_at_path(
    value: serde_yaml::Value,
    source_map: &YamlSourceMap,
    base_path: YamlPath,
) -> Result<DawnObject<Authored>, DawnParseDiagnostic> {
    let object_type = object_type(&value).ok_or_else(|| DawnParseDiagnostic {
        message: "missing field `type`".to_string(),
        range: source_map.entry_range(base_path.clone()),
    })?;
    let value = object_body(value);
    match object_type.as_str() {
        "project" => deserialize_at_path::<Project<Authored>>(value, source_map, base_path)
            .map(DawnObject::Project),
        "display" => deserialize_at_path::<Display<Authored>>(value, source_map, base_path)
            .map(DawnObject::Display),
        "controller" => deserialize_at_path::<Controller>(value, source_map, base_path)
            .map(DawnObject::Controller),
        "layout" => deserialize_at_path::<Layout<Authored>>(value, source_map, base_path)
            .map(DawnObject::Layout),
        "fixture" => {
            deserialize_at_path::<Fixture>(value, source_map, base_path).map(DawnObject::Fixture)
        }
        "patch" => deserialize_at_path::<Patch<Authored>>(value, source_map, base_path)
            .map(DawnObject::Patch),
        "sequence" => deserialize_at_path::<Sequence<Authored>>(value, source_map, base_path)
            .map(DawnObject::Sequence),
        "curve" => {
            deserialize_at_path::<Curve>(value, source_map, base_path).map(DawnObject::Curve)
        }
        other => Err(DawnParseDiagnostic {
            message: format!("unknown Dawn object type `{other}`"),
            range: source_map.entry_range(base_path.field("type")),
        }),
    }
}

fn object_type(value: &serde_yaml::Value) -> Option<String> {
    let mapping = value.as_mapping()?;
    mapping
        .get(serde_yaml::Value::String("type".to_string()))?
        .as_str()
        .map(str::to_string)
}

fn object_body(mut value: serde_yaml::Value) -> serde_yaml::Value {
    if let serde_yaml::Value::Mapping(mapping) = &mut value {
        mapping.remove(serde_yaml::Value::String("type".to_string()));
    }
    value
}

fn deserialize_at_path<T: DeserializeOwned>(
    value: serde_yaml::Value,
    source_map: &YamlSourceMap,
    base_path: YamlPath,
) -> Result<T, DawnParseDiagnostic> {
    serde_path_to_error::deserialize(value).map_err(|error| {
        let path = serde_error_path(&base_path, error.path());
        DawnParseDiagnostic {
            message: error.inner().to_string(),
            range: source_map.entry_range(path),
        }
    })
}

fn serde_error_path(base_path: &YamlPath, path: &serde_path_to_error::Path) -> YamlPath {
    let mut mapped = base_path.clone();
    for segment in path.iter() {
        match segment {
            serde_path_to_error::Segment::Seq { index } => {
                mapped = mapped.push_index(*index);
            }
            serde_path_to_error::Segment::Map { key } => {
                mapped = mapped.push_field(key.to_string());
            }
            serde_path_to_error::Segment::Enum { variant } => {
                mapped = mapped.push_field(variant.to_string());
            }
            serde_path_to_error::Segment::Unknown => {}
        }
    }
    mapped
}

fn build_source_map(text: &str) -> Result<YamlSourceMap, DawnParseDiagnostic> {
    let mut receiver = EventCollector::default();
    Parser::new_from_str(text)
        .load(&mut receiver, false)
        .map_err(|error| DawnParseDiagnostic {
            message: error.to_string(),
            range: Some(marker_range(error.marker())),
        })?;

    let mut builder = SourceMapBuilder {
        events: receiver.events,
        cursor: 0,
        source_map: YamlSourceMap::default(),
    };
    while builder.cursor < builder.events.len() {
        match &builder.events[builder.cursor].0 {
            Event::StreamStart | Event::DocumentStart => builder.cursor += 1,
            Event::StreamEnd | Event::DocumentEnd => break,
            _ => {
                builder.parse_node(&YamlPath::root())?;
                break;
            }
        }
    }
    Ok(builder.source_map)
}

#[derive(Default)]
struct EventCollector {
    events: Vec<(Event, Marker)>,
}

impl MarkedEventReceiver for EventCollector {
    fn on_event(&mut self, event: Event, marker: Marker) {
        self.events.push((event, marker));
    }
}

struct SourceMapBuilder {
    events: Vec<(Event, Marker)>,
    cursor: usize,
    source_map: YamlSourceMap,
}

impl SourceMapBuilder {
    fn parse_node(&mut self, path: &YamlPath) -> Result<TextRange, DawnParseDiagnostic> {
        let (event, marker) = self.next_event()?;
        let range = marker_range(&marker);
        self.source_map.insert_value(path.clone(), range);
        match event {
            Event::Scalar(..) | Event::Alias(_) => Ok(range),
            Event::MappingStart(..) => {
                while !self.next_is_mapping_end() {
                    let (key_event, key_marker) = self.next_event()?;
                    let Event::Scalar(key, ..) = key_event else {
                        return Err(DawnParseDiagnostic {
                            message: "mapping keys must be scalars".to_string(),
                            range: Some(marker_range(&key_marker)),
                        });
                    };
                    let child_path = path.push_field(key);
                    let key_range = marker_range(&key_marker);
                    let value_range = self.parse_node(&child_path)?;
                    self.source_map.insert_key(child_path.clone(), key_range);
                    self.source_map.insert_value(child_path, value_range);
                }
                self.cursor += 1;
                Ok(range)
            }
            Event::SequenceStart(..) => {
                let mut index = 0;
                while !self.next_is_sequence_end() {
                    let child_path = path.push_index(index);
                    self.parse_node(&child_path)?;
                    index += 1;
                }
                self.cursor += 1;
                Ok(range)
            }
            Event::Nothing
            | Event::StreamStart
            | Event::StreamEnd
            | Event::DocumentStart
            | Event::DocumentEnd
            | Event::MappingEnd
            | Event::SequenceEnd => Err(DawnParseDiagnostic {
                message: "unexpected YAML parser event".to_string(),
                range: Some(range),
            }),
        }
    }

    fn next_event(&mut self) -> Result<(Event, Marker), DawnParseDiagnostic> {
        let Some((event, marker)) = self.events.get(self.cursor).cloned() else {
            return Err(DawnParseDiagnostic {
                message: "unexpected end of YAML events".to_string(),
                range: None,
            });
        };
        self.cursor += 1;
        Ok((event, marker))
    }

    fn next_is_mapping_end(&self) -> bool {
        matches!(
            self.events.get(self.cursor).map(|(event, _)| event),
            Some(Event::MappingEnd)
        )
    }

    fn next_is_sequence_end(&self) -> bool {
        matches!(
            self.events.get(self.cursor).map(|(event, _)| event),
            Some(Event::SequenceEnd)
        )
    }
}

fn yaml_error_range(error: &serde_yaml::Error) -> Option<TextRange> {
    error.location().map(|location| {
        let line = location.line().saturating_sub(1) as u32;
        let character = location.column().saturating_sub(1) as u32;
        TextRange {
            start: TextPosition { line, character },
            end: TextPosition {
                line,
                character: character.saturating_add(1),
            },
        }
    })
}

fn marker_range(marker: &Marker) -> TextRange {
    let line = marker.line().saturating_sub(1) as u32;
    let character = marker.col() as u32;
    TextRange {
        start: TextPosition { line, character },
        end: TextPosition {
            line,
            character: character.saturating_add(1),
        },
    }
}
