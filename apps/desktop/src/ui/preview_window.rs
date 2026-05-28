use std::cell::RefCell;
use std::rc::Rc;

use floem::context::{ComputeLayoutCx, EventCx, PaintCx, UpdateCx};
use floem::event::{Event, EventListener};
use floem::kurbo::{Circle, Rect, Size};
use floem::peniko::{Brush, Color};
use floem::prelude::*;
use floem::reactive::create_effect;
use floem::style::Foreground;
use floem::window::WindowId;
use floem::{View, ViewId};
use floem_renderer::Renderer;

use crate::actions::AppAction;
use crate::layout_persistence::PreviewWindowLayout;
use crate::output_runtime::{
    InProcessOutputRuntime, OutputFrame, OutputPixelFrame, OutputPreviewSource,
};
use crate::ui::components::ui_label;
use crate::ui::theme;

pub fn preview_window_view(
    window_id: WindowId,
    snapshot: crate::ui::UiSnapshot,
    playback_clock: crate::ui::UiPlaybackClock,
    dispatch: crate::ui::UiDispatch,
    preview_window_id: Rc<RefCell<Option<WindowId>>>,
) -> impl IntoView {
    *preview_window_id.borrow_mut() = Some(window_id);
    let runtime = Rc::new(RefCell::new(InProcessOutputRuntime::default()));
    let last_source = Rc::new(RefCell::new(None::<OutputPreviewSource>));
    let last_bounds = Rc::new(RefCell::new(
        snapshot.get_untracked().workbench_layout.preview_window,
    ));
    let bounds_for_move = Rc::clone(&last_bounds);
    let bounds_for_resize = Rc::clone(&last_bounds);
    let move_dispatch = Rc::clone(&dispatch);
    let resize_dispatch = Rc::clone(&dispatch);
    let close_dispatch = Rc::clone(&dispatch);
    let close_window_id = Rc::clone(&preview_window_id);

    v_stack((
        preview_content(snapshot, playback_clock, runtime, last_source).style(|s| s.size_full()),
    ))
    .on_event_stop(EventListener::WindowMoved, move |event| {
        if let Event::WindowMoved(point) = event {
            let mut bounds = bounds_for_move.borrow_mut();
            bounds.x = point.x;
            bounds.y = point.y;
            move_dispatch(bounds_action(&bounds));
        }
    })
    .on_event_stop(EventListener::WindowResized, move |event| {
        if let Event::WindowResized(size) = event {
            let mut bounds = bounds_for_resize.borrow_mut();
            bounds.width = size.width;
            bounds.height = size.height;
            resize_dispatch(bounds_action(&bounds));
        }
    })
    .on_event_stop(EventListener::WindowClosed, move |_| {
        *close_window_id.borrow_mut() = None;
        close_dispatch(AppAction::PreviewWindowClosed);
    })
    .style(theme::app_root_style)
}

fn preview_frame(
    snapshot: &crate::app_model::AppSnapshot,
    runtime: &Rc<RefCell<InProcessOutputRuntime>>,
    last_source: &Rc<RefCell<Option<OutputPreviewSource>>>,
) -> OutputFrame {
    if let Some(source) = OutputPreviewSource::from_snapshot(snapshot) {
        *last_source.borrow_mut() = Some(source.clone());
        return runtime.borrow_mut().evaluate_source(snapshot, source);
    }
    if let Some(source) = last_source.borrow().clone() {
        return runtime.borrow_mut().evaluate_source(snapshot, source);
    }
    runtime.borrow_mut().evaluate_snapshot(snapshot)
}

fn preview_content(
    snapshot: crate::ui::UiSnapshot,
    playback_clock: crate::ui::UiPlaybackClock,
    runtime: Rc<RefCell<InProcessOutputRuntime>>,
    last_source: Rc<RefCell<Option<OutputPreviewSource>>>,
) -> impl IntoView {
    v_stack((
        h_stack((
            ui_label(move || preview_label(&snapshot.get())).style(|s| {
                s.font_size(theme::FONT_SMALL)
                    .font_bold()
                    .color(theme::color(theme::TEXT))
                    .set(Foreground, Brush::Solid(theme::color(theme::TEXT)))
            }),
            ui_label(move || {
                preview_header_status(&snapshot.get_untracked(), &playback_clock.get())
            })
            .style(|s| {
                s.font_size(theme::FONT_SMALL)
                    .color(theme::color(theme::MUTED))
                    .set(Foreground, Brush::Solid(theme::color(theme::MUTED)))
            }),
        ))
        .style(|s| {
            s.height(theme::TOOLBAR_HEIGHT)
                .width_full()
                .items_center()
                .justify_between()
                .padding_horiz(theme::SPACE_10)
                .background(theme::color(theme::PANEL_DARK))
        }),
        PreviewCanvas::new(snapshot, playback_clock, runtime, last_source)
            .style(|s| s.flex_grow(1.0).min_height(0.0)),
    ))
    .style(|s| s.width_full().height_full().background(Color::BLACK))
}

fn preview_label(snapshot: &crate::app_model::AppSnapshot) -> String {
    if let Some(document) = snapshot.active_sequence_document.as_ref() {
        if snapshot.solo_selected_clip {
            return snapshot
                .selected_sequence_effect
                .and_then(|id| document.effects.iter().find(|effect| effect.id == id))
                .map(|effect| format!("Solo clip {}  {}", effect.id, effect.script))
                .unwrap_or_else(|| "Solo selected clip".to_string());
        }
        return format!("Sequence {}", document.object_key);
    }
    if let Some(path) = snapshot.active_file.as_ref().filter(|path| {
        path.file_name()
            .is_some_and(|name| name.ends_with(".effect.dawn"))
    }) {
        return path
            .file_name()
            .map(str::to_string)
            .unwrap_or_else(|| "Effect script".to_string());
    }
    "No preview source".to_string()
}

