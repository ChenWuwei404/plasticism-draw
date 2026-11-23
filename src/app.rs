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
#[cfg(target_os = "windows")]
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{Window, WindowId};

use crate::{Canvas, Color, DpiMode};

type EventCallback<S> = Box<
    dyn for<'a, 'b, 'c> FnMut(&mut S, &'a WindowEvent, &'b ActiveEventLoop, &'c Window) + 'static,
>;
type DrawCallback<S> = Box<dyn for<'a> FnMut(&mut S, &mut Canvas, Frame, &'a Window) + 'static>;

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
/// to the event and draw callbacks. Both callbacks also receive the same
/// [`Window`] instance, so drawing code can inspect or configure the window
/// that delivered the events.
///
/// Windows are opaque unless [`App::transparent`] is enabled.
pub struct App<S = ()> {
    title: String,
    width: u32,
    height: u32,
    background: Option<Color>,
    transparent: bool,
    dpi_mode: DpiMode,
    frameless: bool,
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
            background: None,
            transparent: false,
            dpi_mode: DpiMode::Auto,
            frameless: false,
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
    /// mode and frameless setting are preserved.
    pub fn state<S: 'static>(self, state: S) -> App<S> {
        App {
            title: self.title,
            width: self.width,
            height: self.height,
            background: self.background,
            transparent: self.transparent,
            dpi_mode: self.dpi_mode,
            frameless: self.frameless,
            state,
            on_event: None,
            on_draw: None,
            close_on_request: false,
        }
    }

    /// Registers the stateless drawing callback and returns the builder.
    ///
    /// The callback receives the canvas, the frame timing, and the application
    /// window. That is the same [`Window`] instance that
    /// [`on_event`](App::on_event) receives, so a draw callback may read window
    /// state such as the title, the scale factor, or whether the window is
    /// focused.
    ///
    /// This convenience entry responds to `CloseRequested` by exiting the
    /// event loop. Use [`App::on_draw`] with a state if you need custom close
    /// handling.
    pub fn draw<F>(mut self, mut draw: F) -> Self
    where
        F: FnMut(&mut Canvas, Frame, &Window) + 'static,
    {
        self.on_draw = Some(Box::new(
            move |_state: &mut (), canvas: &mut Canvas, frame: Frame, window: &Window| {
                draw(canvas, frame, window);
            },
        ));
        self.close_on_request = true;
        self
    }
}

