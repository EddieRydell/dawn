use std::{cell::RefCell, collections::HashMap, rc::Rc};

use dawn_project::analysis::ProjectAnalysis;
use dawn_project::document::{
    SequenceDocument, SequenceEffectDocument, SequenceEffectPixelDocument,
    SequenceEffectScriptDocument, SequenceLaneDocument,
};
use dawn_project::effect_script::{FixtureContext, PixelContext, RuntimeValue};
use dawn_project::model::{EffectParam, Resolved};
use floem::context::{ComputeLayoutCx, EventCx, PaintCx, UpdateCx};
use floem::event::{Event, EventPropagation};
use floem::keyboard::{Key, Modifiers, NamedKey};
use floem::kurbo::{Point, Rect, Size, Stroke};
use floem::peniko::{Brush, Color};
use floem::prelude::*;
use floem::reactive::create_effect;
use floem::style::Foreground;
use floem::text::{Attrs, AttrsList, FamilyOwned, TextLayout};
use floem::{View, ViewId};
use floem_renderer::Renderer;

use crate::actions::AppAction;
use crate::app_model::AppSnapshot;
use crate::ui::components::dropdown_menu::{DropdownMenuController, DropdownMenuEntry};
use crate::ui::components::{ui_button, ui_static_label, ui_text_input};
use crate::ui::editor::gui::EditorGuiUiState;
use crate::ui::theme;

#[derive(Debug, Clone)]
pub struct SequenceTimelineState {
    data: Rc<RefCell<SequenceTimelineStateData>>,
}

#[derive(Debug, Clone)]
struct SequenceTimelineStateData {
    pixels_per_ms: f64,
    lane_height: f64,
    scroll_x: f64,
    scroll_y: f64,
    gesture: Option<TimelineGesture>,
    raster_cache: HashMap<String, Vec<RasterCell>>,
}

impl SequenceTimelineState {
    pub fn new() -> Self {
        Self {
            data: Rc::new(RefCell::new(SequenceTimelineStateData {
                pixels_per_ms: 0.02,
                lane_height: 44.0,
                scroll_x: 0.0,
                scroll_y: 0.0,
                gesture: None,
                raster_cache: HashMap::new(),
            })),
        }
    }
}

impl Default for SequenceTimelineState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn sequence_viewer(
    state: AppSnapshot,
    gui_state: EditorGuiUiState,
    dropdown_menu: DropdownMenuController,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let Some(document) = state.active_sequence_document.clone() else {
        return empty().into_any();
    };
    let timeline_state = gui_state.sequence_timeline(&document.path, &document.object_key);
    let selected_effect = state.selected_sequence_effect;
    v_stack((
        sequence_header(&document),
        h_stack((
            SequenceTimeline::new(
                document.clone(),
                state.analysis.clone(),
                timeline_state,
                selected_effect,
                state.sequence_playhead_ms,
                dropdown_menu,
                Rc::clone(&dispatch),
            )
            .style(|s| {
                s.flex_grow(1.0)
                    .height_full()
                    .min_width(0.0)
                    .min_height(0.0)
                    .border(theme::BORDER_WIDTH)
                    .border_color(theme::color(theme::BORDER))
                    .border_radius(theme::CONTROL_RADIUS)
            }),
            sequence_inspector(document, selected_effect, dispatch),
        ))
        .style(|s| {
            s.flex_grow(1.0)
                .height_full()
                .min_width(0.0)
                .min_height(0.0)
                .gap(theme::SPACE_12)
        }),
    ))
    .style(|s| {
        s.height_full()
            .padding(theme::SPACE_12)
            .gap(theme::SPACE_8)
            .background(theme::color(theme::SURFACE))
    })
    .into_any()
}

fn sequence_header(document: &SequenceDocument) -> impl IntoView {
    let degraded = if document.degraded {
        "Fallback lanes"
    } else {
        "Layout lanes"
    };
    h_stack((
        ui_static_label("Sequence").style(|s| s.font_bold()),
        ui_static_label(format!(
            "{}  {}  {} fps",
            document.object_key,
            format_time(document.duration_ms),
            document.frame_rate
        )),
        ui_static_label(degraded).style(|s| {
            s.color(theme::color(theme::MUTED))
                .set(Foreground, Brush::Solid(theme::color(theme::MUTED)))
        }),
        empty().style(|s| s.flex_grow(1.0).min_width(0.0)),
    ))
    .style(|s| {
        s.width_full()
            .items_center()
            .gap(theme::SPACE_12)
            .padding_bottom(theme::SPACE_4)
            .border_bottom(theme::BORDER_WIDTH)
            .border_color(theme::color(theme::BORDER))
    })
}

