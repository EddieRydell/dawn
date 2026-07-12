pub(super) fn curve_value(curve: &Curve) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("curve");
    value.insert(
        string_value("points"),
        Value::Sequence(
            curve
                .points
                .iter()
                .map(|point| {
                    let mut value = Mapping::new();
                    value.insert(string_value("position"), number_value(point.position)?);
                    value.insert(string_value("value"), number_value(point.value)?);
                    Ok(Value::Mapping(value))
                })
                .collect::<Result<Vec<_>, ExportProjectError>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn gradient_value(gradient: &Gradient) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("gradient");
    value.insert(
        string_value("stops"),
        Value::Sequence(
            gradient
                .stops
                .iter()
                .map(|stop| {
                    let mut value = Mapping::new();
                    value.insert(string_value("position"), number_value(stop.position)?);
                    value.insert(string_value("color"), Value::String(stop.color.to_hex()));
                    Ok(Value::Mapping(value))
                })
                .collect::<Result<Vec<_>, ExportProjectError>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn geometry_value(geometry: &Geometry) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    match geometry {
        Geometry::Points { points } => {
            value.insert(string_value("type"), Value::String("points".to_string()));
            value.insert(
                string_value("points"),
                Value::Sequence(
                    points
                        .iter()
                        .map(point_value)
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            );
        }
        Geometry::Lines { points, pixels } => {
            value.insert(string_value("type"), Value::String("lines".to_string()));
            value.insert(
                string_value("points"),
                Value::Sequence(
                    points
                        .iter()
                        .map(point_value)
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            );
            value.insert(string_value("pixels"), number_value(*pixels)?);
        }
        Geometry::Arc {
            center,
            radius,
            start_degrees,
            end_degrees,
            pixels,
        } => {
            value.insert(string_value("type"), Value::String("arc".to_string()));
            value.insert(string_value("center"), point_value(center)?);
            value.insert(
                string_value("radius"),
                number_value(radius.as_meters_f64())?,
            );
            value.insert(string_value("startDegrees"), number_value(*start_degrees)?);
            value.insert(string_value("endDegrees"), number_value(*end_degrees)?);
            value.insert(string_value("pixels"), number_value(*pixels)?);
        }
    }
    Ok(Value::Mapping(value))
}

pub(super) fn transform_value(fixture: &FixtureInst) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("position"), point_value(&fixture.position)?);
    value.insert(string_value("rotation"), rotation_value(&fixture.rotation)?);
    value.insert(string_value("scale"), scale_value(&fixture.scale)?);
    Ok(Value::Mapping(value))
}

pub(super) fn point_value(point: &Point3) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("x"), number_value(point.x.as_meters_f64())?);
    value.insert(string_value("y"), number_value(point.y.as_meters_f64())?);
    value.insert(string_value("z"), number_value(point.z.as_meters_f64())?);
    Ok(Value::Mapping(value))
}

pub(super) fn rotation_value(rotation: &Rotation3) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("x"), number_value(rotation.x)?);
    value.insert(string_value("y"), number_value(rotation.y)?);
    value.insert(string_value("z"), number_value(rotation.z)?);
    Ok(Value::Mapping(value))
}

pub(super) fn scale_value(scale: &Scale3) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("x"), number_value(scale.x)?);
    value.insert(string_value("y"), number_value(scale.y)?);
    value.insert(string_value("z"), number_value(scale.z)?);
    Ok(Value::Mapping(value))
}

pub(super) fn layout_target_value(target: &LayoutTarget) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    match target {
        LayoutTarget::Fixture(id) => {
            value.insert(string_value("type"), Value::String("fixture".to_string()));
            value.insert(string_value("id"), number_value(id.0)?);
        }
        LayoutTarget::Group(id) => {
            value.insert(string_value("type"), Value::String("group".to_string()));
            value.insert(string_value("id"), number_value(id.0)?);
        }
    }
    Ok(Value::Mapping(value))
}

pub(super) fn effect_target_value(target: &EffectTarget) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    match target {
        EffectTarget::Fixture(id) => {
            value.insert(string_value("type"), Value::String("fixture".to_string()));
            value.insert(string_value("id"), number_value(id.0)?);
        }
        EffectTarget::Group(id) => {
            value.insert(string_value("type"), Value::String("group".to_string()));
            value.insert(string_value("id"), number_value(id.0)?);
        }
    }
    Ok(Value::Mapping(value))
}

pub(super) fn write_source_reference(
    session: &ProjectSession,
    from_document: &Utf8Path,
    kind: SourceObjectKind,
    identity: &SourceIdentity,
) -> Result<String, ExportProjectError> {
    let alias = session
        .source
        .documents
        .get(from_document)
        .into_iter()
        .flat_map(|document| &document.imports)
        .find(|edge| {
            edge.targets
                .iter()
                .any(|target| target == identity.document())
        })
        .map(|edge| edge.alias.clone())
        .ok_or_else(|| ExportProjectError::InvalidReference {
            path: from_document.to_path_buf(),
            reference: identity.object().to_string(),
            message: format!(
                "no import alias makes the {kind:?} target visible from this document"
            ),
        })?;
    Ok(format!("{alias}.{}", identity.object()))
}

pub(super) fn typed_object(object_type: &str) -> Mapping {
    let mut value = Mapping::new();
    value.insert(string_value("type"), Value::String(object_type.to_string()));
    value
}

pub(super) fn string_value(value: &str) -> Value {
    Value::String(value.to_string())
}

pub(super) fn number_value<T: serde::Serialize>(value: T) -> Result<Value, ExportProjectError> {
    yaml_serde::to_value(value).map_err(|source| ExportProjectError::Serialize {
        path: Utf8PathBuf::from("<sync>"),
        source,
    })
}

pub(super) fn seconds_string(seconds: f64) -> String {
    format!("{seconds}s")
}

pub(super) fn channel_order_name(order: &RgbChannelOrder) -> &'static str {
    match order {
        RgbChannelOrder::Rgb => "rgb",
        RgbChannelOrder::Rbg => "rbg",
        RgbChannelOrder::Grb => "grb",
        RgbChannelOrder::Gbr => "gbr",
        RgbChannelOrder::Brg => "brg",
        RgbChannelOrder::Bgr => "bgr",
    }
}
use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::effect::EffectTarget;
use dawn_language::identity::SourceIdentity;
use dawn_language::setup::{FixtureInst, Geometry, LayoutTarget, RgbChannelOrder};
use dawn_language::values::{Curve, Gradient, Point3, Rotation3, Scale3};
use yaml_serde::{Mapping, Value};

use super::ProjectSession;
use crate::ExportProjectError;
use crate::source::SourceObjectKind;
