//! Window creation, GPU setup, and the continuous vsync render loop.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use vello_gpu::{
    ClearSettings, RenderSize, RenderTargetConfig, Renderer, TargetInit, TextureBindings,
};
use wgpu::CurrentSurfaceTexture;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::{Canvas, Color, DpiMode};

type EventCallback<S> = Box<
    dyn for<'a, 'b, 'c> FnMut(&mut S, &'a WindowEvent, &'b ActiveEventLoop, &'c Window) + 'static,
>;
type DrawCallback<S> = Box<dyn FnMut(&mut S, &mut Canvas, Frame) + 'static>;

/// Timing information for one rendered frame.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// Time elapsed since the application started.
    pub time: Duration,
    /// Time elapsed since the previous render callback started.
    ///
    /// This is `Duration::ZERO` for the first frame, and for the first frame
    /// after the window was minimized, occluded, or otherwise paused. Resizing
    /// the window, moving it between monitors, changing DPI, or a transient
    /// surface acquisition failure does not reset it.
    pub delta: Duration,
    /// Zero-based index of this rendered frame.
    pub index: u64,
}

/// A window that continuously redraws using Vello GPU.
///
/// Rendering is paced by the surface's vertical synchronization. No fixed
/// frame rate, sleeps, or manual frame limiter are applied.
///
/// The window is created with a logical inner size. How canvas coordinates map
/// to physical pixels is selected with [`App::dpi_mode`] and defaults to
/// [`DpiMode::Auto`].
///
/// The optional state parameter is owned by the application and mutably passed
/// to the event and draw callbacks.
pub struct App<S = ()> {
    title: String,
    width: u32,
    height: u32,
    background: Color,
    dpi_mode: DpiMode,
    state: S,
    on_event: Option<EventCallback<S>>,
    on_draw: Option<DrawCallback<S>>,
    close_on_request: bool,
}

impl App<()> {
    /// Creates an application with a logical window size.
    ///
    /// The window is always created at this logical size, independent of the
    /// selected [`DpiMode`]. The canvas coordinate size depends on the mode:
    /// with the default [`DpiMode::Auto`] it equals this size, while fixed
    /// scales change it according to the physical size of the window.
    pub fn new(title: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            title: title.into(),
            width: width.max(1),
            height: height.max(1),
            background: Color::WHITE,
            dpi_mode: DpiMode::Auto,
            state: (),
            on_event: None,
            on_draw: None,
            close_on_request: false,
        }
    }

    /// Attaches user-owned mutable state for the callbacks.
    ///
    /// Call this before registering `on_event`, `on_draw`, or `draw`; any
    /// callbacks already registered for `()` are discarded. The configured DPI
    /// mode is preserved.
    pub fn state<S: 'static>(self, state: S) -> App<S> {
        App {
            title: self.title,
            width: self.width,
            height: self.height,
            background: self.background,
            dpi_mode: self.dpi_mode,
            state,
            on_event: None,
            on_draw: None,
            close_on_request: false,
        }
    }

    /// Registers the stateless drawing callback and returns the builder.
    ///
    /// This convenience entry responds to `CloseRequested` by exiting the
    /// event loop. Use [`App::on_draw`] with a state if you need custom close
    /// handling.
    pub fn draw<F>(mut self, mut draw: F) -> Self
    where
        F: FnMut(&mut Canvas, Frame) + 'static,
    {
        self.on_draw = Some(Box::new(
            move |_state: &mut (), canvas: &mut Canvas, frame: Frame| {
                draw(canvas, frame);
            },
        ));
        self.close_on_request = true;
        self
    }
}

