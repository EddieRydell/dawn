use std::collections::HashMap;
use std::ops::Range;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use dawn_language::model::DawnProject;
use dawn_language::setup::{FixtureInstanceId, Geometry};
use dawn_language::values::{Distance, DistanceSpan, Point3};
use glam::{EulerRot, Mat4, Vec2, Vec3};
use tauri::async_runtime::block_on;
use tauri::window::WindowBuilder;
use tauri::{AppHandle, Manager, Window};
use wgpu::util::DeviceExt;

use crate::dto::AudioTransportState;
use crate::show_render::AudioClockRenderIdentity;
use crate::show_render::ShowRenderError;

pub const PREVIEW_LABEL: &str = "preview";
const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

pub struct PreviewWindowService {
    running: Mutex<Option<Arc<AtomicBool>>>,
    closing_for_main_shutdown: Arc<AtomicBool>,
}

impl PreviewWindowService {
    pub fn new() -> Self {
        Self {
            running: Mutex::new(None),
            closing_for_main_shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn open_or_focus(
        &self,
        app: AppHandle,
        restore: crate::persistence::PersistedPreviewWindowState,
    ) -> Result<(), String> {
        if let Some(window) = app.get_window(PREVIEW_LABEL) {
            window.set_focus().map_err(|error| error.to_string())?;
            return Ok(());
        }
        self.closing_for_main_shutdown
            .store(false, Ordering::Release);

        let window = WindowBuilder::new(&app, PREVIEW_LABEL)
            .title("Dawn Preview")
            .inner_size(960.0, 640.0)
            .min_inner_size(360.0, 240.0)
            .center()
            .build()
            .map_err(|error| error.to_string())?;
        if let Some(geometry) = restore.geometry.as_ref() {
            crate::persistence::apply_window_state(&window, geometry);
        }
        let renderer = PreviewRenderer::new(&window)?;
        let running = Arc::new(AtomicBool::new(true));

        {
            let mut current = self
                .running
                .lock()
                .map_err(|_| "Preview lifecycle lock is poisoned.".to_string())?;
            *current = Some(running.clone());
        }

        let close_flag = running.clone();
        let close_app = app.clone();
        let closing_for_main_shutdown = self.closing_for_main_shutdown.clone();
        window.on_window_event(move |event| match event {
            tauri::WindowEvent::CloseRequested { .. } => {
                if closing_for_main_shutdown.load(Ordering::Acquire) {
                    return;
                }
                let state = close_app.state::<crate::state::DesktopState>();
                let geometry = close_app
                    .get_window(PREVIEW_LABEL)
                    .and_then(|window| crate::persistence::read_window_state(&window));
                let _ = state.persistence().record_preview_window(
                    crate::persistence::PersistedPreviewWindowState {
                        open: false,
                        geometry,
                    },
                );
            }
            tauri::WindowEvent::Destroyed => {
                close_flag.store(false, Ordering::Release);
            }
            tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                let state = close_app.state::<crate::state::DesktopState>();
                let geometry = close_app
                    .get_window(PREVIEW_LABEL)
                    .and_then(|window| crate::persistence::read_window_state(&window));
                let _ = state.persistence().record_preview_window(
                    crate::persistence::PersistedPreviewWindowState {
                        open: true,
                        geometry,
                    },
                );
            }
            _ => {}
        });

        std::thread::spawn(move || {
            run_preview_loop(app, window, renderer, running);
        });

        Ok(())
    }

    pub fn close_for_main_shutdown(
        &self,
        app: &AppHandle,
        persistence: &crate::persistence::PersistenceService,
    ) -> Result<(), String> {
        let Some(window) = app.get_window(PREVIEW_LABEL) else {
            return Ok(());
        };
        let geometry = crate::persistence::read_window_state(&window);
        persistence.record_preview_window(crate::persistence::PersistedPreviewWindowState {
            open: true,
            geometry,
        })?;
        self.closing_for_main_shutdown
            .store(true, Ordering::Release);
        window.close().map_err(|error| error.to_string())
    }
}

impl Default for PreviewWindowService {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct PreviewScene {
    revision: u64,
    instances: Vec<PreviewInstanceGpu>,
    fixture_ranges: HashMap<FixtureInstanceId, Range<usize>>,
    bounds: PreviewBounds,
}

impl PreviewScene {
    pub fn from_project(revision: u64, project: &DawnProject) -> Self {
        let Some(setup) = project.setups.get(&project.root.setup) else {
            return Self::empty(revision);
        };
        let Some(layout) = project.layouts.get(&setup.layout) else {
            return Self::empty(revision);
        };

        let mut instances = Vec::new();
        let mut fixture_ranges = HashMap::new();
        for fixture in &layout.fixtures {
            let Some(definition) = project.definitions.fixtures.get(&fixture.definition) else {
                continue;
            };
            let transform = Mat4::from_translation(point_vec3(fixture.position))
                * Mat4::from_euler(
                    EulerRot::XYZ,
                    fixture.rotation.x.to_radians() as f32,
                    fixture.rotation.y.to_radians() as f32,
                    fixture.rotation.z.to_radians() as f32,
                )
                * Mat4::from_scale(Vec3::new(
                    fixture.scale.x as f32,
                    fixture.scale.y as f32,
                    fixture.scale.z as f32,
                ));
            let radius_meters = distance_span_meters(definition.bulb_radius) as f32;
            let fixture_start = instances.len();
            for emitter in emitters(&definition.geometry) {
                let point = transform.transform_point3(emitter);
                instances.push(PreviewInstanceGpu {
                    center_radius: [point.x, point.y, radius_meters.max(0.005), 0.0],
                });
            }
            let fixture_end = instances.len();
            if fixture_start < fixture_end {
                fixture_ranges.insert(fixture.id.clone(), fixture_start..fixture_end);
            }
        }

        let bounds = PreviewBounds::from_instances(&instances);
        Self {
            revision,
            instances,
            fixture_ranges,
            bounds,
        }
    }

