use std::rc::Rc;

use floem::prelude::*;

use crate::actions::AppAction;
use crate::app_model::AppSnapshot;
use crate::ui::components::{ui_button, ui_label, ui_static_label};
use crate::ui::theme;

pub fn fixture_viewer(state: AppSnapshot, dispatch: crate::ui::UiDispatch) -> impl IntoView {
    let Some(document) = state.active_fixture_document.clone() else {
        return empty().into_any();
    };
    let fixtures = document.fixtures.clone();

    v_stack((
        ui_static_label("Fixture Definitions").style(|s| s.font_bold()),
        ui_label(move || {
            document
                .selected_object_key
                .clone()
                .map(|key| format!("Selected object: {key}"))
                .unwrap_or_else(|| "All fixture objects".to_string())
        }),
        scroll(v_stack_from_iter(fixtures.into_iter().map(
            move |fixture| {
                let smaller = Rc::clone(&dispatch);
                let larger = Rc::clone(&dispatch);
                let duplicate = Rc::clone(&dispatch);
                let delete = Rc::clone(&dispatch);
                let key = fixture.object_key.clone();
                let key_smaller = key.clone();
                let key_larger = key.clone();
                let key_duplicate = key.clone();
                let key_delete = key.clone();
                v_stack((
                    ui_static_label(format!("{}  {}", fixture.object_key, fixture.name))
                        .style(|s| s.font_bold()),
                    ui_static_label(format!(
                        "{:?}  bulb {:.2}  {}",
                        fixture.color_model, fixture.bulb_size, fixture.geometry_summary
                    )),
                    ui_static_label(format!(
                        "Pixels: {}  Guides: {}",
                        fixture.render_plan.emitters.len(),
                        fixture.render_plan.guides.len()
                    )),
                    h_stack((
                        ui_button("Bulb -").action(move || {
                            smaller(AppAction::AdjustFixtureBulb {
                                object_key: key_smaller.clone(),
                                delta: -theme::FIXTURE_BULB_STEP,
                            })
                        }),
                        ui_button("Bulb +").action(move || {
                            larger(AppAction::AdjustFixtureBulb {
                                object_key: key_larger.clone(),
                                delta: theme::FIXTURE_BULB_STEP,
                            })
                        }),
                        ui_button("Duplicate").action(move || {
                            duplicate(AppAction::DuplicateFixtureDefinition {
                                object_key: key_duplicate.clone(),
                            })
                        }),
                        ui_button("Delete").action(move || {
                            delete(AppAction::DeleteFixtureDefinition {
                                object_key: key_delete.clone(),
                            })
                        }),
                    ))
                    .style(|s| s.gap(theme::SPACE_4).items_center()),
                ))
                .style(|s| {
                    s.padding(theme::SPACE_10)
                        .gap(theme::SPACE_4)
                        .border_bottom(theme::BORDER_WIDTH)
                        .border_color(theme::color(theme::BORDER))
                })
            },
        )))
        .style(|s| s.flex_grow(1.0).min_height(0.0)),
    ))
    .style(|s| {
        s.height_full()
            .padding(theme::SPACE_12)
            .gap(theme::SPACE_8)
            .background(theme::color(theme::SURFACE))
    })
    .into_any()
}
