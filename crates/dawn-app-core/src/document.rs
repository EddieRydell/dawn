use std::collections::HashMap;

use dawn_project::{
    ColorModel, Curve, CurveValueType, Distance, DistanceSpan, EffectParam, EffectScriptKind,
    Fixture, FixtureId, Geometry, LayoutTargetKind, ObjectKind, SequenceEffectScope, Transform,
};

#[derive(Debug, Clone)]
pub struct DocumentDescriptor {
    pub path: String,
    pub objects: Vec<DocumentObjectDescriptor>,
    pub available_views: Vec<DocumentViewId>,
    pub default_object_keys: HashMap<DocumentViewId, String>,
}

#[derive(Debug, Clone)]
pub struct DocumentObjectDescriptor {
    pub key: String,
    pub kind: ObjectKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocumentViewId {
    Text,
    Layout,
    Fixture,
    Sequence,
}

#[derive(Debug, Clone)]
pub struct SequenceDocument {
    pub path: String,
    pub object_key: String,
    pub duration_seconds: f64,
    pub frame_rate: u32,
    pub audio: Option<SequenceAudioDocument>,
    pub mark_collections: Vec<SequenceMarkCollectionDocument>,
    pub lanes: Vec<SequenceLaneDocument>,
    pub effect_scripts: Vec<SequenceEffectScriptDocument>,
    pub curve_library: Vec<SequenceCurveLibraryItemDocument>,
    pub effects: Vec<SequenceEffectDocument>,
    pub degraded: bool,
}

#[derive(Debug, Clone)]
pub struct SequenceAudioDocument {
    pub import: String,
    pub resolved_path: String,
    pub file_name: String,
    pub exists: bool,
}

#[derive(Debug, Clone)]
pub struct SequenceMarkCollectionDocument {
    pub key: String,
    pub name: String,
    pub color: String,
    pub marks_seconds: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct SequenceLaneDocument {
    pub target: LayoutTargetDocument,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct SequenceEffectScriptDocument {
    pub name: String,
    pub kind: EffectScriptKind,
    pub script: EffectScriptReferenceDocument,
    pub import: String,
    pub params: Vec<SequenceEffectScriptParamDocument>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EffectScriptReferenceDocument {
    pub path: String,
    pub effect_name: String,
}

#[derive(Debug, Clone)]
pub struct SequenceEffectScriptParamDocument {
    pub name: String,
    pub value_type: dawn_project::ScriptType,
}

#[derive(Debug, Clone)]
pub struct SequenceCurveLibraryItemDocument {
    pub path: String,
    pub object_key: String,
    pub display_name: String,
    pub value_type: CurveValueType,
    pub curve: Curve,
}

#[derive(Debug, Clone)]
pub struct SequenceEffectDocument {
    pub index: usize,
    pub id: u32,
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub target: LayoutTargetDocument,
    pub target_label: String,
    pub scope: SequenceEffectScope,
    pub script: String,
    pub script_source: Option<EffectScriptReferenceDocument>,
    pub params: Vec<SequenceEffectParamDocument>,
    pub render: Option<SequenceEffectRenderDocument>,
}

#[derive(Debug, Clone)]
pub struct SequenceEffectRenderDocument {
    pub script_source: String,
}

#[derive(Debug, Clone)]
pub struct SequenceEffectParamDocument {
    pub name: String,
    pub value: EffectParam<dawn_project::Resolved>,
    pub curve_source: Option<SequenceEffectParamCurveSourceDocument>,
}

#[derive(Debug, Clone)]
pub enum SequenceEffectParamCurveSourceDocument {
    Inline,
    Library {
        reference: String,
        path: Option<String>,
        object_key: Option<String>,
        display_name: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutTargetDocument {
    pub kind: LayoutTargetKind,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct LayoutDocument {
    pub path: String,
    pub object_key: String,
    pub name: String,
    pub render_bounds: GeometryRenderBounds,
    pub fixtures: Vec<LayoutFixturePlacement>,
}

#[derive(Debug, Clone)]
pub struct LayoutFixturePlacement {
    pub id: FixtureId,
    pub name: String,
    pub transform: Transform,
    pub resolved_fixture: ResolvedLayoutFixture,
}

#[derive(Debug, Clone)]
pub struct ResolvedLayoutFixture {
    pub name: String,
    pub color_model: ColorModel,
    pub bulb_diameter: DistanceSpan,
    pub geometry_summary: String,
    pub render_plan: GeometryRenderPlan,
    pub source_path: String,
    pub object_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FixtureDocument {
    pub path: String,
    pub selected_object_key: Option<String>,
    pub fixtures: Vec<FixtureDefinitionDocument>,
}

#[derive(Debug, Clone)]
pub struct FixtureDefinitionDocument {
    pub object_key: String,
    pub name: String,
    pub color_model: ColorModel,
    pub bulb_diameter: DistanceSpan,
    pub geometry: Geometry,
    pub geometry_summary: String,
    pub render_plan: GeometryRenderPlan,
}

#[derive(Debug, Clone, Copy)]
pub struct GeometryRenderPoint {
    pub x: Distance,
    pub y: Distance,
    pub z: Distance,
}

#[derive(Debug, Clone, Copy)]
pub struct GeometryRenderBounds {
    pub min_x: Distance,
    pub min_y: Distance,
    pub max_x: Distance,
    pub max_y: Distance,
}

#[derive(Debug, Clone)]
pub enum GeometryRenderGuide {
    Line {
        from: GeometryRenderPoint,
        to: GeometryRenderPoint,
    },
    Arc {
        start: GeometryRenderPoint,
        end: GeometryRenderPoint,
        radius_x: DistanceSpan,
        radius_y: DistanceSpan,
        rotation: f64,
        large_arc: bool,
        sweep_positive: bool,
    },
}

#[derive(Debug, Clone)]
pub struct GeometryRenderPlan {
    pub emitters: Vec<GeometryRenderPoint>,
    pub guides: Vec<GeometryRenderGuide>,
    pub bounds: GeometryRenderBounds,
    pub bulb_radius: DistanceSpan,
}

pub fn geometry_summary(geometry: &Geometry) -> String {
    match geometry {
        Geometry::Points { points } => format!("{} points", points.len()),
        Geometry::Lines { pixels, .. } => format!("{pixels} line pixels"),
        Geometry::Arc { pixels, .. } => format!("{pixels} arc pixels"),
    }
}

pub fn geometry_render_plan(fixture: &Fixture) -> GeometryRenderPlan {
    let emitters = geometry_emitters(&fixture.geometry);
    let guides = geometry_guides(&fixture.geometry);
    let bounds = geometry_bounds(&emitters).unwrap_or(GeometryRenderBounds {
        min_x: Distance::ZERO,
        min_y: Distance::ZERO,
        max_x: Distance::ZERO,
        max_y: Distance::ZERO,
    });
    GeometryRenderPlan {
        emitters,
        guides,
        bounds,
        bulb_radius: DistanceSpan::from_micrometers(fixture.bulb_diameter.as_micrometers() / 2),
    }
}

fn geometry_emitters(geometry: &Geometry) -> Vec<GeometryRenderPoint> {
    match geometry {
        Geometry::Points { points } | Geometry::Lines { points, .. } => points
            .iter()
            .copied()
            .map(|point| GeometryRenderPoint {
                x: point.x,
                y: point.y,
                z: point.z,
            })
            .collect(),
        Geometry::Arc {
            center,
            radius,
            start_degrees,
            end_degrees,
            pixels,
        } => {
            let count = (*pixels).max(1);
            (0..count)
                .map(|index| {
                    let amount = if count == 1 {
                        0.0
                    } else {
                        f64::from(index) / f64::from(count - 1)
                    };
                    let degrees = start_degrees + (end_degrees - start_degrees) * amount;
                    let radians = degrees.to_radians();
                    let x = center.x.as_meters_f64() + radius.as_meters_f64() * radians.cos();
                    let y = center.y.as_meters_f64() + radius.as_meters_f64() * radians.sin();
                    GeometryRenderPoint {
                        x: Distance::try_from_meters_f64_truncated(x).unwrap_or(Distance::ZERO),
                        y: Distance::try_from_meters_f64_truncated(y).unwrap_or(Distance::ZERO),
                        z: center.z,
                    }
                })
                .collect()
        }
    }
}

fn geometry_guides(geometry: &Geometry) -> Vec<GeometryRenderGuide> {
    match geometry {
        Geometry::Lines { points, .. } => points
            .windows(2)
            .map(|window| GeometryRenderGuide::Line {
                from: GeometryRenderPoint {
                    x: window[0].x,
                    y: window[0].y,
                    z: window[0].z,
                },
                to: GeometryRenderPoint {
                    x: window[1].x,
                    y: window[1].y,
                    z: window[1].z,
                },
            })
            .collect(),
        Geometry::Arc {
            center,
            radius,
            start_degrees,
            end_degrees,
            ..
        } => {
            let start = arc_point(*center, *radius, *start_degrees);
            let end = arc_point(*center, *radius, *end_degrees);
            vec![GeometryRenderGuide::Arc {
                start,
                end,
                radius_x: *radius,
                radius_y: *radius,
                rotation: 0.0,
                large_arc: (end_degrees - start_degrees).abs() > 180.0,
                sweep_positive: end_degrees >= start_degrees,
            }]
        }
        Geometry::Points { .. } => Vec::new(),
    }
}

fn arc_point(
    center: dawn_project::Point3,
    radius: DistanceSpan,
    degrees: f64,
) -> GeometryRenderPoint {
    let radians = degrees.to_radians();
    GeometryRenderPoint {
        x: Distance::try_from_meters_f64_truncated(
            center.x.as_meters_f64() + radius.as_meters_f64() * radians.cos(),
        )
        .unwrap_or(Distance::ZERO),
        y: Distance::try_from_meters_f64_truncated(
            center.y.as_meters_f64() + radius.as_meters_f64() * radians.sin(),
        )
        .unwrap_or(Distance::ZERO),
        z: center.z,
    }
}

fn geometry_bounds(points: &[GeometryRenderPoint]) -> Option<GeometryRenderBounds> {
    let first = points.first()?;
    let mut min_x = first.x;
    let mut min_y = first.y;
    let mut max_x = first.x;
    let mut max_y = first.y;
    for point in points.iter().skip(1) {
        if point.x < min_x {
            min_x = point.x;
        }
        if point.y < min_y {
            min_y = point.y;
        }
        if point.x > max_x {
            max_x = point.x;
        }
        if point.y > max_y {
            max_y = point.y;
        }
    }
    Some(GeometryRenderBounds {
        min_x,
        min_y,
        max_x,
        max_y,
    })
}
