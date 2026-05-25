use std::rc::Rc;

use dawn_project::analysis::{ProjectDiagnostic, TextRange};
use dawn_project::path::PathStringExt;
use floem::prelude::*;
use floem::style::Selectable;
use floem::Clipboard;

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
                diagnostic_row(diagnostic, dispatch)
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

fn diagnostic_row(diagnostic: ProjectDiagnostic, dispatch: crate::ui::UiDispatch) -> impl IntoView {
    let path = diagnostic.path.clone();
    let (line, column) = range_start_to_one_based(diagnostic.range);
    let location = format!("{}:{}:{}", diagnostic.path.to_slash_string(), line, column);
    let kind = format!("{:?}  {:?}", diagnostic.severity, diagnostic.code);
    let copy_text = diagnostic_text(&diagnostic);
    let copy_text_for_button = copy_text.clone();
    let open = Rc::clone(&dispatch);

    v_stack((
        ui_static_label(location).style(|s| {
            s.width_full()
                .font_size(theme::FONT_SMALL)
                .color(theme::color(theme::MUTED))
                .set(Selectable, true)
        }),
        ui_static_label(kind).style(|s| {
            s.width_full()
                .font_size(theme::FONT_SMALL)
                .color(theme::color(theme::MUTED))
                .set(Selectable, true)
        }),
        ui_static_label(diagnostic.message).style(|s| s.width_full().set(Selectable, true)),
        h_stack((
            empty().style(|s| s.flex_grow(1.0).min_width(0.0)),
            ui_button("Open").action(move || {
                open(AppAction::OpenFile(path.clone()));
            }),
            ui_button("Copy").action(move || {
                let _ = Clipboard::set_contents(copy_text_for_button.clone());
            }),
        ))
        .style(|s| s.width_full().items_center().gap(theme::SPACE_6)),
    ))
    .style(|s| {
        s.width_full()
            .padding(theme::SPACE_8)
            .gap(theme::SPACE_4)
            .border_bottom(theme::BORDER_WIDTH)
            .border_color(theme::color(theme::BORDER))
    })
}

fn diagnostic_text(diagnostic: &ProjectDiagnostic) -> String {
    let (line, column) = range_start_to_one_based(diagnostic.range);
    format!(
        "{}:{}:{}\n{:?} {:?}\n{}",
        diagnostic.path.to_slash_string(),
        line,
        column,
        diagnostic.severity,
        diagnostic.code,
        diagnostic.message
    )
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