    fn empty(revision: u64) -> Self {
        Self {
            revision,
            instances: Vec::new(),
            fixture_ranges: HashMap::new(),
            bounds: PreviewBounds::default(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PreviewBounds {
    min: Vec2,
    max: Vec2,
}

impl PreviewBounds {
    fn from_instances(instances: &[PreviewInstanceGpu]) -> Self {
        let Some(first) = instances.first() else {
            return Self::default();
        };
        let mut min = instance_position(first);
        let mut max = instance_position(first);
        for instance in instances.iter().skip(1) {
            let position = instance_position(instance);
            min = min.min(position);
            max = max.max(position);
        }
        Self { min, max }
    }
}

impl Default for PreviewBounds {
    fn default() -> Self {
        Self {
            min: Vec2::ZERO,
            max: Vec2::new(1.0, 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PreviewCamera {
    pan: Vec2,
    zoom: f32,
}

impl PreviewCamera {
    fn fit(bounds: PreviewBounds, size: PreviewSize) -> Self {
        let span = (bounds.max - bounds.min).max(Vec2::splat(1.0));
        let available = Vec2::new(size.width as f32, size.height as f32) * 0.82;
        let zoom = (available.x / span.x).min(available.y / span.y).max(1.0);
        Self {
            pan: (bounds.min + bounds.max) * 0.5,
            zoom,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreviewSize {
    width: u32,
    height: u32,
}

impl PreviewSize {
    fn clamp_to_max_dimension(self, max_dimension: u32) -> Self {
        Self {
            width: self.width.min(max_dimension),
            height: self.height.min(max_dimension),
        }
    }
}

struct PreviewRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    max_surface_dimension: u32,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    color_buffer: wgpu::Buffer,
    instance_capacity: usize,
    instance_count: usize,
    uploaded_revision: Option<u64>,
    uniform_revision: Option<u64>,
    uniform_size: Option<PreviewSize>,
    camera: PreviewCamera,
    color_scratch: Vec<PreviewColorGpu>,
}

impl PreviewRenderer {
    fn new(window: &Window) -> Result<Self, String> {
        block_on(Self::new_async(window))
    }

    async fn new_async(window: &Window) -> Result<Self, String> {
        let size = window_size(window)?;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window.clone())
            .map_err(|error| error.to_string())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|error| error.to_string())?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Preview Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|error| error.to_string())?;
        let max_surface_dimension = device.limits().max_texture_dimension_2d;
        let size = size.clamp_to_max_dimension(max_surface_dimension);
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .or_else(|| capabilities.formats.first().copied())
            .ok_or_else(|| "Preview surface has no supported formats.".to_string())?;
        let present_mode = if capabilities
            .present_modes
            .iter()
            .any(|mode| matches!(mode, wgpu::PresentMode::Fifo))
        {
            wgpu::PresentMode::Fifo
        } else {
            capabilities
                .present_modes
                .first()
                .copied()
                .ok_or_else(|| "Preview surface has no present modes.".to_string())?
        };
        let alpha_mode = capabilities
            .alpha_modes
            .first()
            .copied()
            .unwrap_or(wgpu::CompositeAlphaMode::Opaque);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width,
            height: size.height,
            present_mode,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: Vec::new(),
        };
        surface.configure(&device, &config);

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Preview Uniforms"),
            contents: bytemuck::bytes_of(&PreviewUniforms::default()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Preview Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Preview Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Preview Shader"),
            source: wgpu::ShaderSource::Wgsl(PREVIEW_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Preview Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Preview Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[PreviewInstanceGpu::layout(), PreviewColorGpu::layout()],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let instance_buffer = empty_instance_buffer(&device, "Preview Instances");
        let color_buffer = empty_instance_buffer(&device, "Preview Colors");

        Ok(Self {
            surface,
            device,
            queue,
            max_surface_dimension,
            config,
            pipeline,
            bind_group,
            uniform_buffer,
            instance_buffer,
            color_buffer,
            instance_capacity: 1,
            instance_count: 0,
            uploaded_revision: None,
            uniform_revision: None,
            uniform_size: None,
            camera: PreviewCamera::fit(PreviewBounds::default(), size),
            color_scratch: Vec::new(),
        })
    }

    fn render(
        &mut self,
        size: PreviewSize,
        scene: Option<&PreviewScene>,
        frame: Option<&dawn_runtime::RenderedFrame>,
    ) {
        let size = size.clamp_to_max_dimension(self.max_surface_dimension);
        if size.width == 0 || size.height == 0 {
            return;
        }
        if self.config.width != size.width || self.config.height != size.height {
            self.resize(size);
        }

        if let Some(scene) = scene {
            self.update_scene(scene, frame, size);
        } else {
            self.instance_count = 0;
            self.uploaded_revision = None;
            self.uniform_revision = None;
        }

        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(output)
            | wgpu::CurrentSurfaceTexture::Suboptimal(output) => output,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return,
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Preview Render Encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Preview Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(CLEAR_COLOR),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if self.instance_count > 0 {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
                pass.set_vertex_buffer(1, self.color_buffer.slice(..));
                pass.draw(0..6, 0..self.instance_count as u32);
            }
        }
        self.queue.submit([encoder.finish()]);
        output.present();
    }

    fn resize(&mut self, size: PreviewSize) {
        let size = size.clamp_to_max_dimension(self.max_surface_dimension);
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
        self.uniform_size = None;
    }

    fn update_scene(
        &mut self,
        scene: &PreviewScene,
        frame: Option<&dawn_runtime::RenderedFrame>,
        size: PreviewSize,
    ) {
        if self.uploaded_revision != Some(scene.revision) {
            self.upload_scene(scene);
        }
        if self.uniform_revision != Some(scene.revision) || self.uniform_size != Some(size) {
            self.update_uniforms(scene, size);
        }
        self.update_colors(scene, frame);
    }

    fn upload_scene(&mut self, scene: &PreviewScene) {
        self.instance_count = scene.instances.len();
        self.ensure_instance_capacity(scene.instances.len());
        if !scene.instances.is_empty() {
            self.queue.write_buffer(
                &self.instance_buffer,
                0,
                bytemuck::cast_slice(&scene.instances),
            );
        }
        self.uploaded_revision = Some(scene.revision);
    }

    fn update_uniforms(&mut self, scene: &PreviewScene, size: PreviewSize) {
        self.camera = PreviewCamera::fit(scene.bounds, size);
        let uniforms = PreviewUniforms {
            screen_zoom_min_radius: [size.width as f32, size.height as f32, self.camera.zoom, 2.0],
            pan: [self.camera.pan.x, self.camera.pan.y, 0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        self.uniform_revision = Some(scene.revision);
        self.uniform_size = Some(size);
    }

    fn update_colors(&mut self, scene: &PreviewScene, frame: Option<&dawn_runtime::RenderedFrame>) {
        self.color_scratch.clear();
        self.color_scratch
            .resize(scene.instances.len(), PreviewColorGpu::black());
        if let Some(frame) = frame {
            for fixture in &frame.fixtures {
                let Some(range) = scene.fixture_ranges.get(&fixture.fixture_id) else {
                    continue;
                };
                for (target, color) in self.color_scratch[range.clone()]
                    .iter_mut()
                    .zip(fixture.pixels.iter())
                {
                    *target = PreviewColorGpu::from_color(*color);
                }
            }
        }
        if !self.color_scratch.is_empty() {
            self.queue.write_buffer(
                &self.color_buffer,
                0,
                bytemuck::cast_slice(&self.color_scratch),
            );
        }
    }

    fn ensure_instance_capacity(&mut self, needed: usize) {
        if needed <= self.instance_capacity {
            return;
        }
        let capacity = needed.next_power_of_two();
        self.instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Preview Instances"),
            size: (capacity * std::mem::size_of::<PreviewInstanceGpu>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.color_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Preview Colors"),
            size: (capacity * std::mem::size_of::<PreviewColorGpu>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.instance_capacity = capacity;
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct PreviewUniforms {
    screen_zoom_min_radius: [f32; 4],
    pan: [f32; 4],
}

impl Default for PreviewUniforms {
    fn default() -> Self {
        Self {
            screen_zoom_min_radius: [1.0, 1.0, 1.0, 2.0],
            pan: [0.0, 0.0, 0.0, 0.0],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct PreviewInstanceGpu {
    center_radius: [f32; 4],
}

impl PreviewInstanceGpu {
    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 0,
                shader_location: 0,
            }],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct PreviewColorGpu {
    color: [f32; 4],
}

impl PreviewColorGpu {
    fn black() -> Self {
        Self {
            color: [0.0, 0.0, 0.0, 1.0],
        }
    }

    fn from_color(color: dawn_language::values::Color) -> Self {
        Self {
            color: [
                f32::from(color.red) / 255.0,
                f32::from(color.green) / 255.0,
                f32::from(color.blue) / 255.0,
                1.0,
            ],
        }
    }

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 0,
                shader_location: 1,
            }],
        }
    }
}

fn run_preview_loop(
    app: AppHandle,
    window: Window,
    mut renderer: PreviewRenderer,
    running: Arc<AtomicBool>,
) {
    let mut stats = PreviewFrameStats::new();
    let mut cached_scene: Option<PreviewScene> = None;
    let mut last_render_key: Option<PreviewRenderKey> = None;
    let mut reported_render_error = false;
    while running.load(Ordering::Acquire) {
        let size = match window_size(&window) {
            Ok(size) => size,
            Err(_) => break,
        };

        let state = app.state::<crate::state::DesktopState>();
        match state.preview_scene_revision() {
            Some(revision)
                if cached_scene
                    .as_ref()
                    .is_none_or(|scene| scene.revision != revision) =>
            {
                cached_scene = state.preview_scene();
            }
            Some(_) => {}
            None => cached_scene = None,
        }

        let clock = match state.active_preview_render_identity() {
            Ok(clock) => Some(clock),
            Err(ShowRenderError::NoRenderSession | ShowRenderError::ClockUnavailable { .. }) => {
                None
            }
            Err(ShowRenderError::Render(_)) => None,
        };
        let render_key = PreviewRenderKey::new(size, cached_scene.as_ref(), clock.as_ref());
        if last_render_key.as_ref() != Some(&render_key) {
            let frame = match state.render_current_sequence_frame() {
                Ok(rendered) => {
                    if reported_render_error {
                        state.clear_render_error_if_set();
                        reported_render_error = false;
                    }
                    Some(rendered.frame)
                }
                Err(
                    ShowRenderError::NoRenderSession | ShowRenderError::ClockUnavailable { .. },
                ) => None,
                Err(ShowRenderError::Render(error)) => {
                    state.set_render_error_if_changed(format!("Render failed: {error:?}"));
                    reported_render_error = true;
                    None
                }
            };
            renderer.render(size, cached_scene.as_ref(), frame.as_ref());
            last_render_key = Some(render_key);
            if let Some(fps) = stats.record_frame() {
                let _ = window.set_title(&format!("Dawn Preview - {fps:.0} FPS"));
            }
        }
        std::thread::sleep(preview_sleep_duration(clock.as_ref()));
    }
}

#[derive(Clone, Debug, PartialEq)]
struct PreviewRenderKey {
    size: PreviewSize,
    scene_revision: Option<u64>,
    clock: Option<PreviewClockKey>,
}

impl PreviewRenderKey {
    fn new(
        size: PreviewSize,
        scene: Option<&PreviewScene>,
        clock: Option<&AudioClockRenderIdentity>,
    ) -> Self {
        Self {
            size,
            scene_revision: scene.map(|scene| scene.revision),
            clock: clock.map(PreviewClockKey::new),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct PreviewClockKey {
    session_generation: u64,
    audio_generation: u32,
    audio_state: AudioTransportState,
    frame_rate: u32,
    frame_count: u64,
    frame_index: u64,
    paused_position_bits: Option<u64>,
}

impl PreviewClockKey {
    fn new(clock: &AudioClockRenderIdentity) -> Self {
        let paused_position_bits = if matches!(clock.audio_state, AudioTransportState::Playing) {
            None
        } else {
            Some(clock.position_seconds.to_bits())
        };
        Self {
            session_generation: clock.session_generation,
            audio_generation: clock.audio_generation,
            audio_state: clock.audio_state.clone(),
            frame_rate: clock.frame_rate,
            frame_count: clock.frame_count,
            frame_index: clock.frame_index,
            paused_position_bits,
        }
    }
}

fn preview_sleep_duration(clock: Option<&AudioClockRenderIdentity>) -> Duration {
    const IDLE_POLL: Duration = Duration::from_millis(100);
    const MIN_PLAYING_SLEEP: Duration = Duration::from_millis(1);
    let Some(clock) = clock else {
        return IDLE_POLL;
    };
    if !matches!(clock.audio_state, AudioTransportState::Playing) || clock.frame_rate == 0 {
        return IDLE_POLL;
    }
    let next_frame_seconds =
        (clock.frame_index.saturating_add(1)) as f64 / f64::from(clock.frame_rate);
    let delay_seconds = (next_frame_seconds - clock.position_seconds).max(0.0);
    let delay = Duration::from_secs_f64(delay_seconds);
    delay.clamp(MIN_PLAYING_SLEEP, IDLE_POLL)
}

struct PreviewFrameStats {
    window_started: Instant,
    frame_count: u32,
}

impl PreviewFrameStats {
    fn new() -> Self {
        Self {
            window_started: Instant::now(),
            frame_count: 0,
        }
    }

    fn record_frame(&mut self) -> Option<f64> {
        self.frame_count = self.frame_count.saturating_add(1);
        let elapsed = self.window_started.elapsed();
        if elapsed < Duration::from_secs(1) {
            return None;
        }
        let fps = f64::from(self.frame_count) / elapsed.as_secs_f64();
        self.window_started = Instant::now();
        self.frame_count = 0;
        Some(fps)
    }
}

fn window_size(window: &Window) -> Result<PreviewSize, String> {
    let size = window.inner_size().map_err(|error| error.to_string())?;
    Ok(PreviewSize {
        width: size.width.max(1),
        height: size.height.max(1),
    })
}

fn empty_instance_buffer(device: &wgpu::Device, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: 16,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn instance_position(instance: &PreviewInstanceGpu) -> Vec2 {
    Vec2::new(instance.center_radius[0], instance.center_radius[1])
}

fn emitters(geometry: &Geometry) -> Vec<Vec3> {
    match geometry {
        Geometry::Points { points } => points.iter().map(|point| point_vec3(*point)).collect(),
        Geometry::Lines { points, pixels } => line_emitters(points, *pixels),
        Geometry::Arc {
            center,
            radius,
            start_degrees,
            end_degrees,
            pixels,
        } => arc_emitters(*center, *radius, *start_degrees, *end_degrees, *pixels),
    }
}

fn line_emitters(points: &[Point3], pixels: u32) -> Vec<Vec3> {
    if points.is_empty() || pixels == 0 {
        return Vec::new();
    }
    if points.len() == 1 || pixels == 1 {
        return vec![point_vec3(points[0])];
    }
    let first = point_vec3(points[0]);
    let last = point_vec3(points[points.len() - 1]);
    (0..pixels)
        .map(|index| {
            let t = index as f32 / pixels.saturating_sub(1) as f32;
            first.lerp(last, t)
        })
        .collect()
}

fn arc_emitters(
    center: Point3,
    radius: DistanceSpan,
    start_degrees: f64,
    end_degrees: f64,
    pixels: u32,
) -> Vec<Vec3> {
    if pixels == 0 {
        return Vec::new();
    }
    let center = point_vec3(center);
    let radius_meters = distance_span_meters(radius) as f32;
    (0..pixels)
        .map(|index| {
            let t = if pixels == 1 {
                0.0
            } else {
                index as f64 / f64::from(pixels.saturating_sub(1))
            };
            let degrees = lerp(start_degrees, end_degrees, t);
            let radians = degrees.to_radians() as f32;
            Vec3::new(
                center.x + radius_meters * radians.cos(),
                center.y + radius_meters * radians.sin(),
                center.z,
            )
        })
        .collect()
}

fn point_vec3(point: Point3) -> Vec3 {
    Vec3::new(
        distance_meters(point.x) as f32,
        distance_meters(point.y) as f32,
        distance_meters(point.z) as f32,
    )
}

fn distance_meters(distance: Distance) -> f64 {
    distance.micrometers as f64 / 1_000_000.0
}

fn distance_span_meters(distance: DistanceSpan) -> f64 {
    distance.micrometers as f64 / 1_000_000.0
}

fn lerp(start: f64, end: f64, t: f64) -> f64 {
    start + (end - start) * t
}

const PREVIEW_SHADER: &str = r#"
struct Uniforms {
    screen_zoom_min_radius: vec4<f32>,
    pan: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct VertexInput {
    @builtin(vertex_index) vertex_index: u32,
    @location(0) center_radius: vec4<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
    );
    let local = corners[input.vertex_index];
    let screen = uniforms.screen_zoom_min_radius.xy;
    let zoom = uniforms.screen_zoom_min_radius.z;
    let min_radius = uniforms.screen_zoom_min_radius.w;
    let center = input.center_radius.xy;
    let radius = max(input.center_radius.z * zoom, min_radius);
    let pixel = vec2<f32>(
        screen.x * 0.5 + (center.x - uniforms.pan.x) * zoom,
        screen.y * 0.5 - (center.y - uniforms.pan.y) * zoom,
    ) + local * radius;

    var output: VertexOutput;
    output.position = vec4<f32>(
        pixel.x / screen.x * 2.0 - 1.0,
        1.0 - pixel.y / screen.y * 2.0,
        0.0,
        1.0,
    );
    output.local = local;
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    if (dot(input.local, input.local) > 1.0) {
        discard;
    }
    return input.color;
}
"#;
