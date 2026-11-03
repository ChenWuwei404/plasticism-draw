//! Gradients, clipping, opacity, and delta-time animation.

use plasticism_draw::prelude::*;

fn main() -> anyhow::Result<()> {
    let sky = Gradient::new_linear((0.0, 0.0), (0.0, 660.0))
        .with_stops([(0.0, colors::SKY_BLUE), (1.0, colors::WHITE)]);
    let sunset = Gradient::new_radial((450.0, 330.0), 260.0).with_stops([
        (0.0, colors::GOLD),
        (0.55, colors::ORANGE_RED),
        (1.0, colors::TRANSPARENT),
    ]);
    let mut angle = 0.0_f64;

    App::new("plasticism · gradients", 900, 660).run(move |c, frame| {
        angle += frame.delta.as_secs_f64() * 0.8;

        c.fill_rect(
            Rect::from_origin_size((0.0, 0.0), (c.width(), c.height())),
            &sky,
        );
        c.fill_rect(
            Rect::from_origin_size((0.0, 0.0), (c.width(), c.height())),
            &sunset,
        );

        c.with_clip(Circle::new((450.0, 330.0), 240.0), |c| {
            c.with_transform(Affine::rotate_about(angle, (450.0, 330.0)), |c| {
                for i in 0..12 {
                    let offset = f64::from(i) * 34.0 - 180.0;
                    c.fill_rect(
                        Rect::from_center_size((450.0 + offset, 330.0), (18.0, 360.0)),
                        Paint::solid(colors::WHITE).with_alpha(0.35),
                    );
                }
            });
        });

        c.with_opacity(0.4, |c| {
            c.fill_circle((450.0, 330.0), 300.0, colors::BLACK);
        });

        c.stroke_circle((450.0, 330.0), 240.0, Stroke::new(2.0, colors::BLACK));
    })
}
