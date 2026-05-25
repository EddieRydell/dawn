use std::rc::Rc;

use floem::event::{Event, EventListener};
use floem::keyboard::{Key, NamedKey};
use floem::kurbo::{Point, Size};
use floem::peniko::{Brush, Color};
use floem::prelude::*;
use floem::reactive::create_effect;
use floem::style::{CursorStyle, Foreground, Selectable};
use floem::ViewId;

use crate::ui::components::ui_static_label;

#[derive(Clone)]
pub struct DropdownMenuController {
    state: RwSignal<Option<DropdownMenuState>>,
    active_index: RwSignal<Option<usize>>,
    window_size: RwSignal<Size>,
    style: Rc<DropdownMenuStyle>,
}

impl DropdownMenuController {
    pub fn new(style: DropdownMenuStyle) -> Self {
        let window_size = style.initial_window_size;
        Self {
            state: RwSignal::new(None),
            active_index: RwSignal::new(None),
            window_size: RwSignal::new(window_size),
            style: Rc::new(style),
        }
    }

    pub fn set_window_size(&self, size: Size) {
        self.window_size.set(size);
    }

    pub fn open_at(&self, position: Point, entries: Vec<DropdownMenuEntry>) {
        let entries = Rc::new(entries);
        self.active_index.set(first_enabled_index(&entries));
        self.state
            .set(Some(DropdownMenuState { position, entries }));
    }

    pub fn open_below_view(&self, anchor: ViewId, entries: Vec<DropdownMenuEntry>) {
        let rect = anchor.layout_rect();
        self.open_at(Point::new(rect.x0, rect.y1), entries);
    }

    pub fn open_at_view_point(
        &self,
        anchor: ViewId,
        local_point: Point,
        entries: Vec<DropdownMenuEntry>,
    ) {
        let rect = anchor.layout_rect();
        self.open_at(
            Point::new(rect.x0 + local_point.x, rect.y0 + local_point.y),
            entries,
        );
    }

    pub fn close(&self) {
        self.active_index.set(None);
        self.state.set(None);
    }

    fn activate_index(&self, index: usize) {
        let action = self.state.with_untracked(|state| {
            state
                .as_ref()
                .and_then(|state| match state.entries.get(index) {
                    Some(DropdownMenuEntry::Item(item)) if item.enabled => {
                        Some(Rc::clone(&item.action))
                    }
                    _ => None,
                })
        });

        if let Some(action) = action {
            action();
            self.close();
        }
    }

    fn activate_current(&self) {
        if let Some(index) = self.active_index.get_untracked() {
            self.activate_index(index);
        }
    }

    fn move_active(&self, direction: NavigationDirection) {
        let next_index = self.state.with_untracked(|state| {
            state.as_ref().and_then(|state| {
                next_enabled_index(&state.entries, self.active_index.get_untracked(), direction)
            })
        });
        self.active_index.set(next_index);
    }
}

#[derive(Clone)]
struct DropdownMenuState {
    position: Point,
    entries: Rc<Vec<DropdownMenuEntry>>,
}

#[derive(Clone)]
pub enum DropdownMenuEntry {
    Item(DropdownMenuItem),
    Separator,
}

#[derive(Clone)]
pub struct DropdownMenuItem {
    label: String,
    enabled: bool,
    action: Rc<dyn Fn()>,
}

impl DropdownMenuEntry {
    pub fn item(label: impl Into<String>, enabled: bool, action: impl Fn() + 'static) -> Self {
        Self::Item(DropdownMenuItem {
            label: label.into(),
            enabled,
            action: Rc::new(action),
        })
    }

    pub fn separator() -> Self {
        Self::Separator
    }
}

#[derive(Clone)]
pub struct DropdownMenuStyle {
    pub initial_window_size: Size,
    pub font_family: String,
    pub font_size: f64,
    pub line_height: f32,
    pub width: f64,
    pub row_height: f64,
    pub padding: f64,
    pub row_padding_horiz: f64,
    pub border_width: f64,
    pub border_radius: f64,
    pub background: Color,
    pub text: Color,
    pub disabled_text: Color,
    pub border: Color,
    pub active_background: Color,
    pub separator: Color,
    pub separator_height: f64,
    pub separator_margin_vert: f64,
    pub z_index: i32,
}

