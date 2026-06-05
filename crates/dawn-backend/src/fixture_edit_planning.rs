use dawn_language::{
    document::FixtureDocument,
    model::{DistanceSpan, Geometry},
};

use crate::types::FixtureGuiEdit;

pub(crate) fn apply_fixture_gui_edit(
    document: &mut FixtureDocument,
    edit: FixtureGuiEdit,
) -> Result<(), String> {
    match edit {
        FixtureGuiEdit::UpdateBulbDiameter {
            object_key,
            bulb_diameter_meters,
        } => {
            let fixture = document
                .fixtures
                .iter_mut()
                .find(|fixture| fixture.object_key == object_key)
                .ok_or_else(|| format!("fixture `{object_key}` was not found"))?;
            fixture.bulb_diameter =
                DistanceSpan::try_from_meters_f64_truncated(bulb_diameter_meters)
                    .map_err(str::to_string)?;
        }
        FixtureGuiEdit::MovePoint {
            object_key,
            point_index,
            point,
        } => {
            let fixture = document
                .fixtures
                .iter_mut()
                .find(|fixture| fixture.object_key == object_key)
                .ok_or_else(|| format!("fixture `{object_key}` was not found"))?;
            let Geometry::Points { points } = &mut fixture.geometry else {
                return Err("only point geometry can be edited in this milestone".to_string());
            };
            let target = points
                .get_mut(point_index as usize)
                .ok_or_else(|| format!("point `{point_index}` was not found"))?;
            *target = point;
        }
    }
    Ok(())
}
