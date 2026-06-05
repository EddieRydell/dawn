use dawn_language::document::LayoutDocument;

use crate::gui_edits::types::LayoutGuiEdit;

pub fn apply_layout_gui_edit(
    document: &mut LayoutDocument,
    edit: LayoutGuiEdit,
) -> Result<(), String> {
    match edit {
        LayoutGuiEdit::UpdatePlacementTransform { id, transform } => {
            let id = dawn_language::model::FixtureId(id);
            let placement = document
                .fixtures
                .iter_mut()
                .find(|fixture| fixture.id == id)
                .ok_or_else(|| format!("fixture placement `{id}` was not found"))?;
            placement.transform = transform;
        }
    }
    Ok(())
}
