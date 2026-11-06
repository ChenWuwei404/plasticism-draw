//! The drawing surface passed to the render callback.

use glifo::Glyph;
use vello_gpu::peniko::{Fill, ImageSampler};
use vello_gpu::{Image as VelloImage, ImageSource, Renderer, Resources, Scene, Tint, TintMode};

use crate::Color;
use crate::dpi::normalize_scale;
use crate::icon::VectorIcon;
use crate::image::{Image, ImageFit, fitted_rect};
use crate::kurbo::{
    Affine, BezPath, Circle, Ellipse, Line, Point, Rect, RoundedRect, RoundedRectRadii, Shape,
    Size, Vec2,
};
use crate::paint::{Paint, Stroke};
use crate::r2::R2RoundedRect;
use crate::text::Font;

const TOLERANCE: f64 = 0.1;

/// A drawing surface whose coordinates are canvas units.
///
/// Coordinates have their origin at the top-left corner and the y axis
/// pointing down. By default the window's scale factor is applied internally,
/// so coordinates are logical pixels independent of the display DPI. Use
/// [`App::dpi_mode`](crate::App::dpi_mode) to render in physical pixels or at
/// an explicit scale instead.
///
/// A `Canvas` is only valid for the duration of one render callback. It is
/// reset at the start of every frame and must not be stored by the callback.
pub struct Canvas {
    scene: Scene,
    resources: Resources,
    canvas_width: f64,
    canvas_height: f64,
    scale_factor: f64,
    user_transform: Affine,
    fill_rule: Fill,
    open: Vec<Open>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Open {
    Clip,
    Opacity,
}

impl Canvas {
    pub(crate) fn new(resources: Resources, width: u16, height: u16, scale_factor: f64) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let scale_factor = normalize_scale(scale_factor);

        Self {
            scene: Scene::new(width, height),
            resources,
            canvas_width: f64::from(width) / scale_factor,
            canvas_height: f64::from(height) / scale_factor,
            scale_factor,
            user_transform: Affine::IDENTITY,
            fill_rule: Fill::NonZero,
            open: Vec::new(),
        }
    }

    pub(crate) fn begin_frame(&mut self, width: u16, height: u16, scale_factor: f64) {
        let width = width.max(1);
        let height = height.max(1);
        self.scale_factor = normalize_scale(scale_factor);
        self.canvas_width = f64::from(width) / self.scale_factor;
        self.canvas_height = f64::from(height) / self.scale_factor;
        self.user_transform = Affine::IDENTITY;
        self.fill_rule = Fill::NonZero;
        self.open.clear();
        self.scene.reset_and_resize(width, height);
        self.apply_transform();
    }

    pub(crate) fn finish_frame(&mut self) {
        while let Some(open) = self.open.pop() {
            match open {
                Open::Clip => self.scene.pop_clip(),
                Open::Opacity => self.scene.pop_layer(),
            }
        }
    }

    pub(crate) fn parts(&mut self) -> (&Scene, &mut Resources) {
        (&self.scene, &mut self.resources)
    }

    pub(crate) fn upload_pending_images(
        &mut self,
        renderer: &mut Renderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        for image in crate::image::take_pending() {
            if image.id.get().is_none() {
                let id = renderer.upload_image(
                    &mut self.resources,
                    device,
                    queue,
                    encoder,
                    &image.pixmap,
                );
                let _ = image.id.set(id);
            }
        }
    }

    pub(crate) fn destroy_pending_images(
        &mut self,
        renderer: &mut Renderer,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        for id in crate::image::take_destroyed() {
            renderer.destroy_image(&mut self.resources, encoder, id);
        }
    }

    /// Returns the width of the drawable area in canvas units.
    pub fn width(&self) -> f64 {
        self.canvas_width
    }

    /// Returns the height of the drawable area in canvas units.
    pub fn height(&self) -> f64 {
        self.canvas_height
    }

    /// Returns the size of the drawable area in canvas units.
    pub fn size(&self) -> crate::kurbo::Size {
        crate::kurbo::Size::new(self.canvas_width, self.canvas_height)
    }

