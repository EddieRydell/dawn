use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

use dawn_project::model::FixtureId;
use dawn_project::render::{GeometryRenderBounds, GeometryRenderPoint};
use floem::context::{ComputeLayoutCx, EventCx, PaintCx, UpdateCx};
use floem::event::{Event, EventPropagation};
use floem::keyboard::Modifiers;
use floem::kurbo::{Arc, Circle, Line, Point, Rect, Size, Stroke, Vec2};
use floem::peniko::{Brush, Color};
use floem::reactive::create_effect;
use floem::views::Decorators;
use floem::{View, ViewId};
use floem_renderer::Renderer;

use crate::ui::theme;

#[derive(Debug, Clone)]
pub struct CanvasScene {
    pub bounds: GeometryRenderBounds,
    pub layers: Vec<CanvasLayer>,
    pub items: Vec<CanvasItem>,
}

#[derive(Debug, Clone)]
pub struct CanvasLayer {
    pub id: String,
    pub visible: bool,
}

#[derive(Debug, Clone)]
pub struct CanvasItem {
    pub id: String,
    pub kind: CanvasItemKind,
    pub label: Option<String>,
    pub color: Color,
    pub interaction: CanvasItemInteraction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanvasItemInteraction {
    None,
    Target {
        fixture_id: FixtureId,
        selectable: bool,
        draggable: bool,
    },
}

#[derive(Debug, Clone)]
pub enum CanvasItemKind {
    Point {
        position: GeometryRenderPoint,
        radius: f64,
    },
    Line {
        from: GeometryRenderPoint,
        to: GeometryRenderPoint,
    },
    Arc {
        start: GeometryRenderPoint,
        end: GeometryRenderPoint,
        radius_x: f64,
        radius_y: f64,
        rotation: f64,
        large_arc: bool,
        sweep_positive: bool,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct CanvasCamera {
    pub center_x: f64,
    pub center_y: f64,
    pub scale: f64,
}

#[derive(Debug, Clone)]
pub struct CanvasState {
    data: Rc<RefCell<CanvasStateData>>,
}

#[derive(Debug, Default)]
struct CanvasStateData {
    camera: Option<CanvasCamera>,
    selected_targets: BTreeSet<FixtureId>,
    hovered_target: Option<FixtureId>,
}

impl CanvasState {
    pub fn new() -> Self {
        Self {
            data: Rc::new(RefCell::new(CanvasStateData::default())),
        }
    }

    fn camera(&self) -> Option<CanvasCamera> {
        self.data.borrow().camera
    }

    fn set_camera(&self, camera: CanvasCamera) {
        self.data.borrow_mut().camera = Some(camera);
    }

    fn selected_targets(&self) -> BTreeSet<FixtureId> {
        self.data.borrow().selected_targets.clone()
    }

    fn selected_targets_contains(&self, fixture_id: FixtureId) -> bool {
        self.data.borrow().selected_targets.contains(&fixture_id)
    }

    fn set_selected_targets(&self, selected_targets: BTreeSet<FixtureId>) {
        self.data.borrow_mut().selected_targets = selected_targets;
    }

    fn update_selected_targets(&self, update: impl FnOnce(&mut BTreeSet<FixtureId>)) {
        update(&mut self.data.borrow_mut().selected_targets);
    }

    fn hovered_target(&self) -> Option<FixtureId> {
        self.data.borrow().hovered_target.clone()
    }

    fn set_hovered_target(&self, hovered_target: Option<FixtureId>) {
        self.data.borrow_mut().hovered_target = hovered_target;
    }
}

impl Default for CanvasState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct CanvasConfig {
    pub show_grid: bool,
    pub show_axes: bool,
    pub fit_padding: f64,
    pub min_point_radius: f64,
}

impl Default for CanvasConfig {
    fn default() -> Self {
        Self {
            show_grid: true,
            show_axes: true,
            fit_padding: 24.0,
            min_point_radius: 3.0,
        }
    }
}

#[derive(Default)]
pub struct CanvasCallbacks {
    pub on_select: Option<Box<dyn Fn(FixtureId)>>,
    pub on_drag_end: Option<Box<dyn Fn(Vec<FixtureId>, f64, f64)>>,
    pub on_delete: Option<Box<dyn Fn(Vec<FixtureId>)>>,
    pub on_drop_add: Option<Box<dyn Fn(f64, f64)>>,
    pub on_secondary_click: Option<Box<dyn Fn(Point, f64, f64)>>,
}

enum CanvasUpdate {
    Scene(CanvasScene),
}

pub struct Canvas {
    id: ViewId,
    scene: CanvasScene,
    config: CanvasConfig,
    callbacks: CanvasCallbacks,
    state: CanvasState,
    viewport: CanvasViewport,
    gesture: Option<CanvasGesture>,
}

pub fn canvas(scene: impl Fn() -> CanvasScene + 'static) -> Canvas {
    canvas_with_state(CanvasState::new(), scene)
}

pub fn canvas_with_state(state: CanvasState, scene: impl Fn() -> CanvasScene + 'static) -> Canvas {
    Canvas::new(state, scene, CanvasConfig::default())
}

impl Canvas {
    pub fn new(
        state: CanvasState,
        scene: impl Fn() -> CanvasScene + 'static,
        config: CanvasConfig,
    ) -> Self {
        let id = ViewId::new();
        create_effect(move |_| {
            id.update_state(CanvasUpdate::Scene(scene()));
        });
        Self {
            id,
            scene: empty_scene(),
            config,
            callbacks: CanvasCallbacks::default(),
            state,
            viewport: CanvasViewport::default(),
            gesture: None,
        }
        .keyboard_navigable()
    }

    pub fn on_select(mut self, callback: impl Fn(FixtureId) + 'static) -> Self {
        self.callbacks.on_select = Some(Box::new(callback));
        self
    }

    pub fn on_drag_end(mut self, callback: impl Fn(Vec<FixtureId>, f64, f64) + 'static) -> Self {
        self.callbacks.on_drag_end = Some(Box::new(callback));
        self
    }

    pub fn on_delete(mut self, callback: impl Fn(Vec<FixtureId>) + 'static) -> Self {
        self.callbacks.on_delete = Some(Box::new(callback));
        self
    }

    pub fn on_drop_add(mut self, callback: impl Fn(f64, f64) + 'static) -> Self {
        self.callbacks.on_drop_add = Some(Box::new(callback));
        self
    }

    pub fn on_secondary_click(mut self, callback: impl Fn(Point, f64, f64) + 'static) -> Self {
        self.callbacks.on_secondary_click = Some(Box::new(callback));
        self
    }
}

impl View for Canvas {
    fn id(&self) -> ViewId {
        self.id
    }

    fn update(&mut self, _cx: &mut UpdateCx, state: Box<dyn std::any::Any>) {
        if let Ok(update) = state.downcast::<CanvasUpdate>() {
            match *update {
                CanvasUpdate::Scene(scene) => self.scene = scene,
            }
            self.id.request_layout();
            self.id.request_paint();
        }
    }

    fn compute_layout(&mut self, _cx: &mut ComputeLayoutCx) -> Option<Rect> {
        let layout = self.id.get_layout().unwrap_or_default();
        let size = Size::new(layout.size.width as f64, layout.size.height as f64);
        let camera = self.state.camera().unwrap_or_else(|| {
            let viewport = CanvasViewport::fit(size, self.scene.bounds, self.config.fit_padding);
            let camera = viewport.camera();
            self.state.set_camera(camera);
            camera
        });
        self.viewport = CanvasViewport::from_camera(
            Size::new(layout.size.width as f64, layout.size.height as f64),
            self.scene.bounds,
            camera,
        );
        None
    }

    fn event_before_children(&mut self, cx: &mut EventCx, event: &Event) -> EventPropagation {
        match event {
            Event::PointerWheel(event) => self.handle_wheel(event.pos, event.delta),
            Event::PointerDown(event) => {
                if event.button.is_primary() {
                    cx.update_active(self.id);
                    self.id.request_active();
                    self.handle_pointer_down(event.pos, event.modifiers)
                } else if event.button.is_secondary() && self.callbacks.on_secondary_click.is_some()
                {
                    self.gesture = None;
                    EventPropagation::Stop
                } else {
                    EventPropagation::Continue
                }
            }
            Event::PointerMove(event) => self.handle_pointer_move(event.pos),
            Event::PointerUp(event) => {
                if event.button.is_primary() {
                    self.id.clear_active();
                    self.handle_pointer_up(event.pos)
                } else if event.button.is_secondary() {
                    self.handle_secondary_click(event.pos)
                } else {
                    EventPropagation::Continue
                }
            }
            Event::PointerLeave => {
                self.state.set_hovered_target(None);
                self.id.request_paint();
                EventPropagation::Continue
            }
            Event::FocusLost => {
                self.gesture = None;
                self.state.set_hovered_target(None);
                self.id.clear_active();
                self.id.request_paint();
                EventPropagation::Continue
            }
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

        if self.config.show_grid {
            self.paint_grid(cx);
        }
        if self.config.show_axes {
            self.paint_axes(cx);
        }
        self.paint_items(cx);
        self.paint_selection_box(cx);
    }
}

impl Canvas {
    fn handle_wheel(&mut self, position: Point, delta: Vec2) -> EventPropagation {
        let before = self.viewport.screen_to_world(position);
        let factor = (-delta.y * WHEEL_ZOOM_SENSITIVITY).exp();
        let next_scale = (self.viewport.scale * factor).clamp(MIN_ZOOM_SCALE, MAX_ZOOM_SCALE);
        if (next_scale - self.viewport.scale).abs() <= f64::EPSILON {
            return EventPropagation::Stop;
        }
        let center_x = before.x - (position.x - self.viewport.size.width / 2.0) / next_scale;
        let center_y = before.y + (position.y - self.viewport.size.height / 2.0) / next_scale;
        self.update_camera(CanvasCamera {
            center_x,
            center_y,
            scale: next_scale,
        });
        EventPropagation::Stop
    }

    fn handle_pointer_down(&mut self, position: Point, modifiers: Modifiers) -> EventPropagation {
        self.state.set_hovered_target(None);
        let additive = additive_selection(modifiers);

        let Some(hit) = self.hit_test(position) else {
            self.gesture = if additive {
                Some(CanvasGesture::SelectBox {
                    start_screen: position,
                    current_screen: position,
                    active: false,
                    additive: true,
                })
            } else {
                Some(CanvasGesture::PendingPan {
                    start_screen: position,
                    current_screen: position,
                    start_camera: self.viewport.camera(),
                    active: false,
                })
            };
            self.id.request_paint();
            return EventPropagation::Stop;
        };

        let selected_before = self.state.selected_targets_contains(hit.fixture_id);

        if hit.selectable {
            if additive {
                self.state.update_selected_targets(|selected| {
                    selected.insert(hit.fixture_id);
                });
            } else if !selected_before {
                let mut selected = BTreeSet::new();
                selected.insert(hit.fixture_id);
                self.state.set_selected_targets(selected);
            }
            if let Some(callback) = &self.callbacks.on_select {
                callback(hit.fixture_id);
            }
        }

        let selected_targets = self.state.selected_targets();
        self.gesture = hit.draggable.then_some(CanvasGesture::EditDrag {
            target_ids: if selected_targets.contains(&hit.fixture_id) {
                selected_targets
            } else {
                BTreeSet::from([hit.fixture_id])
            },
            start_screen: position,
            current_screen: position,
            active: false,
        });
        self.id.request_paint();
        EventPropagation::Stop
    }

    fn handle_pointer_move(&mut self, position: Point) -> EventPropagation {
        match self.gesture.clone() {
            Some(CanvasGesture::PendingPan {
                start_screen,
                start_camera,
                active,
                ..
            }) => {
                if let Some(CanvasGesture::PendingPan {
                    current_screen,
                    active: gesture_active,
                    ..
                }) = &mut self.gesture
                {
                    *current_screen = position;
                    if !*gesture_active && position.distance(start_screen) >= DRAG_THRESHOLD_PX {
                        *gesture_active = true;
                    }
                }
                if active || position.distance(start_screen) >= DRAG_THRESHOLD_PX {
                    let dx = (position.x - start_screen.x) / start_camera.scale;
                    let dy = (position.y - start_screen.y) / start_camera.scale;
                    self.update_camera(CanvasCamera {
                        center_x: start_camera.center_x - dx,
                        center_y: start_camera.center_y + dy,
                        scale: start_camera.scale,
                    });
                }
                EventPropagation::Stop
            }
            Some(CanvasGesture::EditDrag { .. }) => {
                if let Some(CanvasGesture::EditDrag {
                    start_screen,
                    current_screen,
                    active,
                    ..
                }) = &mut self.gesture
                {
                    *current_screen = position;
                    if !*active && position.distance(*start_screen) >= DRAG_THRESHOLD_PX {
                        *active = true;
                    }
                }
                self.id.request_paint();
                EventPropagation::Stop
            }
            Some(CanvasGesture::SelectBox { .. }) => {
                if let Some(CanvasGesture::SelectBox {
                    start_screen,
                    current_screen,
                    active,
                    ..
                }) = &mut self.gesture
                {
                    *current_screen = position;
                    if !*active && position.distance(*start_screen) >= DRAG_THRESHOLD_PX {
                        *active = true;
                    }
                }
                self.id.request_paint();
                EventPropagation::Stop
            }
            None => {
                let hovered = self.hit_test(position).map(|hit| hit.fixture_id);
                if hovered != self.state.hovered_target() {
                    self.state.set_hovered_target(hovered);
                    self.id.request_paint();
                }
                EventPropagation::Continue
            }
        }
    }

    fn handle_pointer_up(&mut self, position: Point) -> EventPropagation {
        let Some(gesture) = self.gesture.take() else {
            return EventPropagation::Continue;
        };
        match gesture {
            CanvasGesture::PendingPan { active, .. } => {
                if !active {
                    self.state.set_selected_targets(BTreeSet::new());
                    self.id.request_paint();
                }
                EventPropagation::Stop
            }
            CanvasGesture::EditDrag {
                target_ids,
                start_screen,
                active,
                ..
            } => {
                if active {
                    let start = self.viewport.screen_to_world(start_screen);
                    let end = self.viewport.screen_to_world(position);
                    let dx = end.x - start.x;
                    let dy = end.y - start.y;
                    if (dx.abs() > f64::EPSILON || dy.abs() > f64::EPSILON)
                        && self.callbacks.on_drag_end.is_some()
                    {
                        if let Some(callback) = &self.callbacks.on_drag_end {
                            callback(target_ids.into_iter().collect(), dx, dy);
                        }
                    }
                }
                self.id.request_paint();
                EventPropagation::Stop
            }
            CanvasGesture::SelectBox {
                start_screen,
                active,
                additive,
                ..
            } => {
                if active {
                    let selection = selection_rect(start_screen, position);
                    let selected = self.targets_in_selection_rect(selection);
                    if additive {
                        self.state.update_selected_targets(|targets| {
                            targets.extend(selected);
                        });
                    } else {
                        self.state.set_selected_targets(selected);
                    }
                }
                self.id.request_paint();
                EventPropagation::Stop
            }
        }
    }

    fn handle_secondary_click(&mut self, position: Point) -> EventPropagation {
        let Some(callback) = &self.callbacks.on_secondary_click else {
            return EventPropagation::Continue;
        };
        self.gesture = None;
        self.state.set_hovered_target(None);
        let world = self.viewport.screen_to_world(position);
        callback(position, world.x, world.y);
        self.id.request_paint();
        EventPropagation::Stop
    }

    fn update_camera(&mut self, camera: CanvasCamera) {
        self.state.set_camera(camera);
        self.viewport = CanvasViewport::from_camera(self.viewport.size, self.scene.bounds, camera);
        self.id.request_paint();
    }

    fn paint_grid(&self, cx: &mut PaintCx) {
        let bounds = self.viewport.bounds;
        let span = (bounds.max_x - bounds.min_x)
            .abs()
            .max((bounds.max_y - bounds.min_y).abs());
        let step = grid_step(span);
        let stroke = Stroke::new(1.0);
        let brush = Brush::Solid(Color::rgba8(255, 255, 255, 18));

        let start_x = (bounds.min_x / step).floor() as i32 - 1;
        let end_x = (bounds.max_x / step).ceil() as i32 + 1;
        for index in start_x..=end_x {
            let x = index as f64 * step;
            let from = self.viewport.world_to_screen(GeometryRenderPoint {
                x,
                y: bounds.min_y,
                z: 0.0,
            });
            let to = self.viewport.world_to_screen(GeometryRenderPoint {
                x,
                y: bounds.max_y,
                z: 0.0,
            });
            cx.stroke(&Line::new(from, to), &brush, &stroke);
        }

        let start_y = (bounds.min_y / step).floor() as i32 - 1;
        let end_y = (bounds.max_y / step).ceil() as i32 + 1;
        for index in start_y..=end_y {
            let y = index as f64 * step;
            let from = self.viewport.world_to_screen(GeometryRenderPoint {
                x: bounds.min_x,
                y,
                z: 0.0,
            });
            let to = self.viewport.world_to_screen(GeometryRenderPoint {
                x: bounds.max_x,
                y,
                z: 0.0,
            });
            cx.stroke(&Line::new(from, to), &brush, &stroke);
        }
    }

    fn paint_axes(&self, cx: &mut PaintCx) {
        let bounds = self.viewport.bounds;
        let stroke = Stroke::new(1.5);
        let x_axis = Brush::Solid(Color::rgba8(230, 96, 88, 150));
        let y_axis = Brush::Solid(Color::rgba8(86, 180, 124, 150));

        if bounds.min_y <= 0.0 && bounds.max_y >= 0.0 {
            let from = self.viewport.world_to_screen(GeometryRenderPoint {
                x: bounds.min_x,
                y: 0.0,
                z: 0.0,
            });
            let to = self.viewport.world_to_screen(GeometryRenderPoint {
                x: bounds.max_x,
                y: 0.0,
                z: 0.0,
            });
            cx.stroke(&Line::new(from, to), &x_axis, &stroke);
        }

        if bounds.min_x <= 0.0 && bounds.max_x >= 0.0 {
            let from = self.viewport.world_to_screen(GeometryRenderPoint {
                x: 0.0,
                y: bounds.min_y,
                z: 0.0,
            });
            let to = self.viewport.world_to_screen(GeometryRenderPoint {
                x: 0.0,
                y: bounds.max_y,
                z: 0.0,
            });
            cx.stroke(&Line::new(from, to), &y_axis, &stroke);
        }
    }

    fn paint_items(&self, cx: &mut PaintCx) {
        let selected_targets = self.state.selected_targets();
        let hovered_target = self.state.hovered_target();
        for item in &self.scene.items {
            let offset = self.drag_offset_for(item);
            let target_id = item_target_id(item);
            let selected = target_id
                .as_ref()
                .is_some_and(|fixture_id| selected_targets.contains(&fixture_id));
            let hovered = !selected
                && target_id
                    .as_ref()
                    .is_some_and(|fixture_id| hovered_target == Some(*fixture_id));
            let brush = Brush::Solid(item.color);
            let highlight_brush = if selected {
                Some(Brush::Solid(theme::color(theme::TEXT_INVERTED)))
            } else if hovered {
                Some(Brush::Solid(Color::rgba8(255, 255, 255, 170)))
            } else {
                None
            };
            let guide_stroke = Stroke::new(if selected {
                2.0
            } else if hovered {
                1.75
            } else {
                1.25
            });
            match &item.kind {
                CanvasItemKind::Point { position, radius } => {
                    let center = self
                        .viewport
                        .world_to_screen(offset_point(*position, offset));
                    let radius = (radius * self.viewport.scale).max(self.config.min_point_radius);
                    if let Some(highlight_brush) = &highlight_brush {
                        cx.stroke(
                            &Circle::new(center, radius + 3.0),
                            highlight_brush,
                            &Stroke::new(1.5),
                        );
                    }
                    cx.fill(&Circle::new(center, radius), &brush, 0.0);
                }
                CanvasItemKind::Line { from, to } => {
                    let line = Line::new(
                        self.viewport.world_to_screen(offset_point(*from, offset)),
                        self.viewport.world_to_screen(offset_point(*to, offset)),
                    );
                    if let Some(highlight_brush) = &highlight_brush {
                        cx.stroke(
                            &line,
                            highlight_brush,
                            &Stroke::new(guide_stroke.width + 2.5),
                        );
                    }
                    cx.stroke(&line, &brush, &guide_stroke);
                }
                CanvasItemKind::Arc {
                    start,
                    end,
                    radius_x,
                    radius_y,
                    rotation,
                    large_arc,
                    sweep_positive,
                } => {
                    let start = self.viewport.world_to_screen(offset_point(*start, offset));
                    let end = self.viewport.world_to_screen(offset_point(*end, offset));
                    if let Some(arc) = endpoint_arc_to_center(
                        start,
                        end,
                        radius_x * self.viewport.scale,
                        radius_y * self.viewport.scale,
                        -rotation.to_radians(),
                        *large_arc,
                        !*sweep_positive,
                    ) {
                        if let Some(highlight_brush) = &highlight_brush {
                            cx.stroke(
                                &arc,
                                highlight_brush,
                                &Stroke::new(guide_stroke.width + 2.5),
                            );
                        }
                        cx.stroke(&arc, &brush, &guide_stroke);
                    } else {
                        let line = Line::new(start, end);
                        if let Some(highlight_brush) = &highlight_brush {
                            cx.stroke(
                                &line,
                                highlight_brush,
                                &Stroke::new(guide_stroke.width + 2.5),
                            );
                        }
                        cx.stroke(&line, &brush, &guide_stroke);
                    }
                }
            }
        }
    }

    fn paint_selection_box(&self, cx: &mut PaintCx) {
        let Some(CanvasGesture::SelectBox {
            start_screen,
            current_screen,
            active,
            ..
        }) = self.gesture
        else {
            return;
        };
        if !active {
            return;
        }
        let rect = selection_rect(start_screen, current_screen);
        cx.fill(&rect, &Brush::Solid(Color::rgba8(118, 185, 255, 38)), 0.0);
        cx.stroke(
            &rect,
            &Brush::Solid(Color::rgba8(118, 185, 255, 210)),
            &Stroke::new(1.0),
        );
    }

    fn drag_offset_for(&self, item: &CanvasItem) -> Option<(f64, f64)> {
        let CanvasItemInteraction::Target { fixture_id, .. } = &item.interaction else {
            return None;
        };
        let Some(CanvasGesture::EditDrag {
            target_ids,
            start_screen,
            current_screen,
            active,
        }) = &self.gesture
        else {
            return None;
        };
        if !active || !target_ids.contains(fixture_id) {
            return None;
        }
        let start = self.viewport.screen_to_world(*start_screen);
        let current = self.viewport.screen_to_world(*current_screen);
        Some((current.x - start.x, current.y - start.y))
    }

    fn hit_test(&self, position: Point) -> Option<CanvasHit> {
        self.scene.items.iter().rev().find_map(|item| {
            let CanvasItemInteraction::Target {
                fixture_id,
                selectable,
                draggable,
            } = &item.interaction
            else {
                return None;
            };
            item_hit_distance(&self.viewport, item, position)
                .filter(|distance| *distance <= HIT_TOLERANCE_PX)
                .map(|_| CanvasHit {
                    fixture_id: *fixture_id,
                    selectable: *selectable,
                    draggable: *draggable,
                })
        })
    }

    fn targets_in_selection_rect(&self, selection: Rect) -> BTreeSet<FixtureId> {
        self.scene
            .items
            .iter()
            .fold(BTreeSet::new(), |mut targets, item| {
                let CanvasItemInteraction::Target {
                    fixture_id,
                    selectable,
                    ..
                } = &item.interaction
                else {
                    return targets;
                };
                if !selectable {
                    return targets;
                }
                if item_screen_bounds(&self.viewport, item)
                    .is_some_and(|bounds| bounds.overlaps(selection))
                {
                    targets.insert(*fixture_id);
                }
                targets
            })
    }
}

#[derive(Debug, Clone, Copy)]
struct CanvasViewport {
    size: Size,
    bounds: GeometryRenderBounds,
    center_x: f64,
    center_y: f64,
    scale: f64,
}

impl Default for CanvasViewport {
    fn default() -> Self {
        Self::fit(Size::ZERO, default_bounds(), 0.0)
    }
}

impl CanvasViewport {
    fn from_camera(size: Size, scene_bounds: GeometryRenderBounds, camera: CanvasCamera) -> Self {
        let scale = camera.scale.clamp(MIN_ZOOM_SCALE, MAX_ZOOM_SCALE);
        let half_width = size.width / scale / 2.0;
        let half_height = size.height / scale / 2.0;
        Self {
            size,
            bounds: GeometryRenderBounds {
                min_x: camera.center_x - half_width,
                min_y: camera.center_y - half_height,
                max_x: camera.center_x + half_width,
                max_y: camera.center_y + half_height,
            },
            center_x: camera.center_x,
            center_y: camera.center_y,
            scale,
        }
        .with_scene_bounds(scene_bounds)
    }

    fn fit(size: Size, bounds: GeometryRenderBounds, padding: f64) -> Self {
        let bounds = normalize_bounds(bounds);
        let available_width = (size.width - padding * 2.0).max(1.0);
        let available_height = (size.height - padding * 2.0).max(1.0);
        let width = (bounds.max_x - bounds.min_x).max(0.0001);
        let height = (bounds.max_y - bounds.min_y).max(0.0001);
        let scale = (available_width / width)
            .min(available_height / height)
            .max(0.0001);
        Self {
            size,
            bounds,
            center_x: (bounds.min_x + bounds.max_x) / 2.0,
            center_y: (bounds.min_y + bounds.max_y) / 2.0,
            scale,
        }
    }

    fn camera(&self) -> CanvasCamera {
        CanvasCamera {
            center_x: self.center_x,
            center_y: self.center_y,
            scale: self.scale,
        }
    }

    fn world_to_screen(&self, point: GeometryRenderPoint) -> Point {
        Point::new(
            (point.x - self.center_x) * self.scale + self.size.width / 2.0,
            (self.center_y - point.y) * self.scale + self.size.height / 2.0,
        )
    }

    fn screen_to_world(&self, point: Point) -> GeometryRenderPoint {
        GeometryRenderPoint {
            x: self.center_x + (point.x - self.size.width / 2.0) / self.scale,
            y: self.center_y - (point.y - self.size.height / 2.0) / self.scale,
            z: 0.0,
        }
    }

    fn with_scene_bounds(mut self, scene_bounds: GeometryRenderBounds) -> Self {
        if self.size.width <= 0.0 || self.size.height <= 0.0 {
            self.bounds = normalize_bounds(scene_bounds);
        }
        self
    }
}

#[derive(Debug, Clone)]
enum CanvasGesture {
    PendingPan {
        start_screen: Point,
        current_screen: Point,
        start_camera: CanvasCamera,
        active: bool,
    },
    EditDrag {
        target_ids: BTreeSet<FixtureId>,
        start_screen: Point,
        current_screen: Point,
        active: bool,
    },
    SelectBox {
        start_screen: Point,
        current_screen: Point,
        active: bool,
        additive: bool,
    },
}

#[derive(Debug, Clone)]
struct CanvasHit {
    fixture_id: FixtureId,
    selectable: bool,
    draggable: bool,
}

const MIN_ZOOM_SCALE: f64 = 0.02;
const MAX_ZOOM_SCALE: f64 = 2000.0;
const WHEEL_ZOOM_SENSITIVITY: f64 = 0.0025;
const HIT_TOLERANCE_PX: f64 = 8.0;
const DRAG_THRESHOLD_PX: f64 = 3.0;

fn additive_selection(modifiers: Modifiers) -> bool {
    modifiers.shift() || modifiers.control()
}

fn item_target_id(item: &CanvasItem) -> Option<FixtureId> {
    match &item.interaction {
        CanvasItemInteraction::Target { fixture_id, .. } => Some(*fixture_id),
        CanvasItemInteraction::None => None,
    }
}

fn item_hit_distance(viewport: &CanvasViewport, item: &CanvasItem, position: Point) -> Option<f64> {
    match &item.kind {
        CanvasItemKind::Point {
            position: point,
            radius,
        } => Some(
            viewport.world_to_screen(*point).distance(position)
                - (radius * viewport.scale).max(HIT_TOLERANCE_PX / 2.0),
        ),
        CanvasItemKind::Line { from, to } => Some(distance_to_segment(
            position,
            viewport.world_to_screen(*from),
            viewport.world_to_screen(*to),
        )),
        CanvasItemKind::Arc {
            start,
            end,
            radius_x,
            radius_y,
            rotation,
            large_arc,
            sweep_positive,
        } => {
            let start = viewport.world_to_screen(*start);
            let end = viewport.world_to_screen(*end);
            let arc = endpoint_arc_to_center(
                start,
                end,
                radius_x * viewport.scale,
                radius_y * viewport.scale,
                -rotation.to_radians(),
                *large_arc,
                !*sweep_positive,
            )?;
            Some(distance_to_arc(position, arc))
        }
    }
}

fn item_screen_bounds(viewport: &CanvasViewport, item: &CanvasItem) -> Option<Rect> {
    match &item.kind {
        CanvasItemKind::Point { position, radius } => {
            let center = viewport.world_to_screen(*position);
            let radius = (radius * viewport.scale).max(HIT_TOLERANCE_PX / 2.0);
            Some(Rect::new(center.x, center.y, center.x, center.y).inflate(radius, radius))
        }
        CanvasItemKind::Line { from, to } => Some(rect_from_points(
            viewport.world_to_screen(*from),
            viewport.world_to_screen(*to),
        )),
        CanvasItemKind::Arc {
            start,
            end,
            radius_x,
            radius_y,
            rotation,
            large_arc,
            sweep_positive,
        } => {
            let start = viewport.world_to_screen(*start);
            let end = viewport.world_to_screen(*end);
            let arc = endpoint_arc_to_center(
                start,
                end,
                radius_x * viewport.scale,
                radius_y * viewport.scale,
                -rotation.to_radians(),
                *large_arc,
                !*sweep_positive,
            )?;
            Some(arc_bounds(arc))
        }
    }
    .map(|rect| rect.inflate(HIT_TOLERANCE_PX / 2.0, HIT_TOLERANCE_PX / 2.0))
}

fn selection_rect(from: Point, to: Point) -> Rect {
    rect_from_points(from, to)
}

fn rect_from_points(from: Point, to: Point) -> Rect {
    Rect::new(
        from.x.min(to.x),
        from.y.min(to.y),
        from.x.max(to.x),
        from.y.max(to.y),
    )
}

fn arc_bounds(arc: Arc) -> Rect {
    let samples = ((arc.sweep_angle.abs() / std::f64::consts::TAU) * 96.0)
        .ceil()
        .max(12.0) as usize;
    let mut bounds = Rect::new(
        arc_point(arc, 0.0).x,
        arc_point(arc, 0.0).y,
        arc_point(arc, 0.0).x,
        arc_point(arc, 0.0).y,
    );
    for index in 1..=samples {
        bounds = bounds.union_pt(arc_point(arc, index as f64 / samples as f64));
    }
    bounds
}

fn distance_to_segment(point: Point, from: Point, to: Point) -> f64 {
    let segment = to - from;
    let length_squared = segment.x * segment.x + segment.y * segment.y;
    if length_squared <= f64::EPSILON {
        return point.distance(from);
    }
    let point_delta = point - from;
    let t =
        ((point_delta.x * segment.x + point_delta.y * segment.y) / length_squared).clamp(0.0, 1.0);
    point.distance(Point::new(from.x + segment.x * t, from.y + segment.y * t))
}

fn distance_to_arc(point: Point, arc: Arc) -> f64 {
    let samples = ((arc.sweep_angle.abs() / std::f64::consts::TAU) * 96.0)
        .ceil()
        .max(12.0) as usize;
    let mut distance = f64::INFINITY;
    let mut previous = arc_point(arc, 0.0);
    for index in 1..=samples {
        let t = index as f64 / samples as f64;
        let current = arc_point(arc, t);
        distance = distance.min(distance_to_segment(point, previous, current));
        previous = current;
    }
    distance
}

fn arc_point(arc: Arc, t: f64) -> Point {
    let angle = arc.start_angle + arc.sweep_angle * t;
    let local_x = arc.radii.x * angle.cos();
    let local_y = arc.radii.y * angle.sin();
    let cos = arc.x_rotation.cos();
    let sin = arc.x_rotation.sin();
    Point::new(
        arc.center.x + local_x * cos - local_y * sin,
        arc.center.y + local_x * sin + local_y * cos,
    )
}

fn offset_point(point: GeometryRenderPoint, offset: Option<(f64, f64)>) -> GeometryRenderPoint {
    let Some((dx, dy)) = offset else {
        return point;
    };
    GeometryRenderPoint {
        x: point.x + dx,
        y: point.y + dy,
        z: point.z,
    }
}

fn endpoint_arc_to_center(
    start: Point,
    end: Point,
    radius_x: f64,
    radius_y: f64,
    rotation: f64,
    large_arc: bool,
    sweep_positive: bool,
) -> Option<Arc> {
    let mut rx = radius_x.abs();
    let mut ry = radius_y.abs();
    if rx <= 0.0 || ry <= 0.0 || start.distance(end) <= 0.0001 {
        return None;
    }

    let dx = (start.x - end.x) / 2.0;
    let dy = (start.y - end.y) / 2.0;
    let cos_phi = rotation.cos();
    let sin_phi = rotation.sin();
    let x1 = cos_phi * dx + sin_phi * dy;
    let y1 = -sin_phi * dx + cos_phi * dy;
    let radius_check = x1.powi(2) / rx.powi(2) + y1.powi(2) / ry.powi(2);
    if radius_check > 1.0 {
        let factor = radius_check.sqrt();
        rx *= factor;
        ry *= factor;
    }

    let numerator = rx.powi(2) * ry.powi(2) - rx.powi(2) * y1.powi(2) - ry.powi(2) * x1.powi(2);
    let denominator = rx.powi(2) * y1.powi(2) + ry.powi(2) * x1.powi(2);
    if denominator <= 0.0 {
        return None;
    }

    let sign = if large_arc == sweep_positive {
        -1.0
    } else {
        1.0
    };
    let coefficient = sign * (numerator / denominator).max(0.0).sqrt();
    let cx1 = coefficient * rx * y1 / ry;
    let cy1 = -coefficient * ry * x1 / rx;
    let center = Point::new(
        cos_phi * cx1 - sin_phi * cy1 + (start.x + end.x) / 2.0,
        sin_phi * cx1 + cos_phi * cy1 + (start.y + end.y) / 2.0,
    );

    let start_angle = angle_between(
        Vec2::new(1.0, 0.0),
        Vec2::new((x1 - cx1) / rx, (y1 - cy1) / ry),
    );
    let mut sweep_angle = angle_between(
        Vec2::new((x1 - cx1) / rx, (y1 - cy1) / ry),
        Vec2::new((-x1 - cx1) / rx, (-y1 - cy1) / ry),
    );
    if !sweep_positive && sweep_angle > 0.0 {
        sweep_angle -= std::f64::consts::TAU;
    } else if sweep_positive && sweep_angle < 0.0 {
        sweep_angle += std::f64::consts::TAU;
    }

    Some(Arc::new(
        center,
        Vec2::new(rx, ry),
        start_angle,
        sweep_angle,
        rotation,
    ))
}

fn angle_between(from: Vec2, to: Vec2) -> f64 {
    let dot = from.x * to.x + from.y * to.y;
    let determinant = from.x * to.y - from.y * to.x;
    determinant.atan2(dot)
}

fn grid_step(span: f64) -> f64 {
    if span <= 0.0 || !span.is_finite() {
        return 1.0;
    }
    let base = 10.0_f64.powf((span / 10.0).log10().floor());
    for multiplier in [1.0, 2.0, 5.0, 10.0] {
        let step = base * multiplier;
        if span / step <= 12.0 {
            return step;
        }
    }
    base * 10.0
}

fn normalize_bounds(bounds: GeometryRenderBounds) -> GeometryRenderBounds {
    if !bounds.min_x.is_finite()
        || !bounds.min_y.is_finite()
        || !bounds.max_x.is_finite()
        || !bounds.max_y.is_finite()
        || bounds.min_x > bounds.max_x
        || bounds.min_y > bounds.max_y
    {
        return default_bounds();
    }
    let width = bounds.max_x - bounds.min_x;
    let height = bounds.max_y - bounds.min_y;
    GeometryRenderBounds {
        min_x: bounds.min_x - ((1.0 - width).max(0.0) / 2.0),
        min_y: bounds.min_y - ((1.0 - height).max(0.0) / 2.0),
        max_x: bounds.max_x + ((1.0 - width).max(0.0) / 2.0),
        max_y: bounds.max_y + ((1.0 - height).max(0.0) / 2.0),
    }
}

fn empty_scene() -> CanvasScene {
    CanvasScene {
        bounds: default_bounds(),
        layers: Vec::new(),
        items: Vec::new(),
    }
}

fn default_bounds() -> GeometryRenderBounds {
    GeometryRenderBounds {
        min_x: -5.0,
        min_y: -4.0,
        max_x: 5.0,
        max_y: 4.0,
    }
}
