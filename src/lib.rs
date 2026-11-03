//! A small, immediate-mode drawing library built on
//! [winit](https://crates.io/crates/winit) and
//! [vello_gpu](https://crates.io/crates/vello_gpu).
//!
//! Applications create an [`App`], choose a background color, and pass a
//! drawing callback to [`App::run`]. The callback receives a [`Canvas`] and a
//! [`Frame`] containing the actual delta time since the previous frame:
//!
//! ```no_run
//! use plasticism_draw::prelude::*;
//!
//! fn main() -> anyhow::Result<()> {
//!     App::new("plasticism", 800, 600)
//!         .background(colors::WHITE)
//!         .run(|canvas, frame| {
//!             let t = frame.time.as_secs_f64();
//!             canvas.fill_circle(
//!                 (400.0 + t.cos() * 100.0, 300.0),
//!                 80.0,
//!                 colors::CRIMSON,
//!             );
//!         })
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

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod app;
mod canvas;
mod paint;
mod text;

pub use app::{App, Frame};
pub use canvas::Canvas;
pub use paint::{Paint, Stroke};
pub use text::Font;

pub use vello_gpu::kurbo;
pub use vello_gpu::peniko;
pub use vello_gpu::peniko::Gradient;
pub use vello_gpu::peniko::color::palette::css as colors;
pub use vello_gpu::peniko::{Color, Fill};

pub use kurbo::{
    Affine, BezPath, Cap, Circle, Ellipse, Join, Line, Point, Rect, RoundedRect, RoundedRectRadii,
    Shape, Size, Vec2,
};

/// Commonly used types, geometry helpers, and the CSS color palette.
pub mod prelude {
    pub use crate::Fill;
    pub use crate::kurbo::{
        Affine, BezPath, Cap, Circle, Ellipse, Join, Line, Point, Rect, RoundedRect,
        RoundedRectRadii, Shape, Size, Vec2,
    };
    pub use crate::{
        App, Canvas, Color, Font, Frame, Gradient, Paint, Stroke, colors, kurbo, peniko,
    };
}