fn preview_header_status(
    snapshot: &crate::app_model::AppSnapshot,
    clock: &crate::app_model::PlaybackClock,
) -> String {
    let time_ms = if snapshot.active_sequence_document.is_some() {
        clock.sequence_playhead_ms
    } else {
        clock.time_ms
    };
    let duration_ms = snapshot
        .active_sequence_document
        .as_ref()
        .map(|document| document.duration_ms)
        .unwrap_or(crate::output_runtime::EFFECT_PREVIEW_LOOP_MS);
    let mode = if snapshot.active_sequence_document.is_some() {
        if snapshot.solo_selected_clip {
            "Solo"
        } else {
            "Sequence"
        }
    } else if snapshot
        .active_file
        .as_ref()
        .and_then(|path| path.file_name())
        .is_some_and(|name| name.ends_with(".effect.dawn"))
    {
        "Effect"
    } else {
        "Idle"
    };
    format!("{mode}  {time_ms} / {duration_ms} ms")
}

enum PreviewCanvasUpdate {
    Frame(OutputFrame),
}

struct PreviewCanvas {
    id: ViewId,
    frame: OutputFrame,
    viewport_size: Size,
}

impl PreviewCanvas {
    fn new(
        snapshot: crate::ui::UiSnapshot,
        playback_clock: crate::ui::UiPlaybackClock,
        runtime: Rc<RefCell<InProcessOutputRuntime>>,
        last_source: Rc<RefCell<Option<OutputPreviewSource>>>,
    ) -> Self {
        let id = ViewId::new();
        let latest_snapshot = Rc::new(RefCell::new(snapshot.get_untracked()));
        let frame = preview_frame(&latest_snapshot.borrow(), &runtime, &last_source);
        let snapshot_for_update = Rc::clone(&latest_snapshot);
        create_effect(move |_| {
            *snapshot_for_update.borrow_mut() = snapshot.get();
        });
        let snapshot_for_clock = Rc::clone(&latest_snapshot);
        create_effect(move |_| {
            let clock = playback_clock.get();
            {
                let mut snapshot = snapshot_for_clock.borrow_mut();
                snapshot.playback.is_playing = clock.is_playing;
                snapshot.playback.time_ms = clock.time_ms;
                snapshot.sequence_playhead_ms = clock.sequence_playhead_ms;
                snapshot.sequence_playhead_home_ms = clock.sequence_playhead_home_ms;
            }
            id.update_state(PreviewCanvasUpdate::Frame(preview_frame(
                &snapshot_for_clock.borrow(),
                &runtime,
                &last_source,
            )));
        });
        Self {
            id,
            frame,
            viewport_size: Size::ZERO,
        }
    }

    fn world_to_screen(&self, x: f64, y: f64) -> (f64, f64, f64) {
        let bounds = self.frame.bounds;
        let world_width = (bounds.max_x - bounds.min_x).max(0.001);
        let world_height = (bounds.max_y - bounds.min_y).max(0.001);
        let scale = ((self.viewport_size.width - 56.0).max(1.0) / world_width)
            .min((self.viewport_size.height - 56.0).max(1.0) / world_height);
        let center_x = (bounds.min_x + bounds.max_x) * 0.5;
        let center_y = (bounds.min_y + bounds.max_y) * 0.5;
        (
            self.viewport_size.width * 0.5 + (x - center_x) * scale,
            self.viewport_size.height * 0.5 - (y - center_y) * scale,
            scale,
        )
    }
}

impl View for PreviewCanvas {
    fn id(&self) -> ViewId {
        self.id
    }

    fn update(&mut self, _cx: &mut UpdateCx, state: Box<dyn std::any::Any>) {
        if let Ok(update) = state.downcast::<PreviewCanvasUpdate>() {
            match *update {
                PreviewCanvasUpdate::Frame(frame) => self.frame = frame,
            }
            self.id.request_paint();
        }
    }

    fn compute_layout(&mut self, _cx: &mut ComputeLayoutCx) -> Option<Rect> {
        let layout = self.id.get_layout().unwrap_or_default();
        self.viewport_size = Size::new(layout.size.width as f64, layout.size.height as f64);
        None
    }

    fn event_before_children(
        &mut self,
        _cx: &mut EventCx,
        _event: &Event,
    ) -> floem::event::EventPropagation {
        floem::event::EventPropagation::Continue
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let panel = self.viewport_size.to_rect();
        cx.fill(&panel, &Brush::Solid(Color::BLACK), 0.0);
        for fixture in &self.frame.fixtures {
            for pixel in &fixture.pixels {
                paint_pixel(cx, self, fixture.bulb_radius, pixel);
            }
        }
    }
}

fn paint_pixel(cx: &mut PaintCx, canvas: &PreviewCanvas, radius: f64, pixel: &OutputPixelFrame) {
    let (x, y, scale) = canvas.world_to_screen(pixel.position.x, pixel.position.y);
    let radius = (radius * scale).max(4.0);
    let color = Color::rgb8(pixel.color.red, pixel.color.green, pixel.color.blue);
    cx.fill(&Circle::new((x, y), radius), &Brush::Solid(color), 0.0);
}

fn bounds_action(bounds: &PreviewWindowLayout) -> AppAction {
    AppAction::SetPreviewWindowBounds {
        x: bounds.x,
        y: bounds.y,
        width: bounds.width,
        height: bounds.height,
    }
}
