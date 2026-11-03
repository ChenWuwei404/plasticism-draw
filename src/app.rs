//! Window creation, GPU setup, and the continuous vsync render loop.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use vello_gpu::{
    ClearSettings, RenderSize, RenderTargetConfig, Renderer, TargetInit, TextureBindings,
};
use wgpu::CurrentSurfaceTexture;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::{Canvas, Color};

/// Timing information for one rendered frame.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// Time elapsed since the application started.
    pub time: Duration,
    /// Time elapsed since the previous render callback started.
    ///
    /// This is `Duration::ZERO` for the first frame, and for the first frame
    /// after the window was minimized, occluded, or otherwise paused.
    pub delta: Duration,
    /// Zero-based index of this rendered frame.
    pub index: u64,
}

/// A window that continuously redraws using Vello GPU.
///
/// Rendering is paced by the surface's vertical synchronization. No fixed
/// frame rate, sleeps, or manual frame limiter are applied.
pub struct App {
    title: String,
    width: u32,
    height: u32,
    background: Color,
}

impl App {
    /// Creates an application with a logical window size.
    ///
    /// The size is specified in logical pixels; the physical surface follows
    /// the window's current scale factor.
    pub fn new(title: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            title: title.into(),
            width: width.max(1),
            height: height.max(1),
            background: Color::WHITE,
        }
    }

    /// Sets the color used to clear the window before each frame.
    pub fn background(mut self, color: impl Into<Color>) -> Self {
        self.background = color.into();
        self
    }

    /// Runs the event loop and invokes `draw` for every vsync-paced frame.
    ///
    /// The callback receives the current [`Canvas`] and a [`Frame`] whose
    /// `delta` is the actual time since the previous callback.
    pub fn run<F>(self, draw: F) -> Result<()>
    where
        F: FnMut(&mut Canvas, Frame) + 'static,
    {
        let event_loop = EventLoop::new().context("failed to create the winit event loop")?;
        event_loop.set_control_flow(ControlFlow::Poll);

        let mut runner = Runner {
            app: self,
            draw,
            state: None,
            error: None,
            start: Instant::now(),
            last_frame_start: None,
            next_frame: 0,
        };

        event_loop
            .run_app(&mut runner)
            .context("the winit event loop terminated with an error")?;

        if let Some(error) = runner.error {
            Err(error)
        } else {
            Ok(())
        }
    }
}

struct Runner<F> {
    app: App,
    draw: F,
    state: Option<State>,
    error: Option<anyhow::Error>,
    start: Instant,
    last_frame_start: Option<Instant>,
    next_frame: u64,
}

impl<F> ApplicationHandler for Runner<F>
where
    F: FnMut(&mut Canvas, Frame) + 'static,
{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        match State::new(event_loop, &self.app) {
            Ok(state) => {
                self.state = Some(state);
                self.last_frame_start = None;
                event_loop.set_control_flow(ControlFlow::Poll);
                if let Some(state) = &self.state {
                    state.window.request_redraw();
                }
            }
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        if window_id != state.window.id() {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.last_frame_start = None;
                if let Err(error) = state.resize(size) {
                    self.fail(event_loop, error);
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                self.last_frame_start = None;
                if let Err(error) = state.sync_size() {
                    self.fail(event_loop, error);
                }
            }
            WindowEvent::Occluded(occluded) => {
                state.occluded = occluded;
                self.last_frame_start = None;
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.render() {
                    self.fail(event_loop, error);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.error.is_some() {
            event_loop.exit();
            return;
        }

        let Some(state) = self.state.as_ref() else {
            return;
        };
        let size = state.window.inner_size();
        let visible = !state.occluded && size.width > 0 && size.height > 0;

        if visible {
            event_loop.set_control_flow(ControlFlow::Poll);
            state.window.request_redraw();
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
            self.last_frame_start = None;
        }
    }
}

impl<F> Runner<F>
where
    F: FnMut(&mut Canvas, Frame) + 'static,
{
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: anyhow::Error) {
        self.error = Some(error);
        event_loop.exit();
    }

    fn render(&mut self) -> Result<()> {
        let state = self
            .state
            .as_mut()
            .ok_or_else(|| anyhow!("render requested before graphics initialization"))?;

        state.sync_size()?;
        let size = state.window.inner_size();
        if size.width == 0 || size.height == 0 {
            self.last_frame_start = None;
            return Ok(());
        }

        let (output, suboptimal) = match state.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(texture) => (texture, false),
            CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => {
                self.last_frame_start = None;
                return Ok(());
            }
            CurrentSurfaceTexture::Outdated => {
                state.configure_surface()?;
                self.last_frame_start = None;
                return Ok(());
            }
            CurrentSurfaceTexture::Lost => {
                state.recreate_surface()?;
                self.last_frame_start = None;
                return Ok(());
            }
            CurrentSurfaceTexture::Validation => {
                return Err(anyhow!("surface texture acquisition failed validation"));
            }
        };

        let now = Instant::now();
        let delta = self.last_frame_start.map_or(Duration::ZERO, |previous| {
            now.saturating_duration_since(previous)
        });
        self.last_frame_start = Some(now);
        let frame = Frame {
            time: now.saturating_duration_since(self.start),
            delta,
            index: self.next_frame,
        };
        self.next_frame = self.next_frame.wrapping_add(1);

        let width = u16::try_from(size.width)
            .context("window width exceeds the maximum render target width")?;
        let height = u16::try_from(size.height)
            .context("window height exceeds the maximum render target height")?;
        let scale_factor = state.window.scale_factor();

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = state
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("plasticism-draw frame encoder"),
            });

        state.canvas.begin_frame(width, height, scale_factor);
        (self.draw)(&mut state.canvas, frame);
        state.canvas.finish_frame();

        let render_size = RenderSize { width, height };
        let (scene, resources) = state.canvas.parts();
        state
            .renderer
            .render(
                scene,
                resources,
                &state.device,
                &state.queue,
                &mut encoder,
                &render_size,
                &view,
                Some(&state.depth_view),
                &state.texture_bindings,
                TargetInit::Clear(ClearSettings::Viewport {
                    color: self.app.background,
                }),
            )
            .context("Vello GPU failed to render the scene")?;

        state.queue.submit(Some(encoder.finish()));
        state.queue.present(output);

        if suboptimal {
            state.configure_surface()?;
        }

        state.window.request_redraw();
        Ok(())
    }
}

