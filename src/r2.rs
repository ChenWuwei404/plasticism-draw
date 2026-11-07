//! R2 rounded-rectangle geometry.
//!
//! At full shoulder the corner profile is defined in local coordinates with the
//! origin at the corner center, `x` pointing right and `y` pointing up. It
//! starts on the top edge, uses two cubic Bezier segments with a short line
//! between them, and ends on the right edge. The same profile is
//! rotated/mirrored for the other three corners.
//!
//! The R2 shoulder only exists while there is room for it. Once a requested
//! radius would make neighboring shoulders overlap, each corner is treated as
//! two 1/8 pieces split on the diagonal, and the piece facing the crowded edge
//! blends from the R2 half profile to a plain 45 degree circular arc. The
//! radius itself is clamped at half the shorter side, where the limiting
//! shoulders have shrunk to zero and the corresponding pieces are quarter
//! circles. A square therefore becomes a circle, while an elongated rectangle
//! keeps its R2 shoulders along the long edges and becomes an R2 capsule.

use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, FRAC_PI_4};
use std::ops::{Add, Sub};

use crate::kurbo::{Affine, BezPath, PathEl, Point, Rect, Shape, Size, Vec2};

/// The shoulder extension of the R2 corner profile, as a fraction of `radius`.
const SHOULDER: f64 = 0.528665;

/// How much of a straight edge one corner consumes at full shoulder, as a
/// fraction of `radius`.
///
/// The profile starts at `-SHOULDER * R` on the top edge and ends at
/// `-SHOULDER * R` on the right edge, so a corner extends `1 + SHOULDER`
/// radius units along each edge.
const CORNER_EXTENT: f64 = 1.0 + SHOULDER;

/// Where the final cubic of the R2 profile is split for the adaptive corner.
///
/// Splitting the last cubic lets both 1/8 halves use three cubic segments
/// while keeping the full-shoulder geometry identical to the original profile.
const END_HALF_SPLIT: f64 = 0.25;

/// A rectangle with R2 rounded corners.
///
/// Construction normalizes the rectangle and clamps `radius` to half the
/// shorter side. Up to the legacy shoulder limit the corner profile uses two
/// cubic Bézier segments and a short connecting line; larger radii shrink the
/// shoulder adaptively toward a quarter circle (see the module docs).
///
/// Unlike a circular [`crate::kurbo::RoundedRect`], while the shoulder fits it
/// extends `0.528665 * radius` beyond the nominal tangent points. A corner
/// therefore consumes `1.528665 * radius` along both axes.
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
    /// A reversed rectangle is normalized, and the radius is clamped to half
    /// the shorter side. Non-positive or non-finite radii become zero.
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
    ///
    /// Values larger than half the shorter side are clamped to that maximum,
    /// where the shoulder has shrunk to zero on the limiting axis.
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
        let (x0, y0, x1, y1) = (self.rect.x0, self.rect.y0, self.rect.x1, self.rect.y1);
        let width = self.rect.width();
        let height = self.rect.height();
        let (hx, hy) = shoulder_fractions(self.rect, radius);

        let extent_x = radius * (1.0 + hx);
        let extent_y = radius * (1.0 + hy);
        let legacy = radius <= width.min(height) / (2.0 * CORNER_EXTENT);

        let transforms = [
            // Top-right corner: local +x is screen +x, local +y is screen -y.
            Affine::new([radius, 0.0, 0.0, -radius, x1 - radius, y0 + radius]),
            // Bottom-right corner.
            Affine::new([0.0, radius, radius, 0.0, x1 - radius, y1 - radius]),
            // Bottom-left corner.
            Affine::new([-radius, 0.0, 0.0, radius, x0 + radius, y1 - radius]),
            // Top-left corner.
            Affine::new([0.0, -radius, -radius, 0.0, x0 + radius, y0 + radius]),
        ];

        let mut path = BezPath::new();
        path.move_to((x0 + extent_x, y0));
        path.line_to((x1 - extent_x, y0));

        if legacy {
            // Below the legacy limit both shoulders are full, so use the
            // original profile verbatim to keep its exact path elements.
            append_corner(&mut path, transforms[0]);
            path.line_to((x1, y1 - extent_y));
            append_corner(&mut path, transforms[1]);
            path.line_to((x0 + extent_x, y1));
            append_corner(&mut path, transforms[2]);
            path.line_to((x0, y0 + extent_y));
            append_corner(&mut path, transforms[3]);
        } else {
            let geometry = CornerGeometry::new();
            // `horizontal_vertical` starts on a horizontal edge and ends on a
            // vertical edge; `vertical_horizontal` is its 90 degree rotation.
            let horizontal_vertical = geometry.adaptive_corner(hx, hy);
            let vertical_horizontal = geometry.adaptive_corner(hy, hx);

            append_corner_segments(&mut path, transforms[0], &horizontal_vertical);
            path.line_to((x1, y1 - extent_y));
            append_corner_segments(&mut path, transforms[1], &vertical_horizontal);
            path.line_to((x0 + extent_x, y1));
            append_corner_segments(&mut path, transforms[2], &horizontal_vertical);
            path.line_to((x0, y0 + extent_y));
            append_corner_segments(&mut path, transforms[3], &vertical_horizontal);
        }

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
    let max_radius = rect.width().min(rect.height()) / 2.0;
    if radius.is_finite() && radius > 0.0 {
        radius.min(max_radius)
    } else {
        0.0
    }
}

