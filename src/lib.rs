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
//!         .draw(|canvas, frame| {
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
//!         .on_draw(|state, canvas, frame| {
//!             state.frames = frame.index;
//!             canvas.fill_circle((400.0, 300.0), 80.0, colors::CRIMSON);
//!         })
//!         .run()
//! }
//! ```
//!
//! Coordinates are logical pixels with the origin at the top-left corner and
//! the y axis pointing down. The canvas is reset every frame; drawing is
//! immediate, so the callback should redraw the complete scene each time.
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
mod paint;
mod r2;
mod text;

pub use app::{App, Frame};
pub use canvas::Canvas;
pub use paint::{Paint, Stroke};
pub use r2::r2_rounded_rect_path;
pub use text::Font;

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
        ActiveEventLoop, App, Canvas, Color, ElementState, Font, Frame, Gradient, Key, KeyEvent,
        NamedKey, Paint, Stroke, Window, WindowEvent, WindowId, colors, kurbo, peniko,
        r2_rounded_rect_path, winit,
    };
}
