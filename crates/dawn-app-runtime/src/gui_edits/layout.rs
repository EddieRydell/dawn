use dawn_language::document::LayoutDocument;

use crate::dto::LayoutGuiEditDto;

pub fn apply_layout_gui_edit(
    document: &mut LayoutDocument,
    edit: LayoutGuiEditDto,
) -> Result<(), String> {
    match edit {
        LayoutGuiEditDto::UpdatePlacementTransform { id, transform } => {
            let id = dawn_language::model::FixtureId(id);
            let placement = document
                .fixtures
                .iter_mut()
                .find(|fixture| fixture.id == id)
                .ok_or_else(|| format!("fixture placement `{id}` was not found"))?;
            placement.transform = transform
                .try_into()
                .map_err(|error: &'static str| error.to_string())?;
        }
    }
    Ok(())
}