/// The shoulder fraction available on an edge of `length` for `radius`.
///
/// A corner would like `(1 + SHOULDER) * radius` of the edge. When two corners
/// on the same edge would overlap, each gives up half of the excess, so the
/// available shoulder is `length / (2 * radius) - 1`.
fn shoulder_fraction(length: f64, radius: f64) -> f64 {
    debug_assert!(radius > 0.0);
    (length / (2.0 * radius) - 1.0).clamp(0.0, SHOULDER)
}

fn shoulder_fractions(rect: Rect, radius: f64) -> (f64, f64) {
    (
        shoulder_fraction(rect.width(), radius),
        shoulder_fraction(rect.height(), radius),
    )
}

/// The base R2 corner, split into two 1/8 halves at the diagonal.
///
/// Each half has three cubic segments. The short line in the original profile
/// is kept as a degenerate cubic, and the final cubic is split so both halves
/// share the same segment count and meet at the diagonal.
struct CornerGeometry {
    a: Point,
    b: Point,
    f: Point,
    e: Point,
    c2_start: [Point; 4],
    c2_end: [Point; 4],
    c3_start: [Point; 4],
    c3_end: [Point; 4],
    diagonal: Point,
    angle_a: f64,
    angle_b: f64,
    angle_f: f64,
    angle_g: f64,
}

impl CornerGeometry {
    fn new() -> Self {
        let a = Point::new(0.330066, 0.934504);
        let b = Point::new(0.368506, 0.925089);
        let f = Point::new(0.925088, 0.368506);
        let e = Point::new(1.0, -SHOULDER);
        let c2 = [
            b,
            Point::new(0.627176, 0.830941),
            Point::new(0.830940, 0.627176),
            f,
        ];
        let c3 = [f, Point::new(1.0, 0.131593), Point::new(1.0, -0.088493), e];
        let (c2_start, c2_end) = split_cubic(c2, 0.5);
        let (c3_start, c3_end) = split_cubic(c3, END_HALF_SPLIT);
        let diagonal = c2_start[3];
        let g = c3_start[3];

        Self {
            a,
            b,
            f,
            e,
            c2_start,
            c2_end,
            c3_start,
            c3_end,
            diagonal,
            angle_a: a.y.atan2(a.x),
            angle_b: b.y.atan2(b.x),
            angle_f: f.y.atan2(f.x),
            angle_g: g.y.atan2(g.x),
        }
    }

    /// Builds the six cubic segments of an adaptive corner.
    ///
    /// `h_start` is the shoulder fraction on the edge where the profile starts
    /// (the local top edge), and `h_end` is the fraction on the edge where it
    /// ends (the local right edge). Both must be in `0..=SHOULDER`.
    fn adaptive_corner(&self, h_start: f64, h_end: f64) -> [[Point; 4]; 6] {
        let start_t = 1.0 - (h_start / SHOULDER).clamp(0.0, 1.0);
        let end_t = 1.0 - (h_end / SHOULDER).clamp(0.0, 1.0);
        let diagonal_t = 0.5 * (start_t + end_t);
        let diagonal = lerp(self.diagonal, circle_diagonal(), diagonal_t);

        let start = morph_halves(
            &self.start_half_r2(diagonal),
            &self.start_half_circle(diagonal),
            start_t,
        );
        let end = morph_halves(
            &self.end_half_r2(diagonal),
            &self.end_half_circle(diagonal),
            end_t,
        );

        [start[0], start[1], start[2], end[0], end[1], end[2]]
    }

