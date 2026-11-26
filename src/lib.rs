//! A small, immediate-mode drawing library built on
//! [winit](https://crates.io/crates/winit) and
//! [vello_gpu](https://crates.io/crates/vello_gpu).
//!
//! Applications create an [`App`], attach optional mutable state, register
//! chainable event and drawing callbacks, and call [`App::run`]:
//!
//! ```no_run
//! use plasticism_draw::prelude::*;
//!
//! fn main() -> anyhow::Result<()> {
//!     App::new("plasticism", 800, 600)
//!         .background(colors::WHITE)
//!         .draw(|canvas, frame, _window| {
//!             let t = frame.time.as_secs_f64();
//!             canvas.fill_circle(
//!                 (400.0 + t.cos() * 100.0, 300.0),
//!                 80.0,
//!                 colors::CRIMSON,
//!             );
//!         })
//!         .run()
//! }
//! ```
//!
//! For applications with state and raw winit event handling:
//!
//! ```no_run
//! use plasticism_draw::prelude::*;
//!
//! struct Demo {
//!     frames: u64,
//! }
//!
//! fn main() -> anyhow::Result<()> {
//!     App::new("plasticism", 800, 600)
//!         .state(Demo { frames: 0 })
//!         .on_event(|state, event, event_loop, _window| {
//!             if matches!(event, WindowEvent::CloseRequested) {
//!                 event_loop.exit();
//!             }
//!         })
//!         .on_draw(|state, canvas, frame, _window| {
//!             state.frames = frame.index;
//!             canvas.fill_circle((400.0, 300.0), 80.0, colors::CRIMSON);
//!         })
//!         .run()
//! }
//! ```
//!
//! Both callbacks receive the same [`Window`] instance, so a draw callback can
//! read or adjust window state such as the title, scale factor, or focus
//! without storing the window separately.
//!
//! Coordinates are logical pixels by default, with the origin at the top-left
//! corner and the y axis pointing down. DPI handling is automatic: the scene is
//! scaled by the window's scale factor. Use [`App::dpi_mode`] to select
//! [`DpiMode::Fixed`] and render in physical pixels or at an explicit scale
//! instead. The canvas is reset every frame; drawing is immediate, so the
//! callback should redraw the complete scene each time.
//!
//! Windows show the system title bar by default. [`App::frameless`] removes it
//! while keeping the native top-level window behavior: on Windows, system
//! minimize, maximize, restore, and snap animations still run, and the window
//! keeps its drop shadow. The application then draws and handles its own title
//! bar; see the `frameless` example.
//!
//! Windows are opaque by default. [`App::transparent`] enables per-pixel alpha:
//! the frame is cleared to a transparent background, every paint blends into
//! whatever is behind the window, and pixels that are never painted stay fully
//! transparent. There is no separate mask or alpha buffer; alpha is just the
//! alpha of the [`App::background`] clear and of the painted colors, including
//! [`Canvas::with_opacity`] groups. Antialiased edges and rounded corners fade
//! into the desktop, and a hole is made by clipping or by an even-odd path,
//! because source-over blending cannot erase pixels. Per-pixel alpha is a
//! presentation feature: on Windows the DX12 and OpenGL backends do not expose
//! it, so an adapter that can present premultiplied alpha (in practice Vulkan)
//! is selected; without one the window stays opaque and a warning is printed.
//! Transparent areas still receive input; see
//! [`Window::set_cursor_hittest`](winit::window::Window::set_cursor_hittest).
//!
//! Bitmaps can be loaded with [`Image`] and drawn scaled with
//! [`Canvas::draw_image`] or [`Canvas::draw_image_fit`]. [`VectorIcon`] parses
//! SVG path data or complete SVG documents, and [`Canvas::draw_icon`] fills the
//! resulting geometry with any [`Paint`]. [`Canvas::draw_bitmap_icon`] uses a
//! bitmap's alpha channel as a mask for a solid tint color.
//!
//! Shapes can cast soft drop shadows. [`Shadow`] describes an offset, a blur
//! radius in the `box-shadow` sense, and a color. [`Canvas::draw_shadow`] draws
//! only the shadow, so the shape is painted afterwards on top of it:
//!
//! ```no_run
//! # use plasticism_draw::prelude::*;
//! # fn demo(c: &mut Canvas) {
//! let card = Rect::from_origin_size((40.0, 40.0), (240.0, 160.0));
//! c.draw_shadow(card, Shadow::default());
//! c.fill_rect(card, colors::WHITE);
//! # }
//! ```
//!
//! Text is shaped and laid out by parley. [`Font`] loads a face and registers
//! its family for layout with automatic system-font fallback.
//! [`TextStyle`] selects size, weight, italic/oblique, width, variable axes,
//! OpenType features, line height, spacing, alignment, and wrapping.
//! [`Font::layout`] returns a cached [`TextLayout`], while [`RichText`] adds
//! per-range styles, colors, and font families. Draw with
//! [`Canvas::fill_text_layout`] or [`Canvas::stroke_text_layout`]; metrics are
//! available from [`FontMetrics`], [`TextMetrics`], [`LineMetrics`], and
//! [`RunMetrics`].
//!
//! Rendering is continuous and paced by vertical synchronization. There is no
//! fixed frame rate, and [`Frame::delta`] always contains the real time since
//! the previous callback.
//!
//! The stateless `.draw(...)` shortcut keeps the traditional "close button
//! exits" behavior. The stateful `.on_draw(...)` entry does not handle
//! `CloseRequested`; implement a custom close policy in `.on_event(...)`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod app;
mod canvas;
mod dpi;
mod icon;
mod image;
mod paint;
mod r2;
mod shadow;
mod text;