pub fn dropdown_menu_layer(controller: DropdownMenuController) -> impl IntoView {
    let layer_controller = controller.clone();
    let key_controller = controller.clone();
    let focus_controller = controller.clone();

    let layer = dyn_container(
        move || controller.state.get(),
        move |state| {
            if let Some(state) = state {
                active_layer(layer_controller.clone(), state).into_any()
            } else {
                empty().into_any()
            }
        },
    )
    .keyboard_navigable()
    .on_event_stop(EventListener::KeyDown, move |event| {
        if let Event::KeyDown(event) = event {
            match event.key.logical_key {
                Key::Named(NamedKey::Escape) => key_controller.close(),
                Key::Named(NamedKey::ArrowDown) => {
                    key_controller.move_active(NavigationDirection::Next)
                }
                Key::Named(NamedKey::ArrowUp) => {
                    key_controller.move_active(NavigationDirection::Previous)
                }
                Key::Named(NamedKey::Home) => {
                    key_controller.move_active(NavigationDirection::First)
                }
                Key::Named(NamedKey::End) => key_controller.move_active(NavigationDirection::Last),
                Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                    key_controller.activate_current()
                }
                _ => {}
            }
        }
    })
    .style(move |s| {
        let size = focus_controller.window_size.get();
        let is_open = focus_controller.state.with(|state| state.is_some());
        s.absolute()
            .size(size.width, size.height)
            .z_index(focus_controller.style.z_index)
            .apply_if(!is_open, |s| s.hide())
    });
    let layer_id = layer.id();

    create_effect(move |_| {
        if focus_controller.state.with(|state| state.is_some()) {
            layer_id.request_focus();
        }
    });

    layer
}

fn active_layer(controller: DropdownMenuController, state: DropdownMenuState) -> impl IntoView {
    let close_controller = controller.clone();
    let menu_controller = controller.clone();
    let position = menu_position(&controller, &state);

    stack((
        container(empty())
            .on_event_stop(EventListener::PointerDown, move |_| {
                close_controller.close()
            })
            .style(|s| s.size_full()),
        menu_view(menu_controller, state).style(move |s| {
            let style = &controller.style;
            s.absolute()
                .width(style.width)
                .padding(style.padding)
                .border(style.border_width)
                .border_color(style.border)
                .border_radius(style.border_radius)
                .background(style.background)
                .color(style.text)
                .set(Foreground, Brush::Solid(style.text))
                .margin_left(position.x as f32)
                .margin_top(position.y as f32)
                .z_index(style.z_index)
                .cursor(CursorStyle::Default)
        }),
    ))
    .style(|s| s.size_full())
}

fn menu_view(controller: DropdownMenuController, state: DropdownMenuState) -> impl IntoView {
    v_stack_from_iter(
        state
            .entries
            .iter()
            .cloned()
            .enumerate()
            .map(move |(index, entry)| match entry {
                DropdownMenuEntry::Item(item) => {
                    menu_item(controller.clone(), index, item).into_any()
                }
                DropdownMenuEntry::Separator => menu_separator(controller.clone()).into_any(),
            }),
    )
    .on_event_stop(EventListener::PointerDown, |_| {})
    .on_event_stop(EventListener::PointerUp, |_| {})
    .style(|s| s.width_full())
}

