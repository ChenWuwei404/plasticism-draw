//! R2 rounded-rectangle geometry.
//!
//! The corner profile is defined in local coordinates with the origin at the
//! corner center, `x` pointing right and `y` pointing up. It starts on the top
//! edge, uses two cubic Bezier segments with a short line between them, and
//! ends on the right edge. The same profile is rotated/mirrored for the other
//! three corners.

use std::ops::{Add, Sub};

use crate::kurbo::{Affine, BezPath, PathEl, Point, Rect, Shape, Size, Vec2};

/// The shoulder extension of the R2 corner profile, as a fraction of `radius`.
const SHOULDER: f64 = 0.528665;

/// How much of a straight edge one corner consumes, as a fraction of `radius`.
///
/// The profile starts at `-SHOULDER * R` on the top edge and ends at
/// `-SHOULDER * R` on the right edge, so a corner extends `1 + SHOULDER`
/// radius units along each edge.
const CORNER_EXTENT: f64 = 1.0 + SHOULDER;

/// A rectangle with R2 rounded corners.
///
/// Construction normalizes the rectangle and clamps `radius` so neighboring
/// corners cannot overlap. The corner profile uses two cubic Bézier segments
/// and a short connecting line; see [`r2_rounded_rect_path`] for the geometry.
///
/// Unlike a circular [`crate::kurbo::RoundedRect`], the R2 shoulder extends
/// `0.528665 * radius` beyond the nominal tangent points. A corner therefore
/// consumes `1.528665 * radius` along both axes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct R2RoundedRect {
    rect: Rect,
    radius: f64,
}

impl R2RoundedRect {
    /// Creates a rectangle from minimum and maximum coordinates.
    ///
    /// The rectangle and radius are normalized according to
    /// [`R2RoundedRect::from_rect`].
    #[inline]
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64, radius: f64) -> Self {
        Self::from_rect(Rect::new(x0, y0, x1, y1), radius)
    }

    /// Creates a rounded rectangle from a rectangle and an R2 radius.
    ///
    /// A reversed rectangle is normalized, and the radius is clamped to fit the
    /// rectangle. Non-positive or non-finite radii become zero.
    #[inline]
    pub fn from_rect(rect: Rect, radius: f64) -> Self {
        let rect = rect.abs();
        Self {
            rect,
            radius: clamp_radius(rect, radius),
        }
    }

    /// Creates a rounded rectangle from two points.
    #[inline]
    pub fn from_points(p0: impl Into<Point>, p1: impl Into<Point>, radius: f64) -> Self {
        Self::from_rect(Rect::from_points(p0, p1), radius)
    }

    /// Creates a rounded rectangle from an origin and size.
    #[inline]
    pub fn from_origin_size(origin: impl Into<Point>, size: impl Into<Size>, radius: f64) -> Self {
        Self::from_rect(Rect::from_origin_size(origin, size), radius)
    }

    /// The width of the rectangle.
    #[inline]
    pub const fn width(&self) -> f64 {
        self.rect.width()
    }

    /// The height of the rectangle.
    #[inline]
    pub const fn height(&self) -> f64 {
        self.rect.height()
    }

    /// The clamped R2 radius.
    #[inline]
    pub const fn radius(&self) -> f64 {
        self.radius
    }

    /// The normalized, non-rounded rectangle.
    #[inline]
    pub const fn rect(&self) -> Rect {
        self.rect
    }

    /// The origin of the rectangle (top-left in a y-down coordinate system).
    #[inline]
    pub const fn origin(&self) -> Point {
        self.rect.origin()
    }

    /// The center of the rectangle.
    #[inline]
    pub const fn center(&self) -> Point {
        self.rect.center()
    }

    /// Whether the rectangle and radius are finite.
    #[inline]
    pub const fn is_finite(&self) -> bool {
        self.rect.is_finite() && self.radius.is_finite()
    }

    /// Whether the rectangle or radius is NaN.
    #[inline]
    pub const fn is_nan(&self) -> bool {
        self.rect.is_nan() || self.radius.is_nan()
    }

    fn build_path(&self) -> BezPath {
        if self.radius <= 0.0 {
            return plain_rect_path(self.rect);
        }

        let radius = self.radius;
        let extent = radius * CORNER_EXTENT;
        let (x0, y0, x1, y1) = (self.rect.x0, self.rect.y0, self.rect.x1, self.rect.y1);

        let mut path = BezPath::new();
        path.move_to((x0 + extent, y0));
        path.line_to((x1 - extent, y0));

        // Top-right corner: local +x is screen +x, local +y is screen -y.
        append_corner(
            &mut path,
            Affine::new([radius, 0.0, 0.0, -radius, x1 - radius, y0 + radius]),
        );

        path.line_to((x1, y1 - extent));

        // Bottom-right corner.
        append_corner(
            &mut path,
            Affine::new([0.0, radius, radius, 0.0, x1 - radius, y1 - radius]),
        );

        path.line_to((x0 + extent, y1));

        // Bottom-left corner.
        append_corner(
            &mut path,
            Affine::new([-radius, 0.0, 0.0, radius, x0 + radius, y1 - radius]),
        );

        path.line_to((x0, y0 + extent));

        // Top-left corner.
        append_corner(
            &mut path,
            Affine::new([0.0, -radius, -radius, 0.0, x0 + radius, y0 + radius]),
        );

        path.close_path();
        path
    }
}

