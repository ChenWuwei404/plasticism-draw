//! Vector icon loading and SVG path conversion.

use std::path::Path;

use anyhow::{Context, Result, bail};
use usvg::tiny_skia_path::PathSegment;

use crate::Fill;
use crate::image::{ImageFit, fitted_rect};
use crate::kurbo::{self, Affine, BezPath, Rect, Shape, Stroke as KurboStroke, StrokeOpts};

/// A monochrome icon represented by one or more Bezier paths.
///
/// Icons can be built from SVG path data or from a complete SVG document. In
/// both cases the icon stores geometry only; colors from the source are
/// ignored, and [`Canvas::draw_icon`](crate::Canvas::draw_icon) fills every
/// part with the supplied paint.
///
/// The icon is designed inside [`view_box`](Self::view_box). Drawing maps that
/// rectangle proportionally and centered into the destination rectangle.
#[derive(Clone, Debug)]
pub struct VectorIcon {
    parts: Vec<IconPart>,
    view_box: Rect,
}

#[derive(Clone, Debug)]
pub(crate) struct IconPart {
    pub(crate) path: BezPath,
    pub(crate) fill: Fill,
}

impl VectorIcon {
    /// Builds an icon from SVG path data (the contents of a `d` attribute).
    ///
    /// The path's bounding box becomes the view box. Use
    /// [`with_view_box`](Self::with_view_box) to preserve the original icon
    /// canvas instead.
    pub fn from_svg_path(data: &str) -> Result<Self> {
        let path = BezPath::from_svg(data).context("invalid SVG path data")?;
        if path.is_empty() {
            bail!("SVG path data is empty");
        }
        let view_box = path.bounding_box();
        Ok(Self::from_parts(
            vec![IconPart {
                path,
                fill: Fill::NonZero,
            }],
            view_box,
        ))
    }

    /// Builds an icon from an already constructed path.
    ///
    /// The path's bounding box becomes the view box.
    pub fn from_path(path: impl Into<BezPath>) -> Self {
        let path = path.into();
        let view_box = path.bounding_box();
        Self::from_parts(
            vec![IconPart {
                path,
                fill: Fill::NonZero,
            }],
            view_box,
        )
    }

    /// Parses a complete SVG document.
    ///
    /// Fill and stroke geometry is combined into the icon. Colors, opacity,
    /// gradients, filters, clips, masks, text, and embedded images are ignored;
    /// svg sources are expected to describe monochrome icons. Strokes are
    /// converted to filled outlines so they remain visible when the icon is
    /// rendered with the requested paint.
    pub fn from_svg(svg: &str) -> Result<Self> {
        let tree =
            usvg::Tree::from_str(svg, &usvg::Options::default()).context("failed to parse SVG")?;
        Ok(Self::from_tree(&tree))
    }

    /// Parses a complete SVG document from memory.
    pub fn from_svg_bytes(bytes: impl AsRef<[u8]>) -> Result<Self> {
        let tree = usvg::Tree::from_data(bytes.as_ref(), &usvg::Options::default())
            .context("failed to parse SVG")?;
        Ok(Self::from_tree(&tree))
    }