    /// Returns the number of physical pixels per canvas unit.
    ///
    /// This is the effective scale selected by the application's
    /// [`DpiMode`](crate::DpiMode): the window's scale factor for
    /// [`DpiMode::Auto`](crate::DpiMode::Auto), or the fixed value for
    /// [`DpiMode::Fixed`](crate::DpiMode::Fixed).
    pub fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    /// Replaces the current user transform.
    ///
    /// The effective transform is the canvas DPI scale multiplied by this
    /// transform, so coordinates remain in canvas units.
    pub fn set_transform(&mut self, transform: Affine) {
        self.user_transform = transform;
        self.apply_transform();
    }

    /// Resets the current user transform to the identity.
    pub fn reset_transform(&mut self) {
        self.set_transform(Affine::IDENTITY);
    }

    /// Runs `f` with `transform` composed with the current transform.
    ///
    /// The new transform is `current * transform`, so `transform` is applied
    /// first (local coordinates).
    pub fn with_transform<R>(&mut self, transform: Affine, f: impl FnOnce(&mut Self) -> R) -> R {
        let previous = self.user_transform;
        self.set_transform(previous * transform);
        let result = f(self);
        self.set_transform(previous);
        result
    }

    /// Pushes a clipping region described by `shape`.
    ///
    /// Every call must be paired with [`Canvas::pop_clip`], or use
    /// [`Canvas::with_clip`]. Unpaired clips are closed automatically at the
    /// end of the frame.
    pub fn push_clip(&mut self, shape: impl Shape) {
        let path = shape.into_path(TOLERANCE);
        self.scene.push_clip_path(&path);
        self.open.push(Open::Clip);
    }

    /// Pops the most recently pushed clipping region.
    pub fn pop_clip(&mut self) {
        if self.open.last() == Some(&Open::Clip) {
            self.open.pop();
            self.scene.pop_clip();
        } else {
            debug_assert!(false, "pop_clip called without a matching push_clip");
        }
    }

    /// Runs `f` inside a clipping region described by `shape`.
    pub fn with_clip<R>(&mut self, shape: impl Shape, f: impl FnOnce(&mut Self) -> R) -> R {
        self.push_clip(shape);
        let result = f(self);
        self.pop_clip();
        result
    }

    /// Pushes an opacity layer.
    ///
    /// Every call must be paired with [`Canvas::pop_opacity`], or use
    /// [`Canvas::with_opacity`]. Unpaired layers are closed automatically at
    /// the end of the frame.
    pub fn push_opacity(&mut self, opacity: f32) {
        self.scene.push_opacity_layer(opacity.clamp(0.0, 1.0));
        self.open.push(Open::Opacity);
    }

    /// Pops the most recently pushed opacity layer.
    pub fn pop_opacity(&mut self) {
        if self.open.last() == Some(&Open::Opacity) {
            self.open.pop();
            self.scene.pop_layer();
        } else {
            debug_assert!(false, "pop_opacity called without a matching push_opacity");
        }
    }

    /// Runs `f` inside an opacity layer.
    pub fn with_opacity<R>(&mut self, opacity: f32, f: impl FnOnce(&mut Self) -> R) -> R {
        self.push_opacity(opacity);
        let result = f(self);
        self.pop_opacity();
        result
    }

    /// Sets the fill rule used by subsequent fill operations.
    pub fn set_fill_rule(&mut self, fill: Fill) {
        self.fill_rule = fill;
        self.scene.set_fill_rule(fill);
    }

    /// Fills an arbitrary kurbo shape.
    pub fn fill(&mut self, shape: impl Shape, paint: impl Into<Paint>) {
        let path = shape.into_path(TOLERANCE);
        self.fill_path(&path, paint);
    }

    /// Strokes an arbitrary kurbo shape.
    pub fn stroke(&mut self, shape: impl Shape, stroke: impl Into<Stroke>) {
        let path = shape.into_path(TOLERANCE);
        self.stroke_path(&path, stroke);
    }

    /// Fills a rectangle.
    pub fn fill_rect(&mut self, rect: Rect, paint: impl Into<Paint>) {
        self.scene.set_paint(paint.into().into_inner());
        self.scene.fill_rect(&rect);
    }

