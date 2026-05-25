use std::rc::Rc;

use dawn_project::analysis::TextRange;
use floem::prelude::*;

use crate::actions::AppAction;
use crate::app_model::AppSnapshot;
use crate::ui::components::{ui_button, ui_static_label};
use crate::ui::theme;

pub fn diagnostics_view(state: AppSnapshot, dispatch: crate::ui::UiDispatch) -> impl IntoView {
    let rows = state.diagnostics.clone();
    let body = if rows.is_empty() {
        ui_static_label("No diagnostics").into_any()
    } else {
        scroll(
            v_stack_from_iter(rows.into_iter().map(move |diagnostic| {
                let dispatch = Rc::clone(&dispatch);
                let path = diagnostic.path.clone();
                let (line, column) = range_start_to_one_based(diagnostic.range);
                let summary = format!(
                    "{}:{}:{}  {:?}  {}",
                    diagnostic.path.to_slash_string(),
                    line,
                    column,
                    diagnostic.severity,
                    diagnostic.message
                );
                ui_button(summary)
                    .action(move || {
                        dispatch(AppAction::OpenFile(path.clone()));
                    })
                    .style(|s| {
                        s.width_full()
                            .justify_start()
                            .padding(theme::SPACE_8)
                            .border_bottom(theme::BORDER_WIDTH)
                            .border_color(theme::color(theme::BORDER))
                    })
            }))
            .style(|s| s.width_full()),
        )
        .into_any()
    };

    v_stack((
        ui_static_label("Diagnostics").style(|s| s.font_bold()),
        body.style(|s| s.flex_grow(1.0).min_height(0.0)),
    ))
    .style(|s| s.height_full().padding(theme::SPACE_10).gap(theme::SPACE_8))
}

fn range_start_to_one_based(range: Option<TextRange>) -> (u32, u32) {
    range
        .map(|range| {
            (
                range.start.line.saturating_add(1),
                range.start.character.saturating_add(1),
            )
        })
        .unwrap_or((1, 1))
}