impl<S: 'static> App<S> {
    /// Sets the color used to clear the window before each frame.
    pub fn background(mut self, color: impl Into<Color>) -> Self {
        self.background = color.into();
        self
    }

    /// Selects how canvas coordinates map to physical pixels.
    ///
    /// The default is [`DpiMode::Auto`], which follows the window's scale
    /// factor and keeps canvas coordinates in logical pixels. Use
    /// [`DpiMode::Fixed`] to force a scale independent of the monitor, for
    /// example `DpiMode::Fixed(1.0)` for one canvas unit per physical pixel.
    ///
    /// The window is still created with the logical size passed to
    /// [`App::new`]; changing the mode only changes the canvas coordinate
    /// space. See [`DpiMode`] for the resulting canvas sizes.
    pub fn dpi_mode(mut self, mode: DpiMode) -> Self {
        self.dpi_mode = mode;
        self
    }

    /// Registers a raw winit window-event handler.
    ///
    /// The handler is called before the library performs its own lifecycle
    /// handling. It receives the user state, the raw [`WindowEvent`], the
    /// winit event loop, and the application window.
    ///
    /// Closing is not handled by default here. Call `event_loop.exit()` when a
    /// close request should exit the application.
    pub fn on_event<F>(mut self, handler: F) -> Self
    where
        F: for<'a, 'b, 'c> FnMut(&mut S, &'a WindowEvent, &'b ActiveEventLoop, &'c Window)
            + 'static,
    {
        self.on_event = Some(Box::new(handler));
        self
    }

    /// Registers a stateful drawing callback.
    ///
    /// This entry does not handle `CloseRequested`; use [`App::on_event`] to
    /// implement a custom close policy.
    pub fn on_draw<F>(mut self, draw: F) -> Self
    where
        F: FnMut(&mut S, &mut Canvas, Frame) + 'static,
    {
        self.on_draw = Some(Box::new(draw));
        self.close_on_request = false;
        self
    }

    /// Runs the event loop.
    ///
    /// A draw callback must have been registered with [`App::draw`] or
    /// [`App::on_draw`].
    pub fn run(self) -> Result<()> {
        if self.on_draw.is_none() {
            return Err(anyhow!(
                "no draw callback registered; call .draw(...) or .on_draw(...)"
            ));
        }

        let event_loop = EventLoop::new().context("failed to create the winit event loop")?;
        event_loop.set_control_flow(ControlFlow::Poll);

        let mut runner = Runner {
            app: self,
            state: None,
            error: None,
            clock: FrameClock::new(),
            next_frame: 0,
            perf: stats_enabled().then(PerfStats::default),
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

struct Runner<S> {
    app: App<S>,
    state: Option<State>,
    error: Option<anyhow::Error>,
    clock: FrameClock,
    next_frame: u64,
    perf: Option<PerfStats>,
}

/// Monotonic timing for draw callbacks.
///
/// `previous` only advances on frames that actually reach the draw callback.
/// Resizes, DPI changes, and transient surface errors therefore do not turn
/// the next frame's delta into zero. Only an explicit pause (initial show,
/// minimize, or occlusion) resets delta; `time` never resets.
struct FrameClock {
    start: Instant,
    previous: Option<Instant>,
}

impl FrameClock {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            previous: None,
        }
    }

    fn pause(&mut self) {
        self.previous = None;
    }

    fn tick(&mut self, now: Instant) -> (Duration, Duration) {
        let time = now.saturating_duration_since(self.start);
        let delta = self.previous.map_or(Duration::ZERO, |previous| {
            now.saturating_duration_since(previous)
        });
        self.previous = Some(now);
        (time, delta)
    }
}

#[derive(Default)]
struct PerfStats {
    frames: u32,
    zero_deltas: u32,
    draw: Duration,
    render: Duration,
    submit: Duration,
    since: Option<Instant>,
}

impl PerfStats {
    fn record(&mut self, delta: Duration, draw: Duration, render: Duration, submit: Duration) {
        self.frames += 1;
        self.zero_deltas += u32::from(delta.is_zero());
        self.draw += draw;
        self.render += render;
        self.submit += submit;
        let Some(since) = self.since else {
            self.since = Some(Instant::now());
            return;
        };
        if since.elapsed() < Duration::from_secs(1) {
            return;
        }

        let frames = f64::from(self.frames);
        let elapsed = since.elapsed().as_secs_f64();
        eprintln!(
            "plasticism stats: {:>6.1} fps | draw {:>6.2} ms | vello {:>6.2} ms | submit+present {:>6.2} ms | zero-dt {}",
            frames / elapsed,
            self.draw.as_secs_f64() * 1000.0 / frames,
            self.render.as_secs_f64() * 1000.0 / frames,
            self.submit.as_secs_f64() * 1000.0 / frames,
            self.zero_deltas,
        );
        self.frames = 0;
        self.zero_deltas = 0;
        self.draw = Duration::ZERO;
        self.render = Duration::ZERO;
        self.submit = Duration::ZERO;
        self.since = Some(Instant::now());
    }
}