fn menu_item(
    controller: DropdownMenuController,
    index: usize,
    item: DropdownMenuItem,
) -> impl IntoView {
    let hover_controller = controller.clone();
    let activate_controller = controller.clone();
    let active_index = controller.active_index;
    let enabled = item.enabled;
    let label = item.label;

    container(ui_static_label(label).style({
        let style = Rc::clone(&controller.style);
        move |s| {
            let text_color = if enabled {
                style.text
            } else {
                style.disabled_text
            };
            s.width_full()
                .height(style.row_height)
                .items_center()
                .font_family(style.font_family.clone())
                .font_size(style.font_size)
                .line_height(style.line_height)
                .color(text_color)
                .set(Foreground, Brush::Solid(text_color))
                .set(Selectable, false)
                .text_ellipsis()
        }
    }))
    .on_event_stop(EventListener::PointerEnter, move |_| {
        if enabled {
            hover_controller.active_index.set(Some(index));
        }
    })
    .on_event_stop(EventListener::PointerUp, move |event| {
        if let Event::PointerUp(event) = event {
            if event.button.is_primary() && enabled {
                activate_controller.activate_index(index);
            }
        }
    })
    .style(move |s| {
        let style = &controller.style;
        let background = if active_index.get() == Some(index) && enabled {
            style.active_background
        } else {
            style.background
        };
        s.width(style.width - style.padding * 2.0)
            .height(style.row_height)
            .items_center()
            .padding_horiz(style.row_padding_horiz)
            .background(background)
            .cursor(CursorStyle::Default)
    })
}

fn menu_separator(controller: DropdownMenuController) -> impl IntoView {
    container(empty()).style(move |s| {
        let style = &controller.style;
        s.width(style.width - style.padding * 2.0)
            .height(style.separator_height)
            .margin_vert(style.separator_margin_vert)
            .background(style.separator)
    })
}

fn menu_position(controller: &DropdownMenuController, state: &DropdownMenuState) -> Point {
    let style = &controller.style;
    let window_size = controller.window_size.get();
    let menu_height = state
        .entries
        .iter()
        .fold(style.padding * 2.0, |height, entry| {
            height
                + match entry {
                    DropdownMenuEntry::Item(_) => style.row_height,
                    DropdownMenuEntry::Separator => {
                        style.separator_height + style.separator_margin_vert * 2.0
                    }
                }
        });

    let max_x = (window_size.width - style.width).max(0.0);
    let max_y = (window_size.height - menu_height).max(0.0);

    Point::new(
        state.position.x.clamp(0.0, max_x),
        state.position.y.clamp(0.0, max_y),
    )
}

fn first_enabled_index(entries: &[DropdownMenuEntry]) -> Option<usize> {
    entries.iter().position(is_enabled_item)
}

fn last_enabled_index(entries: &[DropdownMenuEntry]) -> Option<usize> {
    entries.iter().rposition(is_enabled_item)
}

fn is_enabled_item(entry: &DropdownMenuEntry) -> bool {
    matches!(entry, DropdownMenuEntry::Item(item) if item.enabled)
}

#[derive(Clone, Copy)]
enum NavigationDirection {
    Next,
    Previous,
    First,
    Last,
}

fn next_enabled_index(
    entries: &[DropdownMenuEntry],
    active_index: Option<usize>,
    direction: NavigationDirection,
) -> Option<usize> {
    match direction {
        NavigationDirection::First => first_enabled_index(entries),
        NavigationDirection::Last => last_enabled_index(entries),
        NavigationDirection::Next => cycle_enabled_index(entries, active_index, 1),
        NavigationDirection::Previous => cycle_enabled_index(entries, active_index, -1),
    }
}

fn cycle_enabled_index(
    entries: &[DropdownMenuEntry],
    active_index: Option<usize>,
    direction: isize,
) -> Option<usize> {
    let enabled = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| is_enabled_item(entry).then_some(index))
        .collect::<Vec<_>>();

    if enabled.is_empty() {
        return None;
    }

    let current_position = active_index
        .and_then(|index| {
            enabled
                .iter()
                .position(|enabled_index| *enabled_index == index)
        })
        .unwrap_or_else(|| if direction > 0 { enabled.len() - 1 } else { 0 });
    let next_position =
        (current_position as isize + direction).rem_euclid(enabled.len() as isize) as usize;
    Some(enabled[next_position])
}
