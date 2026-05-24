use std::rc::Rc;

use dawn_project::path::ProjectPath;
use floem::prelude::*;

use crate::actions::AppAction;
use crate::app_model::AppSnapshot;
use crate::ui::theme;

pub fn diagnostics_view(state: AppSnapshot, dispatch: crate::ui::UiDispatch) -> impl IntoView {
    let rows = state.diagnostics.clone();
    let body = if rows.is_empty() {
        static_label("No diagnostics").into_any()
    } else {
        scroll(
            v_stack_from_iter(rows.into_iter().map(move |problem| {
                let dispatch = Rc::clone(&dispatch);
                let path = ProjectPath::parse(&problem.path).ok();
                let summary = format!(
                    "{}:{}:{}  {:?}  {}",
                    problem.path, problem.line, problem.column, problem.severity, problem.message
                );
                button(summary)
                    .action(move || {
                        if let Some(path) = path.clone() {
                            dispatch(AppAction::OpenFile(path));
                        }
                    })
                    .style(|s| {
                        s.width_full()
                            .justify_start()
                            .padding(8.0)
                            .border_bottom(1.0)
                            .border_color(theme::color(theme::BORDER))
                    })
            }))
            .style(|s| s.width_full()),
        )
        .into_any()
    };

    v_stack((
        static_label("Diagnostics").style(|s| s.font_bold()),
        body.style(|s| s.flex_grow(1.0).min_height(0.0)),
    ))
    .style(|s| s.height_full().padding(10.0).gap(8.0))
}