fn sequence_inspector(
    document: SequenceDocument,
    selected_effect: Option<u32>,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let selected = selected_effect
        .and_then(|id| document.effects.iter().find(|effect| effect.id == id))
        .cloned();
    let Some(effect) = selected else {
        return container(ui_static_label("No clip selected"))
            .style(|s| {
                s.width(theme::DEFAULT_RIGHT_PANE_WIDTH)
                    .height_full()
                    .padding_left(theme::SPACE_12)
                    .border_left(theme::BORDER_WIDTH)
                    .border_color(theme::color(theme::BORDER))
                    .color(theme::color(theme::MUTED))
            })
            .into_any();
    };

    let start_signal = create_rw_signal(effect.start_ms.to_string());
    let duration_signal = create_rw_signal(effect.duration_ms.to_string());
    let start_dispatch = Rc::clone(&dispatch);
    let duration_dispatch = Rc::clone(&dispatch);
    let start_effect = effect.clone();
    let duration_effect = effect.clone();
    let duplicate_effect = effect.clone();
    let delete_effect = effect.clone();
    let start_duration = effect.duration_ms;
    let duration_start = effect.start_ms;
    let sequence_duration = document.duration_ms;
    let duplicate_dispatch = Rc::clone(&dispatch);
    let delete_dispatch = Rc::clone(&dispatch);

    v_stack((
        ui_static_label("Timing").style(|s| s.font_bold()),
        inspector_row("ID", effect.id.to_string()),
        inspector_row("Target", effect.target_label),
        inspector_row("Script", effect.script),
        numeric_timing_input("Start", start_signal, move || {
            if let Ok(value) = start_signal.get_untracked().trim().parse::<u64>() {
                let start_ms = value.min(sequence_duration.saturating_sub(1));
                start_dispatch(AppAction::ResizeSequenceEffect {
                    id: start_effect.id,
                    start_ms,
                    duration_ms: start_duration
                        .min(sequence_duration.saturating_sub(start_ms).max(1)),
                });
            }
        }),
        numeric_timing_input("Duration", duration_signal, move || {
            if let Ok(value) = duration_signal.get_untracked().trim().parse::<u64>() {
                duration_dispatch(AppAction::ResizeSequenceEffect {
                    id: duration_effect.id,
                    start_ms: duration_start,
                    duration_ms: value,
                });
            }
        }),
        h_stack((
            ui_button("Duplicate").action(move || {
                duplicate_dispatch(AppAction::DuplicateSequenceEffect {
                    id: duplicate_effect.id,
                })
            }),
            ui_button("Delete").action(move || {
                delete_dispatch(AppAction::DeleteSequenceEffect {
                    id: delete_effect.id,
                })
            }),
        ))
        .style(|s| s.width_full().gap(theme::SPACE_6)),
    ))
    .style(|s| {
        s.width(theme::DEFAULT_RIGHT_PANE_WIDTH)
            .height_full()
            .min_height(0.0)
            .padding_left(theme::SPACE_12)
            .gap(theme::SPACE_8)
            .border_left(theme::BORDER_WIDTH)
            .border_color(theme::color(theme::BORDER))
    })
    .into_any()
}

fn inspector_row(label: &'static str, value: String) -> impl IntoView {
    v_stack((
        ui_static_label(label).style(|s| {
            s.font_size(theme::FONT_SMALL)
                .color(theme::color(theme::MUTED))
                .set(Foreground, Brush::Solid(theme::color(theme::MUTED)))
        }),
        ui_static_label(value).style(|s| s.width_full().text_ellipsis()),
    ))
    .style(|s| s.width_full().gap(theme::SPACE_3))
}

fn numeric_timing_input(
    label: &'static str,
    value: RwSignal<String>,
    apply: impl Fn() + 'static,
) -> impl IntoView {
    let apply = Rc::new(apply);
    let enter_apply = Rc::clone(&apply);
    let blur_apply = Rc::clone(&apply);
    v_stack((
        ui_static_label(label).style(|s| {
            s.font_size(theme::FONT_SMALL)
                .color(theme::color(theme::MUTED))
                .set(Foreground, Brush::Solid(theme::color(theme::MUTED)))
        }),
        ui_text_input(value)
            .on_key_down(
                Key::Named(NamedKey::Enter),
                |modifiers| modifiers == Modifiers::empty(),
                move |_| enter_apply(),
            )
            .on_event_stop(floem::event::EventListener::FocusLost, move |_| {
                blur_apply()
            })
            .style(|s| s.width_full()),
    ))
    .style(|s| s.width_full().gap(theme::SPACE_3))
}

fn clip_menu_entries(id: u32, dispatch: crate::ui::UiDispatch) -> Vec<DropdownMenuEntry> {
    let duplicate = Rc::clone(&dispatch);
    let delete = Rc::clone(&dispatch);
    vec![
        DropdownMenuEntry::item("Duplicate", true, move || {
            duplicate(AppAction::DuplicateSequenceEffect { id });
        }),
        DropdownMenuEntry::item("Delete", true, move || {
            delete(AppAction::DeleteSequenceEffect { id });
        }),
    ]
}

fn add_effect_menu_entries(
    scripts: Vec<SequenceEffectScriptDocument>,
    target: dawn_project::document::LayoutTargetDocument,
    start_ms: u64,
    dispatch: crate::ui::UiDispatch,
) -> Vec<DropdownMenuEntry> {
    if scripts.is_empty() {
        return vec![DropdownMenuEntry::item("No compiled effects", false, || {})];
    }
    scripts
        .into_iter()
        .map(|script| {
            let dispatch = Rc::clone(&dispatch);
            let target = target.clone();
            let label = format!("Add Effect: {}", script.name);
            DropdownMenuEntry::item(&label, true, move || {
                dispatch(AppAction::AddSequenceEffect {
                    script_path: script.path.clone(),
                    target: target.clone(),
                    start_ms,
                });
            })
        })
        .collect()
}

#[derive(Clone)]
struct SequenceTimelineUpdate {
    document: SequenceDocument,
    analysis: Option<ProjectAnalysis>,
    selected_effect: Option<u32>,
    playhead_ms: u64,
}

struct SequenceTimeline {
    id: ViewId,
    document: SequenceDocument,
    analysis: Option<ProjectAnalysis>,
    selected_effect: Option<u32>,
    playhead_ms: u64,
    state: SequenceTimelineState,
    viewport: TimelineViewport,
    hovered_effect: Option<usize>,
    dropdown_menu: DropdownMenuController,
    dispatch: crate::ui::UiDispatch,
}