struct State {
    window: Arc<Window>,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    canvas: Canvas,
    depth_view: wgpu::TextureView,
    texture_bindings: TextureBindings,
    occluded: bool,
}

impl State {
    fn new(event_loop: &ActiveEventLoop, app: &App) -> Result<Self> {
        let attributes = Window::default_attributes()
            .with_title(&app.title)
            .with_inner_size(LogicalSize::new(app.width, app.height));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .context("failed to create the application window")?,
        );

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance
            .create_surface(window.clone())
            .context("failed to create the wgpu surface")?;

        let request_adapter = |force_fallback_adapter| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter,
                compatible_surface: Some(&surface),
                ..Default::default()
            }))
        };
        let adapter = request_adapter(false)
            .or_else(|_| request_adapter(true))
            .context("failed to find a compatible GPU adapter")?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("plasticism-draw device"),
            ..Default::default()
        }))
        .context("failed to create the wgpu device")?;

        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Err(anyhow!("window has a zero-sized drawing surface"));
        }
        let width = u16::try_from(size.width)
            .context("window width exceeds the maximum render target width")?;
        let height = u16::try_from(size.height)
            .context("window height exceeds the maximum render target height")?;

        let capabilities = surface.get_capabilities(&adapter);
        if capabilities.formats.is_empty() {
            return Err(anyhow!("surface has no supported texture formats"));
        }
        // Vello GPU writes sRGB-encoded color values directly, so the
        // swapchain must use a non-sRGB format to avoid a second encoding.
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| !is_srgb(*format))
            .unwrap_or(capabilities.formats[0]);
        let alpha_mode = capabilities
            .alpha_modes
            .iter()
            .copied()
            .find(|mode| *mode == wgpu::CompositeAlphaMode::Opaque)
            .unwrap_or(wgpu::CompositeAlphaMode::Auto);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: Vec::new(),
        };
        surface.configure(&device, &config);

        let (renderer, resources) = Renderer::new(
            &device,
            &RenderTargetConfig {
                format,
                width,
                height,
            },
        );
        let depth_view =
            Renderer::create_depth_texture_view(&device, &RenderSize { width, height });
        let canvas = Canvas::new(resources, width, height, window.scale_factor());

        Ok(Self {
            window,
            instance,
            surface,
            device,
            queue,
            config,
            renderer,
            canvas,
            depth_view,
            texture_bindings: TextureBindings::new(),
            occluded: false,
        })
    }

    fn sync_size(&mut self) -> Result<()> {
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        if size.width == self.config.width && size.height == self.config.height {
            return Ok(());
        }

        u16::try_from(size.width)
            .context("window width exceeds the maximum render target width")?;
        u16::try_from(size.height)
            .context("window height exceeds the maximum render target height")?;

        self.config.width = size.width;
        self.config.height = size.height;
        self.configure_surface()?;
        self.depth_view = Renderer::create_depth_texture_view(
            &self.device,
            &RenderSize {
                width: u16::try_from(size.width).unwrap(),
                height: u16::try_from(size.height).unwrap(),
            },
        );
        Ok(())
    }

    fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) -> Result<()> {
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        if size.width == self.config.width && size.height == self.config.height {
            return Ok(());
        }

        u16::try_from(size.width)
            .context("window width exceeds the maximum render target width")?;
        u16::try_from(size.height)
            .context("window height exceeds the maximum render target height")?;

        self.config.width = size.width;
        self.config.height = size.height;
        self.configure_surface()?;
        self.depth_view = Renderer::create_depth_texture_view(
            &self.device,
            &RenderSize {
                width: u16::try_from(size.width).unwrap(),
                height: u16::try_from(size.height).unwrap(),
            },
        );
        self.window.request_redraw();
        Ok(())
    }

    fn configure_surface(&self) -> Result<()> {
        self.surface.configure(&self.device, &self.config);
        Ok(())
    }

    fn recreate_surface(&mut self) -> Result<()> {
        self.surface = self
            .instance
            .create_surface(self.window.clone())
            .context("failed to recreate the wgpu surface")?;
        self.configure_surface()
    }
}

fn is_srgb(format: wgpu::TextureFormat) -> bool {
    matches!(
        format,
        wgpu::TextureFormat::Rgba8UnormSrgb | wgpu::TextureFormat::Bgra8UnormSrgb
    )
}