    /// Reads and parses an SVG file.
    pub fn from_svg_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read SVG {}", path.display()))?;
        Self::from_svg_bytes(bytes)
            .with_context(|| format!("failed to parse SVG {}", path.display()))
    }

    /// Replaces the icon view box with a rectangle whose origin is `(0, 0)`.
    ///
    /// Non-positive or non-finite dimensions make the icon draw nothing.
    #[must_use]
    pub fn with_view_box(mut self, width: f64, height: f64) -> Self {
        self.view_box = Rect::new(0.0, 0.0, normalize_extent(width), normalize_extent(height));
        self
    }

    /// Replaces the icon view box with an explicit rectangle.
    #[must_use]
    pub fn with_view_box_rect(mut self, rect: Rect) -> Self {
        self.view_box = rect.abs();
        self
    }

    /// Overrides the fill rule of every path in the icon.
    ///
    /// Path data does not encode a fill rule, so call this with
    /// [`Fill::EvenOdd`](crate::Fill::EvenOdd) for icons that use even-odd
    /// holes. Complete SVG files keep their original per-path fill rules and
    /// normally do not need this override.
    #[must_use]
    pub fn with_fill_rule(mut self, fill: Fill) -> Self {
        for part in &mut self.parts {
            part.fill = fill;
        }
        self
    }

    /// Returns the icon design rectangle.
    pub fn view_box(&self) -> Rect {
        self.view_box
    }

    /// Returns whether the icon contains no geometry.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    pub(crate) fn parts(&self) -> &[IconPart] {
        &self.parts
    }

    pub(crate) fn transform_for(&self, rect: Rect, fit: ImageFit) -> Option<Affine> {
        let view_box = self.view_box;
        if view_box.width() <= 0.0 || view_box.height() <= 0.0 {
            return None;
        }

        let dest = fitted_rect(view_box.size(), rect, fit)?;
        let scale_x = dest.width() / view_box.width();
        let scale_y = dest.height() / view_box.height();

        Some(
            Affine::translate((dest.x0, dest.y0))
                * Affine::scale_non_uniform(scale_x, scale_y)
                * Affine::translate((-view_box.x0, -view_box.y0)),
        )
    }

    fn from_parts(parts: Vec<IconPart>, view_box: Rect) -> Self {
        Self { parts, view_box }
    }

    fn from_tree(tree: &usvg::Tree) -> Self {
        let size = tree.size();
        let view_box = Rect::new(0.0, 0.0, f64::from(size.width()), f64::from(size.height()));
        let mut parts = Vec::new();
        collect_group(tree.root(), &mut parts);
        Self { parts, view_box }
    }
}

fn collect_group(group: &usvg::Group, parts: &mut Vec<IconPart>) {
    for node in group.children() {
        match node {
            usvg::Node::Group(group) => collect_group(group, parts),
            usvg::Node::Path(path) => collect_path(path, parts),
            _ => {}
        }
    }
}

fn collect_path(path: &usvg::Path, parts: &mut Vec<IconPart>) {
    if !path.is_visible() {
        return;
    }

    let object = tiny_path_to_bez(path.data());
    if object.is_empty() {
        return;
    }

    let transform = affine_from_usvg(path.abs_transform());
    if let Some(fill) = path.fill() {
        let mut filled = object.clone();
        filled.apply_affine(transform);
        if !filled.is_empty() {
            parts.push(IconPart {
                path: filled,
                fill: match fill.rule() {
                    usvg::FillRule::NonZero => Fill::NonZero,
                    usvg::FillRule::EvenOdd => Fill::EvenOdd,
                },
            });
        }
    }

    if let Some(stroke) = path.stroke() {
        let mut outline = expand_stroke(&object, stroke);
        if !outline.is_empty() {
            outline.apply_affine(transform);
            parts.push(IconPart {
                path: outline,
                fill: Fill::NonZero,
            });
        }
    }
}

fn tiny_path_to_bez(path: &usvg::tiny_skia_path::Path) -> BezPath {
    let mut out = BezPath::new();
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(p) => out.move_to((f64::from(p.x), f64::from(p.y))),
            PathSegment::LineTo(p) => out.line_to((f64::from(p.x), f64::from(p.y))),
            PathSegment::QuadTo(p1, p2) => out.quad_to(
                (f64::from(p1.x), f64::from(p1.y)),
                (f64::from(p2.x), f64::from(p2.y)),
            ),
            PathSegment::CubicTo(p1, p2, p3) => out.curve_to(
                (f64::from(p1.x), f64::from(p1.y)),
                (f64::from(p2.x), f64::from(p2.y)),
                (f64::from(p3.x), f64::from(p3.y)),
            ),
            PathSegment::Close => out.close_path(),
        }
    }
    out
}

fn affine_from_usvg(transform: usvg::Transform) -> Affine {
    Affine::new([
        f64::from(transform.sx),
        f64::from(transform.ky),
        f64::from(transform.kx),
        f64::from(transform.sy),
        f64::from(transform.tx),
        f64::from(transform.ty),
    ])
}