impl SequenceTimeline {
    fn new(
        document: SequenceDocument,
        analysis: Option<ProjectAnalysis>,
        state: SequenceTimelineState,
        selected_effect: Option<u32>,
        playhead_ms: u64,
        dropdown_menu: DropdownMenuController,
        dispatch: crate::ui::UiDispatch,
    ) -> Self {
        let id = ViewId::new();
        let update_document = document.clone();
        let update_analysis = analysis.clone();
        create_effect(move |_| {
            id.update_state(SequenceTimelineUpdate {
                document: update_document.clone(),
                analysis: update_analysis.clone(),
                selected_effect,
                playhead_ms,
            });
        });
        Self {
            id,
            document,
            analysis,
            selected_effect,
            playhead_ms,
            state,
            viewport: TimelineViewport::default(),
            hovered_effect: None,
            dropdown_menu,
            dispatch,
        }
        .keyboard_navigable()
    }
}

impl View for SequenceTimeline {
    fn id(&self) -> ViewId {
        self.id
    }

    fn update(&mut self, _cx: &mut UpdateCx, state: Box<dyn std::any::Any>) {
        if let Ok(update) = state.downcast::<SequenceTimelineUpdate>() {
            self.document = update.document;
            self.analysis = update.analysis;
            self.selected_effect = update.selected_effect;
            self.playhead_ms = update.playhead_ms;
            self.state.data.borrow_mut().raster_cache.clear();
            self.id.request_layout();
            self.id.request_paint();
        }
    }

    fn compute_layout(&mut self, _cx: &mut ComputeLayoutCx) -> Option<Rect> {
        let layout = self.id.get_layout().unwrap_or_default();
        self.viewport.size = Size::new(layout.size.width as f64, layout.size.height as f64);
        clamp_timeline_state(&self.state, &self.document, self.viewport.size);
        None
    }

    fn event_before_children(&mut self, cx: &mut EventCx, event: &Event) -> EventPropagation {
        match event {
            Event::PointerWheel(event) => {
                self.handle_wheel(event.delta.x, event.delta.y, event.modifiers)
            }
            Event::PointerDown(event) => {
                if event.button.is_primary() {
                    cx.update_active(self.id);
                    self.id.request_active();
                    self.id.request_focus();
                    self.handle_pointer_down(event.pos)
                } else if event.button.is_secondary() {
                    self.handle_secondary_click(event.pos)
                } else {
                    EventPropagation::Continue
                }
            }
            Event::PointerMove(event) => self.handle_pointer_move(event.pos),
            Event::PointerUp(event) => {
                if event.button.is_primary() {
                    self.id.clear_active();
                    self.handle_pointer_up(event.pos)
                } else {
                    EventPropagation::Continue
                }
            }
            Event::PointerLeave | Event::FocusLost => {
                self.state.data.borrow_mut().gesture = None;
                self.hovered_effect = None;
                self.id.clear_active();
                self.id.request_paint();
                EventPropagation::Continue
            }
            Event::KeyDown(event) => self.handle_key_down(&event.key.logical_key, event.modifiers),
            _ => EventPropagation::Continue,
        }
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let size = self.viewport.size;
        if size.width <= 0.0 || size.height <= 0.0 {
            return;
        }
        let panel = size.to_rect();
        cx.fill(&panel, &Brush::Solid(theme::color(theme::PANEL_DARK)), 0.0);
        cx.clip(&panel);
        self.paint_ruler(cx);
        self.paint_lanes(cx);
        self.paint_clips(cx);
        self.paint_drag_preview(cx);
        self.paint_playhead(cx);
    }
}

impl SequenceTimeline {
    fn handle_wheel(
        &mut self,
        delta_x: f64,
        delta_y: f64,
        modifiers: Modifiers,
    ) -> EventPropagation {
        {
            let mut state = self.state.data.borrow_mut();
            if modifiers.control() && modifiers.shift() {
                let factor = (-delta_y * 0.0025).exp();
                state.lane_height = (state.lane_height * factor).clamp(28.0, 96.0);
            } else if modifiers.control() {
                let factor = (-delta_y * 0.0025).exp();
                state.pixels_per_ms = (state.pixels_per_ms * factor).clamp(0.002, 1.0);
            } else if modifiers.shift() {
                state.scroll_x += delta_y + delta_x;
            } else {
                state.scroll_y += delta_y;
                state.scroll_x += delta_x;
            }
        }
        clamp_timeline_state(&self.state, &self.document, self.viewport.size);
        self.id.request_paint();
        EventPropagation::Stop
    }

    fn handle_pointer_down(&mut self, position: Point) -> EventPropagation {
        if position.y < RULER_HEIGHT && position.x >= LANE_LABEL_WIDTH {
            let time_ms = self.viewport.screen_to_time_ms(position.x, &self.state);
            (self.dispatch)(AppAction::SetSequencePlayhead {
                time_ms: time_ms.min(self.document.duration_ms),
            });
            return EventPropagation::Stop;
        }
        if let Some(hit) = self.hit_test(position) {
            (self.dispatch)(AppAction::SelectSequenceEffect {
                id: Some(hit.effect.id),
            });
            self.state.data.borrow_mut().gesture = Some(TimelineGesture {
                effect: hit.effect,
                kind: hit.kind,
                start_screen: position,
                current_screen: position,
            });
        } else {
            (self.dispatch)(AppAction::SelectSequenceEffect { id: None });
            self.state.data.borrow_mut().gesture = None;
        }
        self.id.request_paint();
        EventPropagation::Stop
    }