fn stats_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("PLASTICISM_DRAW_STATS").is_some())
}

impl<S: 'static> ApplicationHandler for Runner<S> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        match State::new(event_loop, &self.app) {
            Ok(state) => {
                self.state = Some(state);
                self.clock.pause();
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

        if let Some(handler) = self.app.on_event.as_mut() {
            handler(&mut self.app.state, &event, event_loop, &state.window);
        }

        // The stateless `.draw(...)` convenience keeps the traditional
        // "clicking the close button exits" behavior. The stateful
        // `.on_draw(...)` entry deliberately has no default close policy.
        if self.app.close_on_request && matches!(event, WindowEvent::CloseRequested) {
            event_loop.exit();
            return;
        }

        match event {
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                // Do not reconfigure the surface synchronously inside the
                // window procedure. Windows can deliver a burst of WM_SIZE /
                // WM_DPICHANGED messages during an interactive resize; doing
                // GPU work here multiplies that work and can stall the modal
                // loop. `render` calls `sync_size` once, immediately before it
                // acquires the next surface texture.
                state.window.request_redraw();
            }
            WindowEvent::Occluded(occluded) => {
                if state.occluded != occluded {
                    state.occluded = occluded;
                    self.clock.pause();
                }
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
            self.clock.pause();
        }
    }
}

impl<S: 'static> Runner<S> {
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
            self.clock.pause();
            return Ok(());
        }

        let (output, suboptimal) = match state.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(texture) => (texture, false),
            CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            // A Vulkan swapchain can stop delivering images during an
            // interactive resize. wgpu reports that as a timeout after one
            // second; retrying alone leaves the window frozen forever. Reset
            // the swapchain so the next acquire has fresh images.
            CurrentSurfaceTexture::Timeout => {
                let first_timeout = state.acquire_timeouts == 0;
                state.acquire_timeouts = state.acquire_timeouts.saturating_add(1);
                if first_timeout {
                    state.configure_surface()?;
                } else {
                    state.acquire_timeouts = 0;
                    state.recreate_surface()?;
                }
                if stats_enabled() {
                    eprintln!(
                        "plasticism surface acquire timed out; swapchain {}",
                        if first_timeout {
                            "reconfigured"
                        } else {
                            "recreated"
                        }
                    );
                }
                state.window.request_redraw();
                return Ok(());
            }
            CurrentSurfaceTexture::Occluded => {
                state.window.request_redraw();
                return Ok(());
            }
            CurrentSurfaceTexture::Outdated => {
                state.configure_surface()?;
                state.acquire_timeouts = 0;
                state.window.request_redraw();
                return Ok(());
            }
            CurrentSurfaceTexture::Lost => {
                state.recreate_surface()?;
                state.acquire_timeouts = 0;
                state.window.request_redraw();
                return Ok(());
            }
            CurrentSurfaceTexture::Validation => {
                return Err(anyhow!("surface texture acquisition failed validation"));
            }
        };
        state.acquire_timeouts = 0;

        let (time, delta) = self.clock.tick(Instant::now());
        let frame = Frame {
            time,
            delta,
            index: self.next_frame,
        };
        self.next_frame = self.next_frame.wrapping_add(1);

        let width = u16::try_from(size.width)
            .context("window width exceeds the maximum render target width")?;
        let height = u16::try_from(size.height)
            .context("window height exceeds the maximum render target height")?;
        let scale_factor = self
            .app
            .dpi_mode
            .effective_scale(state.window.scale_factor());

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = state
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("plasticism-draw frame encoder"),
            });

        state.canvas.upload_pending_images(
            &mut state.renderer,
            &state.device,
            &state.queue,
            &mut encoder,
        );

        state.canvas.begin_frame(width, height, scale_factor);
        let draw_start = Instant::now();
        {
            let App {
                state: app_state,
                on_draw,
                ..
            } = &mut self.app;
            let draw = on_draw
                .as_mut()
                .ok_or_else(|| anyhow!("no draw callback registered"))?;
            draw(app_state, &mut state.canvas, frame);
        }
        state.canvas.finish_frame();
        let draw_time = draw_start.elapsed();

        let render_size = RenderSize { width, height };
        let (scene, resources) = state.canvas.parts();
        let render_start = Instant::now();
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
        let render_time = render_start.elapsed();

        // Release image atlas slots after the frame that may still reference
        // them has been recorded. New uploads on the next frame can then reuse
        // the slots safely.
        state
            .canvas
            .destroy_pending_images(&mut state.renderer, &mut encoder);

        let submit_start = Instant::now();
        state.queue.submit(Some(encoder.finish()));
        state.queue.present(output);
        let submit_time = submit_start.elapsed();

        if suboptimal {
            state.configure_surface()?;
        }

        if let Some(perf) = &mut self.perf {
            perf.record(delta, draw_time, render_time, submit_time);
        }

        // Keep the animation alive from inside the redraw callback. During a
        // Windows modal move/resize loop, `about_to_wait` may not run, but this
        // request causes WM_PAINT to post the next redraw.
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
    acquire_timeouts: u32,
}

