use std::rc::Rc;

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
        .style(|s| s.gap(theme::SPACE_6)),
        h_stack((
            button(format!("-{:.2}", theme::PREVIEW_STEP_SECONDS)).action(move || {
                back(AppAction::Seek(
                    (time - theme::PREVIEW_STEP_SECONDS).max(0.0),
                ))
            }),
            label(move || format!("{time:.2}s / {:.2}s", theme::PREVIEW_DURATION_SECONDS)),
            button(format!("+{:.2}", theme::PREVIEW_STEP_SECONDS)).action(move || {
                forward(AppAction::Seek(
                    (time + theme::PREVIEW_STEP_SECONDS).min(theme::PREVIEW_DURATION_SECONDS),
                ))
            }),
        ))
        .style(|s| s.gap(theme::SPACE_8).items_center()),
        sequence_readout(state),
    ))
    .style(|s| {
        s.height_full()
            .padding(theme::SPACE_10)
            .gap(theme::SPACE_10)
    })
}

fn sequence_readout(state: AppSnapshot) -> impl IntoView {
    let active_sequence = state
        .active_file
        .as_ref()
        .map(|path| path.to_slash_string())
        .unwrap_or_else(|| "No active file".to_string());

    v_stack((
        static_label("Sequence"),
        label(move || format!("Active: {active_sequence}")),
    ))
    .style(|s| {
        s.padding(theme::SPACE_10)
            .gap(theme::SPACE_6)
            .border(theme::BORDER_WIDTH)
            .border_color(theme::color(theme::BORDER))
            .background(theme::color(theme::PANEL))
    })
}