    fn handle_pointer_move(&mut self, position: Point) -> EventPropagation {
        if let Some(gesture) = &mut self.state.data.borrow_mut().gesture {
            gesture.current_screen = position;
            self.id.request_paint();
            return EventPropagation::Stop;
        }
        let hovered_effect = self.hit_test(position).map(|hit| hit.effect.index);
        if self.hovered_effect != hovered_effect {
            self.hovered_effect = hovered_effect;
            self.id.request_paint();
        }
        EventPropagation::Continue
    }

    fn handle_pointer_up(&mut self, position: Point) -> EventPropagation {
        let Some(gesture) = self.state.data.borrow_mut().gesture.take() else {
            return EventPropagation::Continue;
        };
        let delta_ms = ((position.x - gesture.start_screen.x)
            / self.state.data.borrow().pixels_per_ms)
            .round() as i64;
        let original_start = gesture.effect.start_ms as i64;
        let original_duration = gesture.effect.duration_ms as i64;
        match gesture.kind {
            HitKind::Body => {
                let max_start =
                    self.document
                        .duration_ms
                        .saturating_sub(gesture.effect.duration_ms) as i64;
                let start_ms = (original_start + delta_ms).clamp(0, max_start) as u64;
                let target = self
                    .lane_at_position(position)
                    .map(|lane| lane.target)
                    .filter(|target| *target != gesture.effect.target);
                (self.dispatch)(AppAction::MoveSequenceEffect {
                    id: gesture.effect.id,
                    start_ms,
                    target,
                });
            }
            HitKind::LeftEdge => {
                let end = original_start + original_duration;
                let start_ms = (original_start + delta_ms).clamp(0, end - 1) as u64;
                (self.dispatch)(AppAction::ResizeSequenceEffect {
                    id: gesture.effect.id,
                    start_ms,
                    duration_ms: (end as u64).saturating_sub(start_ms).max(1),
                });
            }
            HitKind::RightEdge => {
                let duration_ms = (original_duration + delta_ms)
                    .clamp(1, self.document.duration_ms as i64 - original_start)
                    as u64;
                (self.dispatch)(AppAction::ResizeSequenceEffect {
                    id: gesture.effect.id,
                    start_ms: gesture.effect.start_ms,
                    duration_ms,
                });
            }
        }
        self.id.request_paint();
        EventPropagation::Stop
    }

    fn handle_secondary_click(&mut self, position: Point) -> EventPropagation {
        let entries = if let Some(hit) = self.hit_test(position) {
            (self.dispatch)(AppAction::SelectSequenceEffect {
                id: Some(hit.effect.id),
            });
            clip_menu_entries(hit.effect.id, Rc::clone(&self.dispatch))
        } else if position.y >= RULER_HEIGHT {
            let Some(lane) = self.lane_at_position_exact(position) else {
                return EventPropagation::Stop;
            };
            let start_ms = self
                .viewport
                .screen_to_time_ms(position.x, &self.state)
                .min(self.document.duration_ms);
            add_effect_menu_entries(
                self.document.effect_scripts.clone(),
                lane.target,
                start_ms,
                Rc::clone(&self.dispatch),
            )
        } else {
            Vec::new()
        };
        if !entries.is_empty() {
            self.dropdown_menu
                .open_at_view_point(self.id, position, entries);
        }
        EventPropagation::Stop
    }

    fn handle_key_down(&mut self, key: &Key, modifiers: Modifiers) -> EventPropagation {
        if let Some(action) =
            sequence_key_action(&self.document, self.selected_effect, key, modifiers)
        {
            (self.dispatch)(action);
            EventPropagation::Stop
        } else {
            EventPropagation::Continue
        }
    }

    fn lane_at_position(&self, position: Point) -> Option<SequenceLaneDocument> {
        if self.document.lanes.is_empty() {
            return None;
        }
        let state = self.state.data.borrow();
        let lane = ((position.y - RULER_HEIGHT + state.scroll_y) / state.lane_height).floor();
        let index = (lane as isize).clamp(0, self.document.lanes.len() as isize - 1) as usize;
        self.document.lanes.get(index).cloned()
    }

    fn lane_at_position_exact(&self, position: Point) -> Option<SequenceLaneDocument> {
        if self.document.lanes.is_empty() {
            return None;
        }
        let state = self.state.data.borrow();
        let lane = ((position.y - RULER_HEIGHT + state.scroll_y) / state.lane_height).floor();
        if !(0.0..self.document.lanes.len() as f64).contains(&lane) {
            return None;
        }
        self.document.lanes.get(lane as usize).cloned()
    }

    fn paint_ruler(&self, cx: &mut PaintCx) {
        let state = self.state.data.borrow();
        let ruler = Rect::new(0.0, 0.0, self.viewport.size.width, RULER_HEIGHT);
        cx.fill(&ruler, &Brush::Solid(theme::color(theme::PANEL)), 0.0);
        cx.stroke(
            &ruler,
            &Brush::Solid(theme::color(theme::BORDER)),
            &Stroke::new(1.0),
        );
        let step = ruler_step_ms(state.pixels_per_ms);
        let first = (state.scroll_x / state.pixels_per_ms / step as f64).floor() as u64 * step;
        let mut time = first;
        while time <= self.document.duration_ms + step {
            let x = LANE_LABEL_WIDTH + time as f64 * state.pixels_per_ms - state.scroll_x;
            if x >= LANE_LABEL_WIDTH && x <= self.viewport.size.width {
                cx.stroke(
                    &floem::kurbo::Line::new(Point::new(x, 18.0), Point::new(x, RULER_HEIGHT)),
                    &Brush::Solid(theme::color(theme::BORDER)),
                    &Stroke::new(1.0),
                );
                draw_text(
                    cx,
                    &format_time(time),
                    x + 4.0,
                    6.0,
                    theme::color(theme::MUTED),
                );
            }
            time = time.saturating_add(step);
        }
    }

