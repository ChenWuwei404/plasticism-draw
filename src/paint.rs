//! Paint and stroke styles.

use crate::kurbo::{Cap, Join, Stroke as KurboStroke};
use crate::{Color, Gradient};
use vello_gpu::PaintType;

/// A source of color used to fill or stroke shapes.
///
/// A paint is usually a solid [`Color`] or a [`Gradient`]. References to both
/// types are accepted by the [`From`] implementations so drawing methods can
/// take `impl Into<Paint>`.
#[derive(Clone, Debug)]
pub struct Paint(PaintType);

impl Paint {
    /// Creates a solid-color paint.
    pub fn solid(color: Color) -> Self {
        Self(color.into())
    }

    /// Returns a copy of this paint with its alpha multiplied by `alpha`.
    ///
    /// The alpha is clamped to the `0.0..=1.0` range.
    #[must_use]
    pub fn with_alpha(mut self, alpha: f32) -> Self {
        let alpha = alpha.clamp(0.0, 1.0);
        self.0 = match self.0 {
            PaintType::Solid(color) => PaintType::Solid(color.with_alpha(alpha)),
            PaintType::Gradient(gradient) => PaintType::Gradient(gradient.with_alpha(alpha)),
            PaintType::Image(image) => PaintType::Image(image.with_alpha(alpha)),
        };
        self
    }

    pub(crate) fn into_inner(self) -> PaintType {
        self.0
    }
}

impl From<Color> for Paint {
    fn from(color: Color) -> Self {
        Self(color.into())
    }
}

impl From<&Color> for Paint {
    fn from(color: &Color) -> Self {
        Self((*color).into())
    }
}

impl From<Gradient> for Paint {
    fn from(gradient: Gradient) -> Self {
        Self(gradient.into())
    }
}

impl From<&Gradient> for Paint {
    fn from(gradient: &Gradient) -> Self {
        Self(gradient.clone().into())
    }
}

/// Style used when stroking a shape.
///
/// The paint is stored together with the stroke width and joins/caps so a
/// single value can be passed to every `stroke_*` method.
#[derive(Clone, Debug)]
pub struct Stroke {
    width: f64,
    paint: Paint,
    cap: Cap,
    join: Join,
    miter_limit: f64,
    dash: Option<(f64, Vec<f64>)>,
}

impl Stroke {
    /// Creates a round-capped, round-joined stroke of `width` canvas units.
    ///
    /// Negative and non-finite widths are treated as zero.
    pub fn new(width: f64, paint: impl Into<Paint>) -> Self {
        Self {
            width: normalize_width(width),
            paint: paint.into(),
            cap: Cap::Round,
            join: Join::Round,
            miter_limit: 4.0,
            dash: None,
        }
    }

    /// Sets the cap style used at the ends of open subpaths.
    #[must_use]
    pub fn cap(mut self, cap: Cap) -> Self {
        self.cap = cap;
        self
    }

    /// Sets the join style used between path segments.
    #[must_use]
    pub fn join(mut self, join: Join) -> Self {
        self.join = join;
        self
    }

    /// Sets the miter limit for [`Join::Miter`] joins.
    #[must_use]
    pub fn miter_limit(mut self, limit: f64) -> Self {
        self.miter_limit = if limit.is_finite() {
            limit.max(0.0)
        } else {
            4.0
        };
        self
    }

    /// Sets a dash pattern with `offset` and alternating on/off lengths.
    ///
    /// Non-finite or negative dash lengths are discarded. An empty pattern
    /// disables dashing.
    #[must_use]
    pub fn dash(mut self, offset: f64, pattern: impl IntoIterator<Item = f64>) -> Self {
        let pattern: Vec<f64> = pattern
            .into_iter()
            .filter(|length| length.is_finite() && *length >= 0.0)
            .collect();
        self.dash = if pattern.is_empty() {
            None
        } else {
            Some((if offset.is_finite() { offset } else { 0.0 }, pattern))
        };
        self
    }

    /// Returns the paint used by this stroke.
    pub fn paint(&self) -> Paint {
        self.paint.clone()
    }

    pub(crate) fn as_paint(&self) -> &Paint {
        &self.paint
    }

    pub(crate) fn to_kurbo(&self) -> KurboStroke {
        let mut stroke = KurboStroke::new(self.width)
            .with_caps(self.cap)
            .with_join(self.join)
            .with_miter_limit(self.miter_limit);

        if let Some((offset, pattern)) = &self.dash {
            stroke = stroke.with_dashes(*offset, pattern.iter().copied());
        }

        stroke
    }
}

impl Default for Stroke {
    fn default() -> Self {
        Self::new(1.0, Color::BLACK)
    }
}

impl From<&Stroke> for Stroke {
    fn from(stroke: &Stroke) -> Self {
        stroke.clone()
    }
}

fn normalize_width(width: f64) -> f64 {
    if width.is_finite() {
        width.max(0.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::{Paint, Stroke};
    use crate::kurbo::{Cap, Join};
    use crate::{Color, Gradient};

    #[test]
    fn paint_accepts_solid_colors_and_gradient_references() {
        let color = Color::from_rgb8(10, 20, 30);
        let _: Paint = color.into();
        let _: Paint = (&color).into();

        let gradient = Gradient::new_linear((0.0, 0.0), (10.0, 0.0));
        let _: Paint = gradient.clone().into();
        let _: Paint = (&gradient).into();
    }

    #[test]
    fn stroke_builder_maps_to_kurbo() {
        let stroke = Stroke::new(3.0, Color::BLACK)
            .cap(Cap::Square)
            .join(Join::Bevel)
            .miter_limit(2.0)
            .dash(1.0, [4.0, 2.0]);
        let kurbo = stroke.to_kurbo();

        assert_eq!(kurbo.width, 3.0);
        assert_eq!(kurbo.start_cap, Cap::Square);
        assert_eq!(kurbo.end_cap, Cap::Square);
        assert_eq!(kurbo.join, Join::Bevel);
        assert_eq!(kurbo.miter_limit, 2.0);
        assert_eq!(kurbo.dash_offset, 1.0);
        assert_eq!(kurbo.dash_pattern.as_slice(), &[4.0, 2.0]);
    }

    #[test]
    fn invalid_stroke_values_are_defensively_normalized() {
        let stroke = Stroke::new(f64::NAN, Color::BLACK)
            .miter_limit(f64::INFINITY)
            .dash(f64::NAN, [f64::NAN, -1.0]);
        let kurbo = stroke.to_kurbo();

        assert_eq!(kurbo.width, 0.0);
        assert_eq!(kurbo.miter_limit, 4.0);
        assert!(kurbo.dash_pattern.is_empty());
    }
}