    /// Strokes a rectangle.
    pub fn stroke_rect(&mut self, rect: Rect, stroke: impl Into<Stroke>) {
        let stroke = stroke.into();
        self.apply_stroke(&stroke);
        self.scene.stroke_rect(&rect);
    }

    /// Fills a circle.
    pub fn fill_circle(&mut self, center: impl Into<Point>, radius: f64, paint: impl Into<Paint>) {
        self.fill(Circle::new(center, radius.max(0.0)), paint);
    }

    /// Strokes a circle.
    pub fn stroke_circle(
        &mut self,
        center: impl Into<Point>,
        radius: f64,
        stroke: impl Into<Stroke>,
    ) {
        self.stroke(Circle::new(center, radius.max(0.0)), stroke);
    }

    /// Fills an axis-aligned ellipse.
    pub fn fill_ellipse(
        &mut self,
        center: impl Into<Point>,
        radii: impl Into<Vec2>,
        paint: impl Into<Paint>,
    ) {
        let radii = radii.into();
        self.fill(
            Ellipse::new(center, Vec2::new(radii.x.max(0.0), radii.y.max(0.0)), 0.0),
            paint,
        );
    }

    /// Strokes an axis-aligned ellipse.
    pub fn stroke_ellipse(
        &mut self,
        center: impl Into<Point>,
        radii: impl Into<Vec2>,
        stroke: impl Into<Stroke>,
    ) {
        let radii = radii.into();
        self.stroke(
            Ellipse::new(center, Vec2::new(radii.x.max(0.0), radii.y.max(0.0)), 0.0),
            stroke,
        );
    }

    /// Fills a rounded rectangle.
    pub fn fill_rounded_rect(
        &mut self,
        rect: Rect,
        radii: impl Into<RoundedRectRadii>,
        paint: impl Into<Paint>,
    ) {
        self.fill(RoundedRect::from_rect(rect, radii.into()), paint);
    }

    /// Strokes a rounded rectangle.
    pub fn stroke_rounded_rect(
        &mut self,
        rect: Rect,
        radii: impl Into<RoundedRectRadii>,
        stroke: impl Into<Stroke>,
    ) {
        self.stroke(RoundedRect::from_rect(rect, radii.into()), stroke);
    }

    /// Fills an R2 rounded rectangle.
    ///
    /// `radius` is the `R` from the R2 corner profile. The profile extends
    /// `0.528665 * radius` beyond the nominal tangent points, and is clamped
    /// when it would not fit inside `rect`.
    pub fn fill_r2_rounded_rect(&mut self, rect: Rect, radius: f64, paint: impl Into<Paint>) {
        self.fill(R2RoundedRect::from_rect(rect, radius), paint);
    }

    /// Strokes an R2 rounded rectangle.
    ///
    /// `radius` is the `R` from the R2 corner profile. The profile extends
    /// `0.528665 * radius` beyond the nominal tangent points, and is clamped
    /// when it would not fit inside `rect`.
    pub fn stroke_r2_rounded_rect(&mut self, rect: Rect, radius: f64, stroke: impl Into<Stroke>) {
        self.stroke(R2RoundedRect::from_rect(rect, radius), stroke);
    }

    /// Fills a closed polygon. Fewer than three points draw nothing.
    pub fn fill_polygon(
        &mut self,
        points: impl IntoIterator<Item = impl Into<Point>>,
        paint: impl Into<Paint>,
    ) {
        if let Some(path) = polygon_path(points, 3) {
            self.fill_path(&path, paint);
        }
    }

    /// Strokes a closed polygon. Fewer than two points draw nothing.
    pub fn stroke_polygon(
        &mut self,
        points: impl IntoIterator<Item = impl Into<Point>>,
        stroke: impl Into<Stroke>,
    ) {
        if let Some(path) = polygon_path(points, 2) {
            self.stroke_path(&path, stroke);
        }
    }