    fn paint_lanes(&self, cx: &mut PaintCx) {
        let state = self.state.data.borrow();
        for (index, lane) in self.document.lanes.iter().enumerate() {
            let y = RULER_HEIGHT + index as f64 * state.lane_height - state.scroll_y;
            if y + state.lane_height < RULER_HEIGHT || y > self.viewport.size.height {
                continue;
            }
            let row = Rect::new(0.0, y, self.viewport.size.width, y + state.lane_height);
            let bg = if index % 2 == 0 {
                theme::color(theme::SURFACE)
            } else {
                theme::color(theme::PANEL_DARK)
            };
            cx.fill(&row, &Brush::Solid(bg), 0.0);
            cx.stroke(
                &row,
                &Brush::Solid(theme::color(theme::BORDER)),
                &Stroke::new(1.0),
            );
            draw_text(cx, &lane.label, 8.0, y + 12.0, theme::color(theme::TEXT));
        }
        let divider = Rect::new(
            LANE_LABEL_WIDTH,
            RULER_HEIGHT,
            LANE_LABEL_WIDTH + 1.0,
            self.viewport.size.height,
        );
        cx.fill(&divider, &Brush::Solid(theme::color(theme::BORDER)), 0.0);
    }

    fn paint_clips(&self, cx: &mut PaintCx) {
        for layout in clip_layouts(&self.document, &self.state) {
            if layout.rect.y1 < RULER_HEIGHT || layout.rect.y0 > self.viewport.size.height {
                continue;
            }
            if layout.rect.x1 < LANE_LABEL_WIDTH || layout.rect.x0 > self.viewport.size.width {
                continue;
            }
            let Some(effect) = self
                .document
                .effects
                .iter()
                .find(|effect| effect.index == layout.effect_index)
            else {
                continue;
            };
            let selected = self.selected_effect == Some(effect.id);
            let border_color = if selected {
                SELECTED_CLIP_BORDER
            } else if self.hovered_effect == Some(effect.index) {
                HOVERED_CLIP_BORDER
            } else {
                theme::color(theme::BORDER)
            };
            let border_width = if selected { 2.0 } else { 1.0 };
            cx.fill(&layout.rect, &Brush::Solid(clip_color(effect.index)), 0.0);
            self.paint_clip_raster(cx, effect, layout.rect);
            if selected {
                cx.fill(&layout.rect, &Brush::Solid(SELECTED_CLIP_HIGHLIGHT), 0.0);
            }
            cx.stroke(
                &stroke_rect(layout.rect, border_width),
                &Brush::Solid(border_color),
                &Stroke::new(border_width),
            );
            draw_text(
                cx,
                &effect.id.to_string(),
                layout.rect.x0 + 6.0,
                layout.rect.y0 + 5.0,
                theme::color(theme::TEXT_INVERTED),
            );
        }
    }

    fn paint_clip_raster(&self, cx: &mut PaintCx, effect: &SequenceEffectDocument, rect: Rect) {
        let Some(render) = &effect.render else {
            return;
        };
        let Some(analysis) = &self.analysis else {
            return;
        };
        if render.target_pixels.is_empty() || effect.duration_ms == 0 {
            return;
        }

        let (pixels_per_ms, visible_x0, visible_x1) = {
            let state = self.state.data.borrow();
            (
                state.pixels_per_ms,
                rect.x0.max(LANE_LABEL_WIDTH).floor() as i64,
                rect.x1.min(self.viewport.size.width).ceil() as i64,
            )
        };
        if visible_x1 <= visible_x0 {
            return;
        }

        let row_bins = raster_row_bins(render.target_pixels.len(), rect.height());
        let key = raster_cache_key(effect, visible_x0, visible_x1, row_bins);
        let cells = {
            let mut state = self.state.data.borrow_mut();
            if let Some(cells) = state.raster_cache.get(&key) {
                cells.clone()
            } else {
                let cells = build_raster_cells(
                    analysis,
                    effect,
                    visible_x0,
                    visible_x1,
                    row_bins,
                    pixels_per_ms,
                    rect,
                );
                state.raster_cache.insert(key, cells.clone());
                cells
            }
        };

        for cell in cells {
            cx.fill(&cell.rect, &Brush::Solid(cell.color), 0.0);
        }
    }

    fn paint_drag_preview(&self, cx: &mut PaintCx) {
        let Some(preview) = self.drag_preview_rect() else {
            return;
        };
        if preview.rect.y1 < RULER_HEIGHT || preview.rect.y0 > self.viewport.size.height {
            return;
        }
        if preview.rect.x1 < LANE_LABEL_WIDTH || preview.rect.x0 > self.viewport.size.width {
            return;
        }

        cx.fill(
            &preview.rect,
            &Brush::Solid(Color::rgba8(170, 178, 188, 126)),
            0.0,
        );
        cx.stroke(
            &stroke_rect(preview.rect, 1.5),
            &Brush::Solid(Color::rgba8(230, 238, 246, 210)),
            &Stroke::new(1.5),
        );
        draw_text(
            cx,
            &preview.effect_id.to_string(),
            preview.rect.x0 + 6.0,
            preview.rect.y0 + 5.0,
            Color::rgba8(255, 255, 255, 220),
        );
    }

