use std::rc::Rc;

use floem::event::{Event, EventListener, EventPropagation};
use floem::keyboard::{Key, NamedKey};
use floem::peniko::Brush;
use floem::prelude::*;
use floem::reactive::create_effect;
use floem::style::Foreground;

use crate::ui::theme;

pub fn modal_layer<T, Content>(
    state: impl Fn() -> Option<T> + Clone + 'static,
    on_cancel: impl Fn() + 'static,
    content: impl Fn(T) -> Content + 'static,
) -> impl IntoView
where
    T: 'static,
    Content: IntoView + 'static,
{
    let cancel: Rc<dyn Fn()> = Rc::new(on_cancel);
    let state_for_content = state.clone();
    let state_for_style = state.clone();
    let state_for_focus = state.clone();
    let key_cancel = Rc::clone(&cancel);

    let layer = dyn_container(state_for_content, move |state| match state {
        Some(state) => active_modal_layer(state, Rc::clone(&cancel), &content).into_any(),
        None => empty().into_any(),
    })
    .keyboard_navigable()
    .on_event(EventListener::KeyDown, move |event| {
        if let Event::KeyDown(event) = event {
            if matches!(event.key.logical_key, Key::Named(NamedKey::Escape)) {
                key_cancel();
                return EventPropagation::Stop;
            }
        }
        EventPropagation::Continue
    })
    .style(move |s| {
        s.absolute()
            .size_full()
            .z_index(theme::MODAL_Z_INDEX)
            .color(theme::color(theme::TEXT))
            .set(Foreground, Brush::Solid(theme::color(theme::TEXT)))
            .apply_if(state_for_style().is_none(), |s| s.hide())
    });
    let layer_id = layer.id();

    create_effect(move |_| {
        if state_for_focus().is_some() {
            layer_id.request_focus();
        }
    });

    layer
}

fn active_modal_layer<T, Content>(
    state: T,
    cancel: Rc<dyn Fn()>,
    content: &impl Fn(T) -> Content,
) -> impl IntoView
where
    T: 'static,
    Content: IntoView + 'static,
{
    let backdrop_cancel = Rc::clone(&cancel);

    stack((
        container(empty())
            .on_event_stop(EventListener::PointerDown, move |_| {
                backdrop_cancel();
            })
            .style(|s| {
                s.absolute()
                    .size_full()
                    .background(floem::peniko::Color::rgba8(
                        theme::MODAL_BACKDROP_RED,
                        theme::MODAL_BACKDROP_GREEN,
                        theme::MODAL_BACKDROP_BLUE,
                        theme::MODAL_BACKDROP_ALPHA,
                    ))
            }),
        container(container(content(state)).style(|s| {
            s.width(theme::MODAL_DEFAULT_WIDTH)
                .max_height(theme::MODAL_DEFAULT_MAX_HEIGHT)
                .padding(theme::SPACE_12)
                .border(theme::BORDER_WIDTH)
                .border_color(theme::color(theme::BORDER))
                .border_radius(theme::CONTROL_RADIUS)
                .background(theme::color(theme::PANEL))
        }))
        .on_event_stop(EventListener::PointerDown, |_| {})
        .on_event_stop(EventListener::PointerUp, |_| {})
        .style(|s| s.absolute().size_full().items_center().justify_center()),
    ))
    .style(|s| s.absolute().size_full())
}