impl Shape for R2RoundedRect {
    type PathElementsIter<'iter> = std::vec::IntoIter<PathEl>;

    fn path_elements(&self, _tolerance: f64) -> Self::PathElementsIter<'_> {
        self.build_path().elements().to_vec().into_iter()
    }

    fn to_path(&self, _tolerance: f64) -> BezPath {
        self.build_path()
    }

    fn area(&self) -> f64 {
        self.build_path().area()
    }

    fn perimeter(&self, accuracy: f64) -> f64 {
        self.build_path().perimeter(accuracy)
    }

    fn winding(&self, point: Point) -> i32 {
        self.build_path().winding(point)
    }

    fn bounding_box(&self) -> Rect {
        self.rect
    }
}

impl Add<Vec2> for R2RoundedRect {
    type Output = R2RoundedRect;

    #[inline]
    fn add(self, offset: Vec2) -> Self::Output {
        R2RoundedRect::from_rect(self.rect + offset, self.radius)
    }
}

impl Sub<Vec2> for R2RoundedRect {
    type Output = R2RoundedRect;

    #[inline]
    fn sub(self, offset: Vec2) -> Self::Output {
        R2RoundedRect::from_rect(self.rect - offset, self.radius)
    }
}

/// Builds the outline of an R2 rounded rectangle.
///
/// This is the path equivalent of `R2RoundedRect::from_rect(rect, radius)`,
/// suitable for `Canvas::fill_path`, `Canvas::stroke_path`, or clipping.
#[must_use]
pub fn r2_rounded_rect_path(rect: Rect, radius: f64) -> BezPath {
    R2RoundedRect::from_rect(rect, radius).build_path()
}

fn clamp_radius(rect: Rect, radius: f64) -> f64 {
    let max_radius = rect.width().min(rect.height()) / (2.0 * CORNER_EXTENT);
    if radius.is_finite() && radius > 0.0 {
        radius.min(max_radius)
    } else {
        0.0
    }
}

fn append_corner(path: &mut BezPath, transform: Affine) {
    let p = |x: f64, y: f64| transform * Point::new(x, y);

    // C1: horizontal tangent, zero curvature at the start.
    path.curve_to(p(-0.088493, 1.0), p(0.131593, 1.0), p(0.330066, 0.934504));

    // L1: short right/down transition.
    path.line_to(p(0.368506, 0.925089));

    // C2: main diagonal corner curve.
    path.curve_to(
        p(0.627176, 0.830941),
        p(0.830940, 0.627176),
        p(0.925088, 0.368506),
    );

    // C3: vertical tangent, zero curvature at the end.
    path.curve_to(p(1.0, 0.131593), p(1.0, -0.088493), p(1.0, -SHOULDER));
}

fn plain_rect_path(rect: Rect) -> BezPath {
    let mut path = BezPath::new();
    path.move_to((rect.x0, rect.y0));
    path.line_to((rect.x1, rect.y0));
    path.line_to((rect.x1, rect.y1));
    path.line_to((rect.x0, rect.y1));
    path.close_path();
    path
}

#[cfg(test)]
mod tests {
    use super::{R2RoundedRect, r2_rounded_rect_path};
    use crate::kurbo::{PathEl, Point, Rect, Shape, Vec2};

    const EPSILON: f64 = 1e-6;

    fn assert_point(actual: Point, expected: (f64, f64)) {
        assert!(
            (actual.x - expected.0).abs() < EPSILON && (actual.y - expected.1).abs() < EPSILON,
            "expected {expected:?}, got ({}, {})",
            actual.x,
            actual.y
        );
    }

    fn assert_rect(actual: Rect, expected: Rect) {
        assert!(
            (actual.x0 - expected.x0).abs() < EPSILON
                && (actual.y0 - expected.y0).abs() < EPSILON
                && (actual.x1 - expected.x1).abs() < EPSILON
                && (actual.y1 - expected.y1).abs() < EPSILON,
            "expected {expected:?}, got {actual:?}"
        );
    }

    #[test]
    fn path_has_expected_corner_endpoints() {
        let path = r2_rounded_rect_path(Rect::new(0.0, 0.0, 100.0, 100.0), 20.0);
        let elements = path.elements();

        assert_eq!(elements.len(), 22);
        assert!(matches!(elements.last(), Some(PathEl::ClosePath)));

        let PathEl::MoveTo(top_start) = elements[0] else {
            panic!("first element should be MoveTo");
        };
        assert_point(top_start, (30.5733, 0.0));

        let PathEl::LineTo(top_end) = elements[1] else {
            panic!("second element should be LineTo");
        };
        assert_point(top_end, (69.4267, 0.0));

        let PathEl::CurveTo(_, _, tr_end) = elements[5] else {
            panic!("top-right C3 should end with CurveTo");
        };
        assert_point(tr_end, (100.0, 30.5733));

        let PathEl::CurveTo(_, _, br_end) = elements[10] else {
            panic!("bottom-right C3 should end with CurveTo");
        };
        assert_point(br_end, (69.4267, 100.0));

        let PathEl::CurveTo(_, _, bl_end) = elements[15] else {
            panic!("bottom-left C3 should end with CurveTo");
        };
        assert_point(bl_end, (0.0, 69.4267));

        let PathEl::CurveTo(_, _, tl_end) = elements[20] else {
            panic!("top-left C3 should end with CurveTo");
        };
        assert_point(tl_end, (30.5733, 0.0));
    }