    fn drag_preview_rect(&self) -> Option<DragPreview> {
        let gesture = self.state.data.borrow().gesture.clone()?;
        let pixels_per_ms = self.state.data.borrow().pixels_per_ms;
        let delta_ms =
            ((gesture.current_screen.x - gesture.start_screen.x) / pixels_per_ms).round() as i64;
        let original_start = gesture.effect.start_ms as i64;
        let original_duration = gesture.effect.duration_ms as i64;

        let preview_effect = match gesture.kind {
            HitKind::Body => {
                let max_start =
                    self.document
                        .duration_ms
                        .saturating_sub(gesture.effect.duration_ms) as i64;
                let start_ms = (original_start + delta_ms).clamp(0, max_start) as u64;
                let target = self
                    .lane_at_position(gesture.current_screen)
                    .map(|lane| lane.target)
                    .unwrap_or_else(|| gesture.effect.target.clone());
                let mut effect = gesture.effect.clone();
                effect.start_ms = start_ms;
                effect.target = target;
                effect
            }
            HitKind::LeftEdge => {
                let end = original_start + original_duration;
                let start_ms = (original_start + delta_ms).clamp(0, end - 1) as u64;
                let mut effect = gesture.effect.clone();
                effect.start_ms = start_ms;
                effect.duration_ms = (end as u64).saturating_sub(start_ms).max(1);
                effect
            }
            HitKind::RightEdge => {
                let duration_ms = (original_duration + delta_ms)
                    .clamp(1, self.document.duration_ms as i64 - original_start)
                    as u64;
                let mut effect = gesture.effect.clone();
                effect.duration_ms = duration_ms;
                effect
            }
        };

        let lane_index = self
            .document
            .lanes
            .iter()
            .position(|lane| lane.target == preview_effect.target)?;
        let state = self.state.data.borrow();
        let rect = clip_rect(&preview_effect, lane_index, 0, 1, &state);
        Some(DragPreview {
            effect_id: preview_effect.id,
            rect,
        })
    }

    fn paint_playhead(&self, cx: &mut PaintCx) {
        let state = self.state.data.borrow();
        let x = LANE_LABEL_WIDTH + self.playhead_ms as f64 * state.pixels_per_ms - state.scroll_x;
        if x < LANE_LABEL_WIDTH || x > self.viewport.size.width {
            return;
        }
        cx.stroke(
            &floem::kurbo::Line::new(Point::new(x, 0.0), Point::new(x, self.viewport.size.height)),
            &Brush::Solid(Color::rgb8(255, 202, 97)),
            &Stroke::new(2.0),
        );
    }

    fn hit_test(&self, position: Point) -> Option<TimelineHit> {
        clip_layouts(&self.document, &self.state)
            .into_iter()
            .rev()
            .find_map(|layout| {
                if !layout.rect.inflate(0.0, 3.0).contains(position) {
                    return None;
                }
                let effect = self
                    .document
                    .effects
                    .iter()
                    .find(|effect| effect.index == layout.effect_index)?;
                let kind = if position.x - layout.rect.x0 <= EDGE_HIT_WIDTH {
                    HitKind::LeftEdge
                } else if layout.rect.x1 - position.x <= EDGE_HIT_WIDTH {
                    HitKind::RightEdge
                } else {
                    HitKind::Body
                };
                Some(TimelineHit {
                    effect: effect.clone(),
                    kind,
                })
            })
    }
}

