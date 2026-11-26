//! Shape-based drop shadows.

use plasticism_draw::prelude::*;

fn main() -> anyhow::Result<()> {
    App::new("plasticism · shadow", 900, 640)
        .background(colors::GHOST_WHITE)
        .draw(|c, frame, _window| {
            let t = frame.time.as_secs_f64();

            // A card: the shadow is drawn first, the shape on top of it.
            let card = Rect::from_origin_size((70.0, 70.0), (300.0, 190.0));
            c.draw_shadow(
                RoundedRect::from_rect(card, 22.0),
                Shadow::default().offset((0.0, 14.0)).blur(30.0),
            );
            c.fill_rounded_rect(card, 22.0, colors::WHITE);

            // A colored shadow that follows an animated circle.
            let center = Point::new(600.0, 190.0 + (t * 1.8).sin() * 62.0);
            c.draw_shadow(
                Circle::new(center, 72.0),
                Shadow::default()
                    .offset((14.0, 26.0))
                    .blur(24.0)
                    .color(Color::from_rgba8(40, 70, 210, 120)),
            );
            c.fill_circle(center, 72.0, colors::ROYAL_BLUE);

            // A Bezier path casts a shadow of its own silhouette. `BezPath` is
            // not a `Shape` by reference, so pass its elements.
            let star = star_path(Point::new(250.0, 470.0), 92.0, 40.0);
            c.draw_shadow(
                star.elements(),
                Shadow::default().offset((0.0, 16.0)).blur(22.0),
            );
            c.fill_path(&star, colors::GOLD);
            c.stroke_path(&star, Stroke::new(2.0, colors::GOLDENROD));

            // The shadow follows the current transform, so it rotates with the
            // square instead of staying axis-aligned.
            c.with_transform(Affine::rotate_about(t * 0.5, (700.0, 460.0)), |c| {
                let square = Rect::from_center_size((700.0, 460.0), (150.0, 150.0));
                c.draw_shadow(square, Shadow::default().offset((0.0, 12.0)).blur(20.0));
                c.fill_rect(square, colors::TOMATO);
            });
        })
        .run()
}

fn star_path(center: Point, outer: f64, inner: f64) -> BezPath {
    let mut path = BezPath::new();
    for index in 0..10 {
        let angle = -std::f64::consts::FRAC_PI_2 + f64::from(index) * std::f64::consts::PI / 5.0;
        let radius = if index % 2 == 0 { outer } else { inner };
        let point = center + Vec2::from_angle(angle) * radius;
        if index == 0 {
            path.move_to(point);
        } else {
            path.line_to(point);
        }
    }
    path.close_path();
    path
}
