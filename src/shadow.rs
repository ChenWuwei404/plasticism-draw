//! Shape-based drop shadows.

use vello_gpu::filter_effects::{EdgeMode, Filter, FilterPrimitive};

use crate::Color;
use crate::kurbo::Vec2;

/// A drop shadow cast by a shape.
///
/// A shadow follows the silhouette of the shape it is drawn from: the shape's
/// alpha coverage is offset by [`Shadow::offset`], blurred with a Gaussian
/// kernel, and colored with [`Shadow::color`]. Only the shadow is drawn by
/// [`Canvas::draw_shadow`](crate::Canvas::draw_shadow), so paint the shape
/// afterwards to place it on top of its own shadow.
///
/// Lengths are canvas units. Like every other canvas parameter they are scaled
/// by the window DPI and by the current transform, so a rotated or scaled
/// transform rotates and scales the shadow along with the shape.
#[derive(Clone, Copy, Debug)]
pub struct Shadow {
    offset: Vec2,
    blur: f64,
    color: Color,
}

impl Shadow {
    /// Creates a shadow with an `offset`, a blur radius, and a color.
    ///
    /// `blur` is a blur radius in the `box-shadow` sense, so it is twice the
    /// Gaussian standard deviation; the visible falloff extends roughly
    /// `1.5 * blur` beyond the silhouette. A value of zero produces a
    /// hard-edged shadow. Negative and non-finite blur values are treated as
    /// zero, and non-finite offset components are treated as zero.
    pub fn new(offset: impl Into<Vec2>, blur: f64, color: impl Into<Color>) -> Self {
        Self {
            offset: normalize_offset(offset.into()),
            blur: normalize_blur(blur),
            color: color.into(),
        }
    }

    /// Sets the offset of the shadow in canvas units.
    ///
    /// A positive x offset moves the shadow right and a positive y offset moves
    /// it down. Non-finite components are treated as zero.
    #[must_use]
    pub fn offset(mut self, offset: impl Into<Vec2>) -> Self {
        self.offset = normalize_offset(offset.into());
        self
    }

    /// Sets the blur radius in canvas units; see [`Shadow::new`].
    #[must_use]
    pub fn blur(mut self, blur: f64) -> Self {
        self.blur = normalize_blur(blur);
        self
    }

    /// Sets the color of the shadow.
    ///
    /// The alpha of the color controls how strongly the shadow shows through.
    /// The paint used to fill the shape never affects the shadow color.
    #[must_use]
    pub fn color(mut self, color: impl Into<Color>) -> Self {
        self.color = color.into();
        self
    }

    /// Opaque version of the shadow color used to paint the layer silhouette.
    ///
    /// The drop shadow filter reads only the layer's alpha and multiplies its
    /// own color by it. Painting the silhouette with the (possibly translucent)
    /// shadow color would therefore apply the color alpha twice and make the
    /// shadow too faint, so the silhouette is always recorded opaque.
    pub(crate) fn silhouette(&self) -> Color {
        self.color.with_alpha(1.0)
    }

    pub(crate) fn filter(&self) -> Filter {
        Filter::from_primitive(FilterPrimitive::DropShadowOnly {
            dx: self.offset.x as f32,
            dy: self.offset.y as f32,
            std_deviation: (self.blur * 0.5) as f32,
            color: self.color,
            edge_mode: EdgeMode::None,
        })
    }
}

impl Default for Shadow {
    /// A soft shadow offset four units down, with an eight-unit blur radius and
    /// 25% black.
    fn default() -> Self {
        Self::new((0.0, 4.0), 8.0, Color::from_rgba8(0, 0, 0, 64))
    }
}

impl From<&Shadow> for Shadow {
    fn from(shadow: &Shadow) -> Self {
        *shadow
    }
}

fn normalize_offset(offset: Vec2) -> Vec2 {
    Vec2::new(finite(offset.x), finite(offset.y))
}

fn normalize_blur(blur: f64) -> f64 {
    finite(blur).max(0.0)
}

fn finite(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::Shadow;
    use crate::Color;
    use vello_gpu::filter_effects::{EdgeMode, Filter, FilterPrimitive};

    fn filter(shadow: Shadow) -> Filter {
        shadow.filter()
    }

    fn drop_shadow(shadow: Shadow) -> (f32, f32, f32, Color, EdgeMode) {
        let filter = filter(shadow);
        match &filter.graph.primitives[0] {
            FilterPrimitive::DropShadowOnly {
                dx,
                dy,
                std_deviation,
                color,
                edge_mode,
            } => (*dx, *dy, *std_deviation, *color, *edge_mode),
            other => panic!("unexpected filter primitive: {other:?}"),
        }
    }

    #[test]
    fn default_shadow_has_documented_values() {
        let (dx, dy, std_deviation, color, edge_mode) = drop_shadow(Shadow::default());

        assert_eq!(dx, 0.0);
        assert_eq!(dy, 4.0);
        assert_eq!(std_deviation, 4.0);
        assert_eq!(color, Color::from_rgba8(0, 0, 0, 64));
        assert_eq!(edge_mode, EdgeMode::None);
    }

    #[test]
    fn blur_radius_is_halved_to_standard_deviation() {
        let (_, _, std_deviation, _, _) = drop_shadow(Shadow::new((0.0, 0.0), 24.0, Color::BLACK));

        assert_eq!(std_deviation, 12.0);
    }

    #[test]
    fn builders_replace_each_component() {
        let shadow = Shadow::default()
            .offset((3.0, -6.0))
            .blur(10.0)
            .color(Color::from_rgba8(10, 20, 30, 40));
        let (dx, dy, std_deviation, color, _) = drop_shadow(shadow);

        assert_eq!(dx, 3.0);
        assert_eq!(dy, -6.0);
        assert_eq!(std_deviation, 5.0);
        assert_eq!(color, Color::from_rgba8(10, 20, 30, 40));
    }

    #[test]
    fn invalid_values_are_normalized() {
        let (dx, dy, std_deviation, _, _) =
            drop_shadow(Shadow::new((f64::NAN, f64::INFINITY), -3.0, Color::BLACK));
        assert_eq!(dx, 0.0);
        assert_eq!(dy, 0.0);
        assert_eq!(std_deviation, 0.0);

        let (_, _, std_deviation, _, _) = drop_shadow(Shadow::default().blur(f64::NAN));
        assert_eq!(std_deviation, 0.0);
    }

    #[test]
    fn silhouette_is_opaque_but_the_filter_keeps_the_color_alpha() {
        let shadow = Shadow::new((0.0, 0.0), 4.0, Color::from_rgba8(10, 20, 30, 128));

        let silhouette = shadow.silhouette().to_rgba8();
        assert_eq!(silhouette.a, 255);
        assert_eq!(
            (silhouette.r, silhouette.g, silhouette.b),
            (10, 20, 30),
            "the silhouette keeps the shadow hue"
        );

        let (_, _, _, color, _) = drop_shadow(shadow);
        assert_eq!(color.to_rgba8().a, 128);
    }

    #[test]
    fn shadow_is_copyable_through_references() {
        let shadow = Shadow::default();
        let copy = Shadow::from(&shadow);

        assert_eq!(shadow.filter().graph, copy.filter().graph);
    }
}