    #[test]
    fn path_uses_the_table_control_points() {
        let path = r2_rounded_rect_path(Rect::new(0.0, 0.0, 100.0, 100.0), 20.0);
        let PathEl::CurveTo(p1, p2, p3) = path.elements()[4] else {
            panic!("top-right C2 should be CurveTo");
        };

        assert_point(p1, (92.54352, 3.38118));
        assert_point(p2, (96.6188, 7.45648));
        assert_point(p3, (98.50176, 12.62988));
    }

    #[test]
    fn path_bounds_match_the_rectangle() {
        let rect = Rect::new(10.0, 20.0, 210.0, 140.0);
        let path = r2_rounded_rect_path(rect, 30.0);
        assert_rect(path.bounding_box(), rect);
    }

    #[test]
    fn oversized_radius_is_clamped_and_stays_inside() {
        let rect = Rect::new(0.0, 0.0, 40.0, 40.0);
        let path = r2_rounded_rect_path(rect, 10_000.0);

        assert_rect(path.bounding_box(), rect);
        for element in path.elements() {
            let points: Vec<Point> = match *element {
                PathEl::MoveTo(point) | PathEl::LineTo(point) => vec![point],
                PathEl::QuadTo(p1, p2) => vec![p1, p2],
                PathEl::CurveTo(p1, p2, p3) => vec![p1, p2, p3],
                PathEl::ClosePath => Vec::new(),
            };
            for point in points {
                assert!(point.x.is_finite() && point.y.is_finite());
                assert!(rect.x0 - EPSILON <= point.x && point.x <= rect.x1 + EPSILON);
                assert!(rect.y0 - EPSILON <= point.y && point.y <= rect.y1 + EPSILON);
            }
        }
    }

    #[test]
    fn invalid_radii_fall_back_to_a_plain_rectangle() {
        for radius in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let path = r2_rounded_rect_path(Rect::new(0.0, 0.0, 50.0, 40.0), radius);
            assert_eq!(path.elements().len(), 5);
        }
    }

    #[test]
    fn reversed_rectangle_is_normalized() {
        let path = r2_rounded_rect_path(Rect::new(100.0, 80.0, 0.0, 0.0), 15.0);
        assert_rect(path.bounding_box(), Rect::new(0.0, 0.0, 100.0, 80.0));
    }

    #[test]
    fn shape_geometry_and_path_agree() {
        let shape = R2RoundedRect::new(0.0, 0.0, 100.0, 100.0, 20.0);
        let path = shape.to_path(0.1);

        assert_eq!(shape.rect(), Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(shape.width(), 100.0);
        assert_eq!(shape.height(), 100.0);
        assert_eq!(shape.radius(), 20.0);
        assert_eq!(shape.path_elements(0.1).count(), 22);
        assert_eq!(path.elements().len(), 22);
        assert_rect(shape.bounding_box(), shape.rect());
        assert!((shape.area() - path.area()).abs() < EPSILON);
        assert!((shape.perimeter(0.01) - path.perimeter(0.01)).abs() < EPSILON);
        assert_eq!(shape.winding(Point::new(50.0, 50.0)), 1);
        assert_eq!(shape.winding(Point::new(-1.0, 50.0)), 0);
    }

    #[test]
    fn shape_constructors_normalize_rectangle_and_radius() {
        let shape = R2RoundedRect::new(100.0, 80.0, 0.0, 0.0, 10_000.0);
        let max_radius = 80.0 / (2.0 * 1.528665);

        assert_eq!(shape.rect(), Rect::new(0.0, 0.0, 100.0, 80.0));
        assert!(shape.radius() <= max_radius + EPSILON);
        assert!(shape.is_finite());
        assert!(!shape.is_nan());

        let from_points = R2RoundedRect::from_points((0.0, 0.0), (50.0, 60.0), 10.0);
        let from_origin = R2RoundedRect::from_origin_size((0.0, 0.0), (50.0, 60.0), 10.0);
        assert_eq!(from_points, from_origin);
    }

    #[test]
    fn translation_keeps_radius_and_normalizes() {
        let shape = R2RoundedRect::new(0.0, 0.0, 100.0, 100.0, 20.0);
        let moved = shape + Vec2::new(10.0, 20.0);
        let back = moved - Vec2::new(10.0, 20.0);

        assert_eq!(moved.rect(), Rect::new(10.0, 20.0, 110.0, 120.0));
        assert_eq!(back.rect(), shape.rect());
        assert_eq!(moved.radius(), shape.radius());
    }
}