fn expand_stroke(path: &BezPath, stroke: &usvg::Stroke) -> BezPath {
    let mut style = KurboStroke::new(f64::from(stroke.width().get()))
        .with_caps(match stroke.linecap() {
            usvg::LineCap::Butt => crate::kurbo::Cap::Butt,
            usvg::LineCap::Round => crate::kurbo::Cap::Round,
            usvg::LineCap::Square => crate::kurbo::Cap::Square,
        })
        .with_join(match stroke.linejoin() {
            usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => crate::kurbo::Join::Miter,
            usvg::LineJoin::Round => crate::kurbo::Join::Round,
            usvg::LineJoin::Bevel => crate::kurbo::Join::Bevel,
        })
        .with_miter_limit(f64::from(stroke.miterlimit().get()));

    if let Some(dashes) = stroke.dasharray() {
        let dashes: Vec<f64> = dashes
            .iter()
            .map(|dash| f64::from(*dash))
            .filter(|dash| dash.is_finite() && *dash > 0.0)
            .collect();
        if !dashes.is_empty() {
            style = style.with_dashes(f64::from(stroke.dashoffset()), dashes);
        }
    }

    kurbo::stroke(
        path.elements().iter().copied(),
        &style,
        &StrokeOpts::default(),
        0.1,
    )
}

fn normalize_extent(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::VectorIcon;
    use crate::image::ImageFit;
    use crate::kurbo::{Point, Rect};

    const HOME: &str = "M3 11 L12 3 L21 11 L21 21 L14 21 L14 14 L10 14 L10 21 L3 21 Z";

    #[test]
    fn path_icon_uses_its_bounding_box_as_view_box() {
        let icon = VectorIcon::from_svg_path(HOME).unwrap();
        let view_box = icon.view_box();

        assert_eq!(view_box.x0, 3.0);
        assert_eq!(view_box.y0, 3.0);
        assert_eq!(view_box.width(), 18.0);
        assert_eq!(view_box.height(), 18.0);
        assert!(!icon.is_empty());
    }

    #[test]
    fn view_box_maps_into_the_destination() {
        let icon = VectorIcon::from_svg_path(HOME)
            .unwrap()
            .with_view_box_rect(Rect::new(3.0, 3.0, 21.0, 21.0));
        let transform = icon
            .transform_for(Rect::new(0.0, 0.0, 18.0, 18.0), ImageFit::Fill)
            .unwrap();

        assert_eq!(transform * Point::new(3.0, 3.0), Point::new(0.0, 0.0));
        assert_eq!(transform * Point::new(21.0, 21.0), Point::new(18.0, 18.0));
    }

    #[test]
    fn complete_svg_keeps_its_canvas_size() {
        let svg = r##"
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
                <rect x="4" y="4" width="16" height="16" fill="#000"/>
                <path d="M2 2 L22 22" stroke="#000" stroke-width="2"/>
            </svg>
        "##;
        let icon = VectorIcon::from_svg(svg).unwrap();

        assert_eq!(icon.view_box(), Rect::new(0.0, 0.0, 24.0, 24.0));
        assert!(!icon.is_empty());
    }

    #[test]
    fn svg_strokes_are_expanded_into_geometry() {
        let svg = r##"
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
                <path d="M2 2 L22 22" fill="none" stroke="#000" stroke-width="2"/>
            </svg>
        "##;
        let icon = VectorIcon::from_svg(svg).unwrap();

        assert_eq!(icon.parts().len(), 1);
        assert!(!icon.parts()[0].path.is_empty());
        assert_eq!(icon.parts()[0].fill, crate::Fill::NonZero);
    }

    #[test]
    fn svg_fill_rule_is_preserved() {
        let svg = r##"
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
                <path fill="#000" fill-rule="evenodd" d="M2 2H22V22H2Z M8 8V16H16V8Z"/>
            </svg>
        "##;
        let icon = VectorIcon::from_svg(svg).unwrap();

        assert_eq!(icon.parts()[0].fill, crate::Fill::EvenOdd);
    }

    #[test]
    fn invalid_path_data_is_rejected() {
        assert!(VectorIcon::from_svg_path("not a path").is_err());
    }

    #[test]
    fn fill_rule_can_be_overridden_for_path_data() {
        let icon = VectorIcon::from_svg_path(HOME)
            .unwrap()
            .with_fill_rule(crate::Fill::EvenOdd);

        assert_eq!(icon.parts()[0].fill, crate::Fill::EvenOdd);
    }
}
