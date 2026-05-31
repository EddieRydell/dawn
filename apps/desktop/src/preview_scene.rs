use dawn_app_core::output_runtime::OutputFrame;

use crate::preview_types::{PreviewSceneDto, PreviewSceneFixtureDto};

pub(crate) fn preview_pixel_count(frame: &OutputFrame) -> usize {
    frame
        .fixtures
        .iter()
        .map(|fixture| fixture.pixels.len())
        .sum()
}

pub(crate) fn preview_scene_from_frame(
    frame: &OutputFrame,
    source_label: String,
) -> PreviewSceneDto {
    let mut first_pixel_index = 0usize;
    let fixtures = frame
        .fixtures
        .iter()
        .map(|fixture| {
            let pixels = fixture
                .pixels
                .iter()
                .map(|pixel| pixel.position.into())
                .collect::<Vec<_>>();
            let dto = PreviewSceneFixtureDto {
                id: fixture.id.0,
                name: fixture.name.clone(),
                bulb_radius_meters: fixture.bulb_radius.as_meters_f64(),
                first_pixel_index: first_pixel_index.min(u32::MAX as usize) as u32,
                pixels,
            };
            first_pixel_index = first_pixel_index.saturating_add(fixture.pixels.len());
            dto
        })
        .collect::<Vec<_>>();
    PreviewSceneDto {
        generation: frame.generation.min(u32::MAX as u64) as u32,
        source_label,
        bounds: frame.bounds.into(),
        pixel_count: first_pixel_index.min(u32::MAX as usize) as u32,
        fixtures,
    }
}