impl<S: 'static> App<S> {
    /// Sets the color used to clear the window before each frame.
    ///
    /// The alpha channel is honored. On a [`transparent`](App::transparent)
    /// window, a background whose alpha is below one lets the desktop show
    /// through everywhere the draw callback does not paint, and it shows
    /// through a partially transparent background itself.
    ///
    /// The default is white, or transparent black when
    /// [`transparent`](App::transparent) is enabled and no background was set.
    pub fn background(mut self, color: impl Into<Color>) -> Self {
        self.background = Some(color.into());
        self
    }

    /// Makes the window composite with per-pixel alpha, so whatever is behind
    /// the window shows through the transparent parts of the frame.
    ///
    /// Alpha is not a separate setting: every frame starts by clearing the
    /// canvas to the [`background`](App::background) color, and the draw
    /// callback then paints on top with source-over blending. The window's
    /// alpha at a pixel is the alpha of that composite:
    ///
    /// - A pixel that is never painted keeps the background alpha. With the
    ///   default background of a transparent window, which is fully
    ///   transparent, the desktop shows through completely there.
    /// - A paint with alpha, such as `colors::WHITE.with_alpha(0.35)`, blends
    ///   with the desktop behind the window, not only with earlier shapes.
    /// - [`Canvas::with_opacity`](crate::Canvas::with_opacity) multiplies the
    ///   alpha of a whole group of shapes.
    /// - Antialiased edges, including rounded corners and text, fade into the
    ///   desktop instead of into an opaque background color.
    ///
    /// Blending is source-over, so painting a transparent color cannot erase
    /// opaque pixels. To create a hole, clip the drawing with
    /// [`Canvas::with_clip`](crate::Canvas::with_clip), or fill an even-odd
    /// path whose inner subpath is the hole, so the area is never painted.
    ///
    /// Enabling this changes the default background from white to transparent;
    /// call [`background`](App::background) to override it.
    ///
    /// # Platform support
    ///
    /// Per-pixel alpha is a presentation feature, so it depends on the adapter.
    /// On Windows the DX12 and OpenGL backends only expose opaque surfaces, so
    /// this asks for an adapter that reports premultiplied-alpha support (in
    /// practice Vulkan). If no adapter can do it, the window is created opaque
    /// and a warning is printed once.
    ///
    /// Transparent windows still receive input in their transparent areas. Use
    /// [`Window::set_cursor_hittest`](winit::window::Window::set_cursor_hittest)
    /// on the window handed to the event callback to make the whole window
    /// click-through.
    pub fn transparent(mut self, transparent: bool) -> Self {
        self.transparent = transparent;
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

    /// Removes the system title bar while keeping the native window behavior.
    ///
    /// On Windows the window is still created as a normal top-level window. The
    /// native frame styles are kept and only the non-client area is removed, so
    /// the system minimize, maximize, restore, and snap animations keep
    /// working, and the window keeps its system drop shadow. The window can
    /// still be moved, resized, and restored like any other window.
    ///
    /// The trade-off is that the system title bar and its hit testing are gone.
    /// The application is responsible for drawing its own title bar and for
    /// handling dragging, minimizing, maximizing, closing, and resizing the
    /// window from its edges. See the `frameless` example for a complete custom
    /// title bar.
    ///
    /// On other platforms this creates a plain undecorated window with
    /// `WindowAttributes::with_decorations(false)`; window animations are then
    /// left to the platform compositor.
    ///
    /// This setting only affects window creation. It is off by default.
    ///
    /// A [`transparent`](App::transparent) window does not keep the drop
    /// shadow: DWM draws it along the rectangular frame, which would show as a
    /// hard square edge around otherwise transparent corners.
    pub fn frameless(mut self, frameless: bool) -> Self {
        self.frameless = frameless;
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
    /// The callback receives the user state, the canvas, the frame timing, and
    /// the application window. The window is the same [`Window`] instance that
    /// [`on_event`](App::on_event) receives, so drawing code and event handling
    /// can share window state without storing the window separately.
    ///
    /// This entry does not handle `CloseRequested`; use [`App::on_event`] to
    /// implement a custom close policy.
    pub fn on_draw<F>(mut self, draw: F) -> Self
    where
        F: FnMut(&mut S, &mut Canvas, Frame, &Window) + 'static,
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
            draw(app_state, &mut state.canvas, frame, &state.window);
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
                    color: state.background,
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
    background: Color,
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
            .with_inner_size(LogicalSize::new(app.width, app.height))
            .with_decorations(!app.frameless)
            .with_transparent(app.transparent);

        // An undecorated window keeps its drop shadow. winit implements
        // undecorated windows on Windows by keeping the native frame styles and
        // overriding `WM_NCCALCSIZE`, not by using `WS_POPUP`, so DWM
        // transitions and the system shadow remain available. A transparent
        // window must not ask for it: the shadow follows the rectangular frame
        // and would outline the invisible parts of the window.
        #[cfg(target_os = "windows")]
        let attributes = if app.frameless && !app.transparent {
            attributes.with_undecorated_shadow(true)
        } else {
            attributes
        };

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
        // Per-pixel alpha is a presentation capability of the adapter, and it
        // has to be decided before the surface is configured. Prefer an
        // adapter that can present premultiplied alpha when the application
        // asked for transparency, then fall back to the normal request.
        let adapter = if app.transparent {
            find_transparent_adapter(&instance, &surface)
        } else {
            None
        };
        let adapter = match adapter {
            Some(adapter) => adapter,
            None => request_adapter(false)
                .or_else(|_| request_adapter(true))
                .context("failed to find a compatible GPU adapter")?,
        };
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
        let alpha_mode = pick_alpha_mode(app.transparent, &capabilities.alpha_modes);
        if app.transparent && alpha_mode != wgpu::CompositeAlphaMode::PreMultiplied {
            eprintln!(
                "plasticism-draw: this adapter cannot present per-pixel alpha (available: {:?}); \
                 the window stays opaque",
                capabilities.alpha_modes
            );
        }

        // FIFO's get_current_texture can stall during Windows modal resizing
        // (the presentation queue stops draining). Mailbox avoids that wait.
        // Applications that want a hard frame cap can throttle their redraw
        // loop externally; this library intentionally imposes no cap.
        let present_mode = if capabilities
            .present_modes
            .contains(&wgpu::PresentMode::Mailbox)
        {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::AutoVsync
        };
        if stats_enabled() {
            eprintln!("plasticism present mode: {present_mode:?}");
        }

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: size.width,
            height: size.height,
            present_mode,
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

        // A transparent application that did not pick a background starts
        // fully transparent; otherwise the historical opaque white is kept.
        let background = resolve_background(app.background, app.transparent);

        Ok(Self {
            window,
            instance,
            surface,
            device,
            queue,
            config,
            background,
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

/// Finds an adapter whose surface can present premultiplied alpha.
///
/// Transparency is a presentation property, so the adapter has to be selected
/// before the surface is configured. Discrete GPUs are preferred, mirroring the
/// high-performance request used for opaque windows.
fn find_transparent_adapter(
    instance: &wgpu::Instance,
    surface: &wgpu::Surface<'_>,
) -> Option<wgpu::Adapter> {
    let mut candidates: Vec<wgpu::Adapter> =
        pollster::block_on(instance.enumerate_adapters(wgpu::Backends::PRIMARY))
            .into_iter()
            .filter(|adapter| adapter.is_surface_supported(surface))
            .filter(|adapter| {
                surface
                    .get_capabilities(adapter)
                    .alpha_modes
                    .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
            })
            .collect();
    candidates.sort_by_key(|adapter| adapter_rank(adapter.get_info().device_type));
    candidates.into_iter().next()
}

fn adapter_rank(device_type: wgpu::DeviceType) -> u8 {
    match device_type {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::VirtualGpu => 2,
        wgpu::DeviceType::Other => 3,
        wgpu::DeviceType::Cpu => 4,
    }
}

/// Picks the surface alpha mode for the window.
///
/// Per-pixel transparency needs premultiplied compositing; everything else
/// falls back to the opaque window the library has always used.
fn pick_alpha_mode(
    transparent: bool,
    available: &[wgpu::CompositeAlphaMode],
) -> wgpu::CompositeAlphaMode {
    if transparent && available.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
        return wgpu::CompositeAlphaMode::PreMultiplied;
    }
    available
        .iter()
        .copied()
        .find(|mode| *mode == wgpu::CompositeAlphaMode::Opaque)
        .unwrap_or(wgpu::CompositeAlphaMode::Auto)
}

/// Resolves the clear color for the frame.
///
/// An explicit [`App::background`] always wins. Otherwise a transparent window
/// clears to transparent black, while an opaque window keeps the historical
/// white.
fn resolve_background(background: Option<Color>, transparent: bool) -> Color {
    background.unwrap_or(if transparent {
        Color::TRANSPARENT
    } else {
        Color::WHITE
    })
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{App, FrameClock, pick_alpha_mode, resolve_background};
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
            .draw(|_canvas, _frame, _window| {})
            .dpi_mode(DpiMode::Fixed(2.0));
        assert_eq!(app.dpi_mode, DpiMode::Fixed(2.0));
    }

    #[test]
    fn frameless_defaults_to_false() {
        let app = App::new("test", 640, 480);
        assert!(!app.frameless);
    }

    #[test]
    fn frameless_is_preserved_by_state() {
        let app = App::new("test", 640, 480).frameless(true).state(0_u32);
        assert!(app.frameless);
    }

    #[test]
    fn frameless_can_be_set_after_callbacks() {
        let app = App::new("test", 640, 480)
            .draw(|_canvas, _frame, _window| {})
            .frameless(true);
        assert!(app.frameless);
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

    #[test]
    fn transparent_defaults_to_false() {
        let app = App::new("test", 640, 480);
        assert!(!app.transparent);
    }

    #[test]
    fn transparent_is_preserved_by_state() {
        let app = App::new("test", 640, 480).transparent(true).state(0_u32);
        assert!(app.transparent);
    }

    #[test]
    fn transparent_can_be_set_after_callbacks() {
        let app = App::new("test", 640, 480)
            .draw(|_canvas, _frame, _window| {})
            .transparent(true);
        assert!(app.transparent);
    }

    #[test]
    fn alpha_mode_prefers_premultiplied_only_for_transparent_windows() {
        let available = [
            wgpu::CompositeAlphaMode::Opaque,
            wgpu::CompositeAlphaMode::PreMultiplied,
        ];
        assert_eq!(
            pick_alpha_mode(false, &available),
            wgpu::CompositeAlphaMode::Opaque
        );
        assert_eq!(
            pick_alpha_mode(true, &available),
            wgpu::CompositeAlphaMode::PreMultiplied
        );
    }

    #[test]
    fn alpha_mode_falls_back_to_opaque_when_transparency_is_unavailable() {
        let available = [
            wgpu::CompositeAlphaMode::Opaque,
            wgpu::CompositeAlphaMode::Inherit,
        ];
        assert_eq!(
            pick_alpha_mode(true, &available),
            wgpu::CompositeAlphaMode::Opaque
        );
        // Without an opaque mode the surface is left to the platform default.
        assert_eq!(
            pick_alpha_mode(true, &[wgpu::CompositeAlphaMode::Inherit]),
            wgpu::CompositeAlphaMode::Auto
        );
    }

    #[test]
    fn background_follows_transparency_unless_set_explicitly() {
        assert_eq!(resolve_background(None, false), crate::Color::WHITE);
        assert_eq!(resolve_background(None, true), crate::Color::TRANSPARENT);

        let explicit = crate::Color::from_rgb8(10, 20, 30);
        assert_eq!(resolve_background(Some(explicit), false), explicit);
        assert_eq!(resolve_background(Some(explicit), true), explicit);
    }
}
