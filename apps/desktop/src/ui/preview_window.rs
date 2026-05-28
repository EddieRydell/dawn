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
use crate::output_runtime::{OutputFrame, OutputPixelFrame};
use crate::ui::components::ui_label;
use crate::ui::theme;

pub fn preview_window_view(
    window_id: WindowId,
    snapshot: crate::ui::UiSnapshot,
    preview_snapshot: crate::ui::UiPreviewSnapshot,
    dispatch: crate::ui::UiDispatch,
    preview_window_id: Rc<RefCell<Option<WindowId>>>,
) -> impl IntoView {
    *preview_window_id.borrow_mut() = Some(window_id);
    let last_bounds = Rc::new(RefCell::new(
        snapshot.get_untracked().workbench_layout.preview_window,
    ));
    let bounds_for_move = Rc::clone(&last_bounds);
    let bounds_for_resize = Rc::clone(&last_bounds);
    let move_dispatch = Rc::clone(&dispatch);
    let resize_dispatch = Rc::clone(&dispatch);
    let close_dispatch = Rc::clone(&dispatch);
    let close_window_id = Rc::clone(&preview_window_id);

    v_stack((preview_content(preview_snapshot).style(|s| s.size_full()),))
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

fn preview_content(preview_snapshot: crate::ui::UiPreviewSnapshot) -> impl IntoView {
    v_stack((
        h_stack((
            ui_label(move || preview_snapshot.get().source_label).style(|s| {
                s.font_size(theme::FONT_SMALL)
                    .font_bold()
                    .color(theme::color(theme::TEXT))
                    .set(Foreground, Brush::Solid(theme::color(theme::TEXT)))
            }),
            ui_label(move || preview_header_status(&preview_snapshot.get())).style(|s| {
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
        PreviewCanvas::new(preview_snapshot).style(|s| s.flex_grow(1.0).min_height(0.0)),
    ))
    .style(|s| s.width_full().height_full().background(Color::BLACK))
}

fn preview_header_status(snapshot: &crate::preview_session::PreviewSnapshot) -> String {
    format!(
        "{}  {} / {} ms",
        snapshot.status, snapshot.position_ms, snapshot.duration_ms
    )
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
    fn new(preview_snapshot: crate::ui::UiPreviewSnapshot) -> Self {
        let id = ViewId::new();
        let frame = preview_snapshot.get_untracked().frame;
        create_effect(move |_| {
            id.update_state(PreviewCanvasUpdate::Frame(preview_snapshot.get().frame));
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