    /// Strokes an open polyline. Fewer than two points draw nothing.
    pub fn stroke_polyline(
        &mut self,
        points: impl IntoIterator<Item = impl Into<Point>>,
        stroke: impl Into<Stroke>,
    ) {
        if let Some(path) = polyline_path(points) {
            self.stroke_path(&path, stroke);
        }
    }

    /// Draws a straight line segment.
    pub fn line(
        &mut self,
        from: impl Into<Point>,
        to: impl Into<Point>,
        stroke: impl Into<Stroke>,
    ) {
        self.stroke(Line::new(from.into(), to.into()), stroke);
    }

    /// Fills an existing Bezier path.
    pub fn fill_path(&mut self, path: &BezPath, paint: impl Into<Paint>) {
        self.scene.set_paint(paint.into().into_inner());
        self.scene.fill_path(path);
    }

    /// Strokes an existing Bezier path.
    pub fn stroke_path(&mut self, path: &BezPath, stroke: impl Into<Stroke>) {
        let stroke = stroke.into();
        self.apply_stroke(&stroke);
        self.scene.stroke_path(path);
    }

    /// Draws a bitmap stretched to fill `rect`.
    ///
    /// The image is uploaded to the GPU automatically and cached. An image
    /// created inside the current draw callback becomes visible on the next
    /// frame. Use [`Canvas::draw_image_fit`] to preserve the aspect ratio.
    pub fn draw_image(&mut self, image: &Image, rect: Rect) {
        self.draw_image_fit(image, rect, ImageFit::Fill);
    }

    /// Draws a bitmap fitted into `rect`.
    ///
    /// [`ImageFit::Fill`] stretches the image, [`ImageFit::Contain`] keeps the
    /// aspect ratio and leaves empty space, and [`ImageFit::Cover`] keeps the
    /// aspect ratio and crops the overflow. The result is centered.
    pub fn draw_image_fit(&mut self, image: &Image, rect: Rect, fit: ImageFit) {
        let Some(id) = image.gpu_id() else {
            image.request_upload();
            return;
        };

        let source = image.size();
        let Some(dst) = fitted_rect(source, rect, fit) else {
            return;
        };

        self.scene.set_paint(VelloImage {
            image: ImageSource::opaque_id_with_transparency_hint(id, image.may_have_transparency()),
            sampler: ImageSampler::default(),
        });
        self.scene.set_paint_transform(image_transform(source, dst));

        if fit == ImageFit::Cover {
            self.push_clip(rect.abs());
            self.scene.fill_rect(&dst);
            self.pop_clip();
        } else {
            self.scene.fill_rect(&dst);
        }

        self.scene.reset_paint_transform();
    }

    /// Draws a vector icon fitted into `rect` with the supplied paint.
    ///
    /// The icon is scaled proportionally and centered. Its internal view box is
    /// mapped to a rectangle with the same aspect ratio as the icon, so a
    /// non-square destination leaves empty space rather than distorting the
    /// icon.
    pub fn draw_icon(&mut self, icon: &VectorIcon, rect: Rect, paint: impl Into<Paint>) {
        let Some(transform) = icon.transform_for(rect, ImageFit::Contain) else {
            return;
        };

        let paint = paint.into();
        let previous_fill_rule = self.fill_rule;
        for part in icon.parts() {
            let mut path = part.path.clone();
            path.apply_affine(transform);
            self.set_fill_rule(part.fill);
            self.fill_path(&path, paint.clone());
        }
        self.set_fill_rule(previous_fill_rule);
    }

    /// Draws a bitmap icon by using its alpha channel as a mask.
    ///
    /// Every fragment of the image is replaced by `color`, weighted by the
    /// source alpha. The icon is scaled proportionally, centered, and fitted
    /// into `rect`. Only the alpha channel of the bitmap is used.
    pub fn draw_bitmap_icon(&mut self, image: &Image, rect: Rect, color: impl Into<Color>) {
        let Some(id) = image.gpu_id() else {
            image.request_upload();
            return;
        };

        let source = image.size();
        let Some(dst) = fitted_rect(source, rect, ImageFit::Contain) else {
            return;
        };

        self.scene.set_paint(VelloImage {
            image: ImageSource::opaque_id_with_transparency_hint(id, true),
            sampler: ImageSampler::default(),
        });
        self.scene.set_paint_transform(image_transform(source, dst));
        self.scene.set_tint(Some(Tint {
            color: color.into(),
            mode: TintMode::AlphaMask,
        }));
        self.scene.fill_rect(&dst);
        self.scene.reset_tint();
        self.scene.reset_paint_transform();
    }