impl State {
    fn new<S>(event_loop: &ActiveEventLoop, app: &App<S>) -> Result<Self> {
        let attributes = Window::default_attributes()
            .with_title(&app.title)
            .with_inner_size(LogicalSize::new(app.width, app.height));
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .context("failed to create the application window")?,
        );

        // Let wgpu choose its default backend order. If a Vulkan swapchain
        // times out during interactive resizing, the render loop resets the
        // swapchain instead of forcing a different backend.
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
        if stats_enabled() {
            let info = adapter.get_info();
            eprintln!(
                "plasticism adapter: {:?} · {} · {}",
                info.backend, info.name, info.driver
            );
        }

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
        let canvas = Canvas::new(
            resources,
            width,
            height,
            app.dpi_mode.effective_scale(window.scale_factor()),
        );

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
            acquire_timeouts: 0,
        })
    }

    /// Applies a pending window size once per frame.
    ///
    /// This is deliberately called from `render`, not from `WM_SIZE` handling,
    /// so a burst of resize events cannot trigger repeated GPU surface and
    /// depth-texture reallocations inside the Windows modal resize loop.
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{App, FrameClock};
    use crate::DpiMode;

    #[test]
    fn dpi_mode_defaults_to_auto() {
        let app = App::new("test", 640, 480);
        assert_eq!(app.dpi_mode, DpiMode::Auto);
    }

    #[test]
    fn dpi_mode_is_preserved_by_state() {
        let app = App::new("test", 640, 480)
            .dpi_mode(DpiMode::Fixed(1.0))
            .state(0_u32);
        assert_eq!(app.dpi_mode, DpiMode::Fixed(1.0));
    }

    #[test]
    fn dpi_mode_can_be_set_after_callbacks() {
        let app = App::new("test", 640, 480)
            .draw(|_canvas, _frame| {})
            .dpi_mode(DpiMode::Fixed(2.0));
        assert_eq!(app.dpi_mode, DpiMode::Fixed(2.0));
    }

    #[test]
    fn frame_clock_only_resets_delta_on_explicit_pause() {
        let start = Instant::now();
        let mut clock = FrameClock {
            start,
            previous: None,
        };

        assert_eq!(clock.tick(start), (Duration::ZERO, Duration::ZERO));

        let first = start + Duration::from_millis(16);
        assert_eq!(
            clock.tick(first),
            (Duration::from_millis(16), Duration::from_millis(16))
        );

        // A delayed or skipped frame still advances delta by the real elapsed
        // time instead of reporting zero.
        let delayed = start + Duration::from_millis(40);
        assert_eq!(
            clock.tick(delayed),
            (Duration::from_millis(40), Duration::from_millis(24))
        );

        // Pausing for minimize/occlusion resets delta, but not time.
        clock.pause();
        let resumed = start + Duration::from_millis(48);
        assert_eq!(
            clock.tick(resumed),
            (Duration::from_millis(48), Duration::ZERO)
        );

        let after = start + Duration::from_millis(64);
        assert_eq!(
            clock.tick(after),
            (Duration::from_millis(64), Duration::from_millis(16))
        );
    }
}
