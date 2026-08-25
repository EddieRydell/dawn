use super::renderer::PreviewInstanceGpu;
use super::*;

#[derive(Clone, Debug)]
pub(crate) struct PreviewScene {
    pub(crate) revision: u64,
    pub(crate) instances: Vec<PreviewInstanceGpu>,
    pub(crate) bindings: Vec<ElementCellAddress>,
    pub(crate) bounds: PreviewBounds,
}

impl PreviewScene {
    pub fn from_project(revision: u64, project: &DawnProject) -> Self {
        let Some(setup) = project.setups.get(&project.root.setup) else {
            return Self::empty(revision);
        };
        let Some(layout) = project.preview_layouts.get(&setup.preview) else {
            return Self::empty(revision);
        };

        let mut instances = Vec::new();
        let mut bindings = Vec::new();
        for prop in &layout.props {
            let Some(definition) = project.definitions.props.definitions.get(&prop.definition)
            else {
                continue;
            };
            let position = point3_meters(prop.position);
            let transform = Mat4::from_translation(Vec3::new(
                position.x_meters as f32,
                position.y_meters as f32,
                position.z_meters as f32,
            )) * Mat4::from_euler(
                EulerRot::XYZ,
                prop.rotation.x.to_radians() as f32,
                prop.rotation.y.to_radians() as f32,
                prop.rotation.z.to_radians() as f32,
            ) * Mat4::from_scale(Vec3::new(
                prop.scale.x as f32,
                prop.scale.y as f32,
                prop.scale.z as f32,
            ));
            let radius_meters = definition.bulb_radius.as_meters_f64() as f32;
            for (emitter, binding) in geometry_emitters(&definition.geometry)
                .into_iter()
                .zip(&prop.bindings)
            {
                let point = transform.transform_point3(Vec3::new(
                    emitter.x_meters as f32,
                    emitter.y_meters as f32,
                    emitter.z_meters as f32,
                ));
                instances.push(PreviewInstanceGpu {
                    center_radius: [point.x, point.y, radius_meters.max(0.005), 0.0],
                });
                bindings.push(*binding);
            }
        }

        let bounds = PreviewBounds::from_instances(&instances);
        Self {
            revision,
            instances,
            bindings,
            bounds,
        }
    }

    fn empty(revision: u64) -> Self {
        Self {
            revision,
            instances: Vec::new(),
            bindings: Vec::new(),
            bounds: PreviewBounds::default(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreviewBounds {
    min: Vec2,
    max: Vec2,
}

impl PreviewBounds {
    pub(crate) fn from_instances(instances: &[PreviewInstanceGpu]) -> Self {
        let Some(first) = instances.first() else {
            return Self::default();
        };
        let mut min = instance_position(first);
        let mut max = instance_position(first);
        for instance in instances.iter().skip(1) {
            let position = instance_position(instance);
            min = min.min(position);
            max = max.max(position);
        }
        Self { min, max }
    }
}

pub(crate) fn instance_position(instance: &PreviewInstanceGpu) -> Vec2 {
    Vec2::new(instance.center_radius[0], instance.center_radius[1])
}

impl Default for PreviewBounds {
    fn default() -> Self {
        Self {
            min: Vec2::ZERO,
            max: Vec2::new(1.0, 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreviewCamera {
    pub(crate) pan: Vec2,
    pub(crate) zoom: f32,
}

impl PreviewCamera {
    pub(crate) fn fit(bounds: PreviewBounds, size: PreviewSize) -> Self {
        let span = (bounds.max - bounds.min).max(Vec2::splat(1.0));
        let available = Vec2::new(size.width as f32, size.height as f32) * 0.82;
        let zoom = (available.x / span.x).min(available.y / span.y).max(1.0);
        Self {
            pan: (bounds.min + bounds.max) * 0.5,
            zoom,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PreviewSize {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl PreviewSize {
    pub(crate) fn clamp_to_max_dimension(self, max_dimension: u32) -> Self {
        Self {
            width: self.width.min(max_dimension),
            height: self.height.min(max_dimension),
        }
    }
}
