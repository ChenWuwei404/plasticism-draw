//! R2 rounded-rectangle geometry.
//!
//! The corner profile is defined in local coordinates with the origin at the
//! corner center, `x` pointing right and `y` pointing up. It starts on the top
//! edge, uses two cubic Bezier segments with a short line between them, and
//! ends on the right edge. The same profile is rotated/mirrored for the other
//! three corners.

use crate::kurbo::{Affine, BezPath, Point, Rect};

/// The shoulder extension of the R2 corner profile, as a fraction of `radius`.
const SHOULDER: f64 = 0.528665;

/// How much of a straight edge one corner consumes, as a fraction of `radius`.
///
/// The profile starts at `-SHOULDER * R` on the top edge and ends at
/// `-SHOULDER * R` on the right edge, so a corner extends `1 + SHOULDER`
/// radius units along each edge.
const CORNER_EXTENT: f64 = 1.0 + SHOULDER;

/// Builds the outline of an R2 rounded rectangle.
///
/// `radius` is the `R` from the R2 corner profile. This is not the total
/// corner size: the profile extends `0.528665 * R` past the nominal tangent
/// points, so a corner consumes `1.528665 * R` along each axis. Values that
/// would make adjacent corners overlap are clamped to fit the rectangle.
///
/// Non-positive or non-finite radii produce a plain rectangle. A reversed
/// rectangle is normalized first.
#[must_use]
pub fn r2_rounded_rect_path(rect: Rect, radius: f64) -> BezPath {
    let rect = rect.abs();
    let max_radius = rect.width().min(rect.height()) / (2.0 * CORNER_EXTENT);
    let radius = if radius.is_finite() && radius > 0.0 {
        radius.min(max_radius)
    } else {
        0.0
    };

    if radius <= 0.0 {
        return plain_rect_path(rect);
    }

    let extent = radius * CORNER_EXTENT;
    let (x0, y0, x1, y1) = (rect.x0, rect.y0, rect.x1, rect.y1);

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
    use super::r2_rounded_rect_path;
    use crate::kurbo::{PathEl, Point, Rect, Shape};

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
}