pub use app::{App, Frame};
pub use canvas::Canvas;
pub use dpi::DpiMode;
pub use icon::VectorIcon;
pub use image::{Image, ImageFit};
pub use paint::{Paint, Stroke};
pub use r2::{R2RoundedRect, r2_rounded_rect_path};
pub use shadow::Shadow;
pub use text::{
    Font, FontFeature, FontMetrics, FontStyle, FontVariation, FontWeight, FontWidth, GenericFamily,
    LineMetrics, RichText, RunMetrics, Tag, TextAlign, TextLayout, TextMetrics, TextStyle,
};

pub use vello_gpu::kurbo;
pub use vello_gpu::peniko;
pub use vello_gpu::peniko::Gradient;
pub use vello_gpu::peniko::color::palette::css as colors;
pub use vello_gpu::peniko::{Color, Fill};

pub use winit;
pub use winit::event::{ElementState, KeyEvent, WindowEvent};
pub use winit::event_loop::ActiveEventLoop;
pub use winit::keyboard::{Key, NamedKey};
pub use winit::window::{Window, WindowId};

pub use kurbo::{
    Affine, BezPath, Cap, Circle, Ellipse, Join, Line, Point, Rect, RoundedRect, RoundedRectRadii,
    Shape, Size, Vec2,
};

/// Commonly used types, geometry helpers, winit event types, and the CSS color palette.
pub mod prelude {
    pub use crate::Fill;
    pub use crate::kurbo::{
        Affine, BezPath, Cap, Circle, Ellipse, Join, Line, Point, Rect, RoundedRect,
        RoundedRectRadii, Shape, Size, Vec2,
    };
    pub use crate::{
        ActiveEventLoop, App, Canvas, Color, DpiMode, ElementState, Font, FontFeature, FontMetrics,
        FontStyle, FontVariation, FontWeight, FontWidth, Frame, GenericFamily, Gradient, Image,
        ImageFit, Key, KeyEvent, LineMetrics, NamedKey, Paint, R2RoundedRect, RichText, RunMetrics,
        Shadow, Stroke, Tag, TextAlign, TextLayout, TextMetrics, TextStyle, VectorIcon, Window,
        WindowEvent, WindowId, colors, kurbo, peniko, r2_rounded_rect_path, winit,
    };
}