    fn start_half_r2(&self, diagonal: Point) -> [[Point; 4]; 3] {
        let shift = diagonal - self.diagonal;
        [
            [
                Point::new(-SHOULDER, 1.0),
                Point::new(-0.088493, 1.0),
                Point::new(0.131593, 1.0),
                self.a,
            ],
            [
                self.a,
                lerp(self.a, self.b, 1.0 / 3.0),
                lerp(self.a, self.b, 2.0 / 3.0),
                self.b,
            ],
            [self.b, self.c2_start[1], self.c2_start[2] + shift, diagonal],
        ]
    }

    fn end_half_r2(&self, diagonal: Point) -> [[Point; 4]; 3] {
        let shift = diagonal - self.diagonal;
        [
            [diagonal, self.c2_end[1] + shift, self.c2_end[2], self.f],
            [self.f, self.c3_start[1], self.c3_start[2], self.c3_start[3]],
            [self.c3_start[3], self.c3_end[1], self.c3_end[2], self.e],
        ]
    }

    fn start_half_circle(&self, diagonal: Point) -> [[Point; 4]; 3] {
        let shift = diagonal - circle_diagonal();
        let mut segments = [
            arc_cubic(FRAC_PI_2, self.angle_a),
            arc_cubic(self.angle_a, self.angle_b),
            arc_cubic(self.angle_b, FRAC_PI_4),
        ];
        segments[2][2] += shift;
        segments[2][3] = diagonal;
        segments
    }

    fn end_half_circle(&self, diagonal: Point) -> [[Point; 4]; 3] {
        let shift = diagonal - circle_diagonal();
        let mut segments = [
            arc_cubic(FRAC_PI_4, self.angle_f),
            arc_cubic(self.angle_f, self.angle_g),
            arc_cubic(self.angle_g, 0.0),
        ];
        segments[0][0] = diagonal;
        segments[0][1] += shift;
        segments
    }
}

fn morph_halves(from: &[[Point; 4]; 3], to: &[[Point; 4]; 3], t: f64) -> [[Point; 4]; 3] {
    std::array::from_fn(|i| std::array::from_fn(|j| lerp(from[i][j], to[i][j], t)))
}

fn append_corner_segments(path: &mut BezPath, transform: Affine, segments: &[[Point; 4]; 6]) {
    for segment in segments {
        path.curve_to(
            transform * segment[1],
            transform * segment[2],
            transform * segment[3],
        );
    }
}

fn split_cubic(cubic: [Point; 4], t: f64) -> ([Point; 4], [Point; 4]) {
    let [p0, p1, p2, p3] = cubic;
    let p01 = lerp(p0, p1, t);
    let p12 = lerp(p1, p2, t);
    let p23 = lerp(p2, p3, t);
    let p012 = lerp(p01, p12, t);
    let p123 = lerp(p12, p23, t);
    let p = lerp(p012, p123, t);
    ([p0, p01, p012, p], [p, p123, p23, p3])
}

/// A cubic Bézier approximation of a circular arc from `a0` to `a1` radians.
fn arc_cubic(a0: f64, a1: f64) -> [Point; 4] {
    let p0 = Point::new(a0.cos(), a0.sin());
    let p3 = Point::new(a1.cos(), a1.sin());
    let k = (4.0 / 3.0) * ((a1 - a0) / 4.0).tan();
    let t0 = Vec2::new(-a0.sin(), a0.cos());
    let t1 = Vec2::new(-a1.sin(), a1.cos());
    [p0, p0 + t0 * k, p3 - t1 * k, p3]
}

fn circle_diagonal() -> Point {
    Point::new(FRAC_1_SQRT_2, FRAC_1_SQRT_2)
}

