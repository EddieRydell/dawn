use std::rc::Rc;

use dawn_project::path::ProjectPath;
use floem::prelude::*;

use crate::actions::AppAction;
use crate::app_model::AppSnapshot;
use crate::ui::theme;

pub fn preview_view(state: AppSnapshot, dispatch: crate::ui::UiDispatch) -> impl IntoView {
    let play = Rc::clone(&dispatch);
    let pause = Rc::clone(&dispatch);
    let back = Rc::clone(&dispatch);
    let forward = Rc::clone(&dispatch);
    let open_sequence = Rc::clone(&dispatch);
    let active_file = state.active_file.clone();
    let time = state.playback.time;

    v_stack((
        static_label("Preview").style(|s| s.font_bold()),
        h_stack((
            button("Use Active Sequence").action(move || {
                if let Some(path) = active_file.clone() {
                    if path.to_slash_string().ends_with(".dawn") {
                        open_sequence(AppAction::OpenSequence(path));
                    }
                }
            }),
            button("Play").action(move || play(AppAction::Play)),
            button("Pause").action(move || pause(AppAction::Pause)),
        ))
        .style(|s| s.gap(6.0)),
        h_stack((
            button("-0.05").action(move || back(AppAction::Seek((time - 0.05).max(0.0)))),
            label(move || format!("{time:.2}s / 30.00s")),
            button("+0.05").action(move || forward(AppAction::Seek((time + 0.05).min(30.0)))),
        ))
        .style(|s| s.gap(8.0).items_center()),
        frame_readout(state),
    ))
    .style(|s| s.height_full().padding(10.0).gap(10.0))
}

fn frame_readout(state: AppSnapshot) -> impl IntoView {
    let active_sequence = state
        .active_file
        .as_ref()
        .map(ProjectPath::to_slash_string)
        .unwrap_or_else(|| "No active file".to_string());
    let frame = state.preview_frame.clone();

    v_stack((
        static_label("Frame"),
        label(move || format!("Active: {active_sequence}")),
        label(move || {
            frame
                .as_ref()
                .map(|frame| {
                    format!(
                        "{} pixels, {} fixture spans, {} warnings",
                        frame.pixels,
                        frame.fixture_spans,
                        frame.warnings.as_ref().map_or(0, Vec::len)
                    )
                })
                .unwrap_or_else(|| "No sequence frame".to_string())
        }),
    ))
    .style(|s| {
        s.padding(10.0)
            .gap(6.0)
            .border(1.0)
            .border_color(theme::color(theme::BORDER))
            .background(theme::color(theme::PANEL))
    })
}