    /// Fills multi-line text.
    ///
    /// `baseline` is the baseline of the first line. Newline characters start
    /// a new line, and each line uses a line height of `1.2 * size`. Glyphs
    /// missing from the font render as `.notdef`.
    pub fn fill_text(
        &mut self,
        font: &Font,
        text: &str,
        baseline: impl Into<Point>,
        size: f64,
        paint: impl Into<Paint>,
    ) {
        let layout = font.layout(text, size);
        if layout.glyphs.is_empty() {
            return;
        }

        let baseline = baseline.into();
        let glyphs = layout.glyphs.iter().map(|glyph| Glyph {
            id: glyph.id,
            x: glyph.x + baseline.x as f32,
            y: glyph.y + baseline.y as f32,
        });

        self.scene.set_paint(paint.into().into_inner());
        let _ = self
            .scene
            .glyph_run(&mut self.resources, font.data())
            .font_size(size as f32)
            .atlas_cache(true)
            .fill_glyphs(glyphs);
    }

    /// Strokes multi-line text.
    ///
    /// The stroke's paint and width are used for the glyph outlines.
    pub fn stroke_text(
        &mut self,
        font: &Font,
        text: &str,
        baseline: impl Into<Point>,
        size: f64,
        stroke: impl Into<Stroke>,
    ) {
        let layout = font.layout(text, size);
        if layout.glyphs.is_empty() {
            return;
        }

        let stroke = stroke.into();
        let baseline = baseline.into();
        let glyphs = layout.glyphs.iter().map(|glyph| Glyph {
            id: glyph.id,
            x: glyph.x + baseline.x as f32,
            y: glyph.y + baseline.y as f32,
        });

        self.apply_stroke(&stroke);
        let _ = self
            .scene
            .glyph_run(&mut self.resources, font.data())
            .font_size(size as f32)
            .atlas_cache(true)
            .stroke_glyphs(glyphs);
    }

    fn apply_stroke(&mut self, stroke: &Stroke) {
        self.scene.set_paint(stroke.as_paint().clone().into_inner());
        self.scene.set_stroke(stroke.to_kurbo());
    }

    fn apply_transform(&mut self) {
        self.scene
            .set_transform(Affine::scale(self.scale_factor) * self.user_transform);
    }
}

fn image_transform(source: Size, dst: Rect) -> Affine {
    Affine::translate((dst.x0, dst.y0))
        * Affine::scale_non_uniform(dst.width() / source.width, dst.height() / source.height)
}

fn build_path(points: impl IntoIterator<Item = impl Into<Point>>) -> (BezPath, usize) {
    let mut path = BezPath::new();
    let mut count = 0_usize;

    for point in points {
        let point = point.into();
        if count == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
        count += 1;
    }

    (path, count)
}

fn polyline_path(points: impl IntoIterator<Item = impl Into<Point>>) -> Option<BezPath> {
    let (path, count) = build_path(points);
    (count >= 2).then_some(path)
}

fn polygon_path(
    points: impl IntoIterator<Item = impl Into<Point>>,
    minimum: usize,
) -> Option<BezPath> {
    let (mut path, count) = build_path(points);
    if count >= minimum {
        path.close_path();
        Some(path)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::polygon_path;
    use crate::dpi::normalize_scale;

    #[test]
    fn scale_factor_is_normalized() {
        assert_eq!(normalize_scale(2.0), 2.0);
        assert_eq!(normalize_scale(0.0), 1.0);
        assert_eq!(normalize_scale(f64::NAN), 1.0);
    }

    #[test]
    fn polygon_requires_minimum_points() {
        assert!(polygon_path([(0.0, 0.0), (1.0, 0.0)], 3).is_none());
        assert!(polygon_path([(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], 3).is_some());
    }
}