fn lerp(a: Point, b: Point, t: f64) -> Point {
    a + (b - a) * t
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
    use super::{
        CORNER_EXTENT, CornerGeometry, R2RoundedRect, SHOULDER, r2_rounded_rect_path,
        shoulder_fractions,
    };
    use crate::kurbo::{BezPath, PathEl, Point, Rect, Shape, Vec2};

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

    fn assert_path_inside(path: &BezPath, rect: Rect) {
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
        assert_path_inside(&path, rect);
    }

    #[test]
    fn adaptive_paths_fit_inside_varied_rectangles() {
        let rects = [
            Rect::new(0.0, 0.0, 100.0, 100.0),
            Rect::new(0.0, 0.0, 200.0, 100.0),
            Rect::new(0.0, 0.0, 100.0, 200.0),
            Rect::new(10.0, 20.0, 310.0, 240.0),
        ];

        for rect in rects {
            let short = rect.width().min(rect.height());
            for radius in [short * 0.35, short * 0.4, short * 0.5, 10_000.0] {
                assert_path_inside(&r2_rounded_rect_path(rect, radius), rect);
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

        assert_eq!(shape.rect(), Rect::new(0.0, 0.0, 100.0, 80.0));
        assert_eq!(shape.radius(), 40.0);
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

    #[test]
    fn shoulders_shrink_until_the_shorter_side_is_consumed() {
        let rect = Rect::new(0.0, 0.0, 200.0, 100.0);
        let legacy_limit = 100.0 / (2.0 * CORNER_EXTENT);

        let (start_hx, start_hy) = shoulder_fractions(rect, legacy_limit);
        assert!((start_hx - SHOULDER).abs() < EPSILON);
        assert!((start_hy - SHOULDER).abs() < EPSILON);

        let (hx, hy) = shoulder_fractions(rect, 50.0);
        assert!((hx - SHOULDER).abs() < EPSILON);
        assert!(hy.abs() < EPSILON);
    }

    #[test]
    fn radius_clamps_at_half_the_shorter_side() {
        let shape = R2RoundedRect::new(0.0, 0.0, 100.0, 60.0, 10_000.0);
        assert_eq!(shape.radius(), 30.0);
    }

    #[test]
    fn adaptive_branch_starts_after_the_legacy_limit() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let legacy_limit = 100.0 / (2.0 * CORNER_EXTENT);

        assert_eq!(
            r2_rounded_rect_path(rect, legacy_limit).elements().len(),
            22
        );
        assert_eq!(
            r2_rounded_rect_path(rect, legacy_limit + 0.01)
                .elements()
                .len(),
            30
        );
    }

    #[test]
    fn square_at_max_radius_is_a_circle() {
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);
        let shape = R2RoundedRect::from_rect(rect, 10_000.0);
        let path = shape.to_path(0.01);
        let center = rect.center();

        assert_eq!(shape.radius(), 50.0);
        assert_rect(path.bounding_box(), rect);

        for element in path.elements() {
            let end = match *element {
                PathEl::MoveTo(point) | PathEl::LineTo(point) => point,
                PathEl::QuadTo(_, point) | PathEl::CurveTo(_, _, point) => point,
                PathEl::ClosePath => continue,
            };
            let distance = (end - center).length();
            assert!(
                (distance - 50.0).abs() < 0.01,
                "point {end:?} is {distance} from the center"
            );
        }

        let circle_area = std::f64::consts::PI * 50.0 * 50.0;
        assert!((shape.area() - circle_area).abs() < 0.05);
        assert_eq!(shape.winding(center), 1);
        assert_eq!(shape.winding(Point::new(-1.0, 50.0)), 0);
    }

    #[test]
    fn wide_rectangle_at_max_radius_keeps_r2_shoulders() {
        let rect = Rect::new(0.0, 0.0, 200.0, 100.0);
        let shape = R2RoundedRect::from_rect(rect, 10_000.0);
        let path = shape.to_path(0.01);
        let elements = path.elements();

        assert_eq!(shape.radius(), 50.0);
        assert_eq!(elements.len(), 30);

        let PathEl::MoveTo(top_start) = elements[0] else {
            panic!("first element should be MoveTo");
        };
        assert_point(top_start, (50.0 * (1.0 + SHOULDER), 0.0));

        let PathEl::CurveTo(_, _, right_end) = elements[7] else {
            panic!("top-right corner should end with CurveTo");
        };
        assert_point(right_end, (200.0, 50.0));
    }

    #[test]
    fn mixed_shoulders_keep_the_diagonal_tangent_continuous() {
        let rect = Rect::new(0.0, 0.0, 300.0, 220.0);
        let (hx, hy) = shoulder_fractions(rect, 110.0);
        assert!(hx > hy);
        assert!(hy.abs() < EPSILON);

        let corner = CornerGeometry::new().adaptive_corner(hx, hy);
        assert_eq!(corner[2][3], corner[3][0]);

        let incoming = corner[2][3] - corner[2][2];
        let outgoing = corner[3][1] - corner[3][0];
        let cross = incoming.x * outgoing.y - incoming.y * outgoing.x;
        assert!(cross.abs() < 1e-6, "diagonal tangents diverge: {cross}");
    }
}