pub fn sequence_key_action(
    document: &SequenceDocument,
    selected_effect: Option<u32>,
    key: &Key,
    modifiers: Modifiers,
) -> Option<AppAction> {
    let id = selected_effect?;
    let effect = document.effects.iter().find(|effect| effect.id == id)?;

    match key {
        Key::Named(NamedKey::Delete) | Key::Named(NamedKey::Backspace)
            if !modifiers.control() && !modifiers.alt() && !modifiers.meta() =>
        {
            Some(AppAction::DeleteSequenceEffect { id })
        }
        Key::Character(value)
            if modifiers.control()
                && !modifiers.shift()
                && !modifiers.alt()
                && !modifiers.meta()
                && value.eq_ignore_ascii_case("d") =>
        {
            Some(AppAction::DuplicateSequenceEffect { id })
        }
        Key::Named(NamedKey::ArrowLeft) | Key::Named(NamedKey::ArrowRight)
            if !modifiers.control() && !modifiers.alt() && !modifiers.meta() =>
        {
            let delta = if modifiers.shift() { 100 } else { 1 };
            let start_ms = if matches!(key, Key::Named(NamedKey::ArrowLeft)) {
                effect.start_ms.saturating_sub(delta)
            } else {
                effect.start_ms.saturating_add(delta)
            };
            Some(AppAction::MoveSequenceEffect {
                id,
                start_ms,
                target: None,
            })
        }
        Key::Named(NamedKey::ArrowUp) | Key::Named(NamedKey::ArrowDown)
            if !modifiers.control() && !modifiers.alt() && !modifiers.meta() =>
        {
            let current = document
                .lanes
                .iter()
                .position(|lane| lane.target == effect.target)?;
            let step = if modifiers.shift() { 5 } else { 1 };
            let next = if matches!(key, Key::Named(NamedKey::ArrowUp)) {
                current.saturating_sub(step)
            } else {
                (current + step).min(document.lanes.len().saturating_sub(1))
            };
            let lane = document.lanes.get(next)?;
            Some(AppAction::RetargetSequenceEffect {
                id,
                target: lane.target.clone(),
            })
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct ClipLayout {
    effect_index: usize,
    rect: Rect,
}

#[derive(Debug, Clone, Copy, Default)]
struct TimelineViewport {
    size: Size,
}

impl TimelineViewport {
    fn screen_to_time_ms(&self, x: f64, state: &SequenceTimelineState) -> u64 {
        let state = state.data.borrow();
        ((x - LANE_LABEL_WIDTH + state.scroll_x) / state.pixels_per_ms)
            .max(0.0)
            .round() as u64
    }
}

#[derive(Debug, Clone)]
struct TimelineGesture {
    effect: SequenceEffectDocument,
    kind: HitKind,
    start_screen: Point,
    current_screen: Point,
}

#[derive(Debug, Clone)]
struct TimelineHit {
    effect: SequenceEffectDocument,
    kind: HitKind,
}

#[derive(Debug, Clone)]
struct RasterCell {
    rect: Rect,
    color: Color,
}

#[derive(Debug, Clone, Copy)]
struct DragPreview {
    effect_id: u32,
    rect: Rect,
}

#[derive(Debug, Clone, Copy)]
enum HitKind {
    Body,
    LeftEdge,
    RightEdge,
}

const LANE_LABEL_WIDTH: f64 = 168.0;
const RULER_HEIGHT: f64 = 32.0;
const CLIP_VERTICAL_INSET: f64 = 1.0;
const MIN_CLIP_WIDTH: f64 = 6.0;
const EDGE_HIT_WIDTH: f64 = 6.0;
const HOVERED_CLIP_BORDER: Color = Color::rgb8(112, 112, 112);
const SELECTED_CLIP_BORDER: Color = Color::rgb8(232, 236, 240);
const SELECTED_CLIP_HIGHLIGHT: Color = Color::rgba8(255, 255, 255, 28);
fn clamp_timeline_state(state: &SequenceTimelineState, document: &SequenceDocument, size: Size) {
    let mut state = state.data.borrow_mut();
    let content_width = document.duration_ms as f64 * state.pixels_per_ms;
    let content_height = document.lanes.len() as f64 * state.lane_height;
    state.scroll_x = state.scroll_x.clamp(
        0.0,
        (content_width - (size.width - LANE_LABEL_WIDTH)).max(0.0),
    );
    state.scroll_y = state.scroll_y.clamp(
        0.0,
        (content_height - (size.height - RULER_HEIGHT)).max(0.0),
    );
}

fn clip_layouts(document: &SequenceDocument, state: &SequenceTimelineState) -> Vec<ClipLayout> {
    let state = state.data.borrow();
    let mut layouts = Vec::with_capacity(document.effects.len());
    for (lane_index, lane) in document.lanes.iter().enumerate() {
        let mut lane_effects = document
            .effects
            .iter()
            .filter(|effect| effect.target == lane.target)
            .collect::<Vec<_>>();
        lane_effects.sort_by_key(|effect| (effect.start_ms, effect.index));

        let mut group_start = 0;
        while group_start < lane_effects.len() {
            let mut group_end = group_start + 1;
            let mut group_end_ms =
                lane_effects[group_start].start_ms + lane_effects[group_start].duration_ms;
            while group_end < lane_effects.len() && lane_effects[group_end].start_ms < group_end_ms
            {
                group_end_ms = group_end_ms
                    .max(lane_effects[group_end].start_ms + lane_effects[group_end].duration_ms);
                group_end += 1;
            }

            let group = &lane_effects[group_start..group_end];
            let stack_indices = clip_stack_indices(group);
            let stack_count = stack_indices.iter().copied().max().unwrap_or(0) + 1;
            for (effect, stack_index) in group.iter().zip(stack_indices) {
                layouts.push(ClipLayout {
                    effect_index: effect.index,
                    rect: clip_rect(effect, lane_index, stack_index, stack_count, &state),
                });
            }
            group_start = group_end;
        }
    }
    layouts
}

fn clip_stack_indices(group: &[&SequenceEffectDocument]) -> Vec<usize> {
    let mut stack_ends: Vec<u64> = Vec::new();
    let mut indices = Vec::with_capacity(group.len());
    for effect in group {
        let stack_index = stack_ends
            .iter()
            .position(|end| *end <= effect.start_ms)
            .unwrap_or(stack_ends.len());
        let end_ms = effect.start_ms + effect.duration_ms;
        if stack_index == stack_ends.len() {
            stack_ends.push(end_ms);
        } else {
            stack_ends[stack_index] = end_ms;
        }
        indices.push(stack_index);
    }
    indices
}

fn clip_rect(
    effect: &SequenceEffectDocument,
    lane_index: usize,
    stack_index: usize,
    stack_count: usize,
    state: &SequenceTimelineStateData,
) -> Rect {
    let lane_y = RULER_HEIGHT + lane_index as f64 * state.lane_height - state.scroll_y;
    let body_height = (state.lane_height - CLIP_VERTICAL_INSET * 2.0).max(1.0);
    let stack_height = body_height / stack_count.max(1) as f64;
    let y0 = lane_y + CLIP_VERTICAL_INSET + stack_index as f64 * stack_height;
    let y1 = if stack_index + 1 == stack_count {
        lane_y + state.lane_height - CLIP_VERTICAL_INSET
    } else {
        y0 + stack_height
    };
    let x0 = LANE_LABEL_WIDTH + effect.start_ms as f64 * state.pixels_per_ms - state.scroll_x;
    let width = (effect.duration_ms as f64 * state.pixels_per_ms).max(MIN_CLIP_WIDTH);
    Rect::new(x0, y0, x0 + width, y1)
}

fn stroke_rect(rect: Rect, width: f64) -> Rect {
    rect.inflate(-width / 2.0, -width / 2.0)
}

fn raster_row_bins(pixel_count: usize, height: f64) -> usize {
    if pixel_count == 0 {
        return 0;
    }
    if height / pixel_count as f64 >= 2.0 {
        pixel_count
    } else {
        ((height / 2.0).floor() as usize).clamp(1, pixel_count)
    }
}

fn raster_cache_key(
    effect: &SequenceEffectDocument,
    visible_x0: i64,
    visible_x1: i64,
    row_bins: usize,
) -> String {
    let render = effect.render.as_ref().expect("render metadata is present");
    format!(
        "{}:{}:{}:{}:{}:{}:{}:{:?}:{:?}:{:?}",
        effect.index,
        effect.start_ms,
        effect.duration_ms,
        visible_x0,
        visible_x1,
        row_bins,
        render.script_key,
        render.script_source,
        render.params,
        render.target_pixels,
    )
}

fn build_raster_cells(
    analysis: &ProjectAnalysis,
    effect: &SequenceEffectDocument,
    visible_x0: i64,
    visible_x1: i64,
    row_bins: usize,
    pixels_per_ms: f64,
    rect: Rect,
) -> Vec<RasterCell> {
    let Some(render) = &effect.render else {
        return Vec::new();
    };
    let params = runtime_params_from_document(&render.params);
    let row_height = rect.height() / row_bins.max(1) as f64;
    let mut cells = Vec::with_capacity((visible_x1 - visible_x0).max(0) as usize * row_bins.max(1));
    for column in visible_x0..visible_x1 {
        let x0 = (column as f64).max(rect.x0);
        let x1 = ((column + 1) as f64).min(rect.x1);
        if x1 <= x0 {
            continue;
        }
        let local_ms =
            ((column as f64 + 0.5 - rect.x0) / pixels_per_ms).clamp(0.0, effect.duration_ms as f64);
        let progress = (local_ms / effect.duration_ms as f64).clamp(0.0, 1.0);
        let seconds = local_ms / 1_000.0;
        for row in 0..row_bins {
            let pixel = sampled_pixel(&render.target_pixels, row, row_bins);
            let color = analysis
                .sample_effect_script_key(
                    &render.script_key,
                    progress,
                    seconds,
                    FixtureContext {
                        index: pixel.fixture_index,
                    },
                    PixelContext {
                        index: pixel.pixel_index,
                    },
                    params.clone(),
                )
                .map(color_to_peniko)
                .unwrap_or_else(|_| sample_error_color());
            let y0 = rect.y0 + row as f64 * row_height;
            let y1 = if row + 1 == row_bins {
                rect.y1
            } else {
                rect.y0 + (row + 1) as f64 * row_height
            };
            cells.push(RasterCell {
                rect: Rect::new(x0, y0, x1, y1),
                color,
            });
        }
    }
    cells
}

fn sampled_pixel(
    pixels: &[SequenceEffectPixelDocument],
    row: usize,
    row_bins: usize,
) -> &SequenceEffectPixelDocument {
    let index = (((row as f64 + 0.5) * pixels.len() as f64) / row_bins.max(1) as f64)
        .floor()
        .clamp(0.0, pixels.len().saturating_sub(1) as f64) as usize;
    &pixels[index]
}

fn runtime_params_from_document(
    params: &[dawn_project::document::SequenceEffectParamDocument],
) -> std::collections::BTreeMap<String, RuntimeValue> {
    params
        .iter()
        .filter_map(|param| {
            runtime_value_from_param(&param.value).map(|value| (param.name.clone(), value))
        })
        .collect()
}

fn runtime_value_from_param(param: &EffectParam<Resolved>) -> Option<RuntimeValue> {
    match param {
        EffectParam::Integer { value } => Some(RuntimeValue::Int(*value as i64)),
        EffectParam::Float { value } => Some(RuntimeValue::Float(*value)),
        EffectParam::Boolean { value } => Some(RuntimeValue::Bool(*value)),
        EffectParam::Enum { value } => Some(RuntimeValue::Enum(value.clone())),
        EffectParam::Flags { value } => Some(RuntimeValue::Flags(value.clone())),
        EffectParam::Color { value } => Some(RuntimeValue::Color(*value)),
        EffectParam::Curve { curve } => Some(RuntimeValue::Curve(curve.clone())),
    }
}

fn color_to_peniko(color: dawn_project::model::Color) -> Color {
    Color::rgb8(color.red, color.green, color.blue)
}

fn sample_error_color() -> Color {
    Color::rgb8(255, 64, 64)
}

fn ruler_step_ms(pixels_per_ms: f64) -> u64 {
    for step in [100, 250, 500, 1_000, 2_000, 5_000, 10_000, 30_000, 60_000] {
        if step as f64 * pixels_per_ms >= 72.0 {
            return step;
        }
    }
    120_000
}

fn format_time(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms % 1_000 == 0 {
        format!("{}s", ms / 1_000)
    } else {
        format!("{:.2}s", ms as f64 / 1_000.0)
    }
}

fn clip_color(index: usize) -> Color {
    const PALETTE: [(u8, u8, u8); 6] = [
        (74, 144, 226),
        (80, 190, 135),
        (220, 125, 90),
        (180, 128, 220),
        (70, 185, 190),
        (210, 170, 70),
    ];
    let (r, g, b) = PALETTE[index % PALETTE.len()];
    Color::rgb8(r, g, b)
}

fn draw_text(cx: &mut PaintCx, text: &str, x: f64, y: f64, color: Color) {
    let mut layout = TextLayout::new();
    let family: Vec<FamilyOwned> = FamilyOwned::parse_list(theme::APP_FONT).collect();
    layout.set_text(
        text,
        AttrsList::new(
            Attrs::new()
                .family(&family)
                .font_size(theme::FONT_SMALL as f32)
                .color(color),
        ),
    );
    cx.draw_text(&layout, Point::new(x, y));
}
