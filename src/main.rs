use plasticism_draw::prelude::*;

fn main() -> anyhow::Result<()> {
    let font = Font::from_bytes(include_bytes!("../assets/Inter-Regular.ttf"))?;
    let sky = Gradient::new_linear((0.0, 0.0), (0.0, 680.0))
        .with_stops([(0.0, colors::ALICE_BLUE), (1.0, colors::WHITE_SMOKE)]);
    let mut angle = 0.0_f64;

    App::new("plasticism-draw", 960, 680)
        .background(colors::WHITE)
        .run(move |c, frame| {
            angle += frame.delta.as_secs_f64() * 16.0;

            c.fill_rect(
                Rect::from_origin_size((0.0, 0.0), (c.width(), c.height())),
                &sky,
            );

            let center = (c.width() / 2.0, c.height() / 2.0);

            c.with_opacity(0.16, |c| {
                for ring in 1..=4 {
                    let radius = 60.0 + f64::from(ring) * 52.0;
                    c.stroke_circle(center, radius, Stroke::new(1.0, colors::SLATE_GRAY));
                }
            });

            c.with_transform(Affine::rotate_about(angle, center), |c| {
                let blob = Rect::from_center_size(center, (170.0, 92.0));
                c.fill_rounded_rect(blob, 34.0, colors::CRIMSON);
                c.stroke_rounded_rect(blob, 34.0, Stroke::new(2.0, colors::BLACK));
            });

            c.with_transform(Affine::rotate_about(-angle * 0.6, center), |c| {
                c.fill_circle(center, 18.0, colors::GOLD);
                c.stroke_circle(center, 34.0, Stroke::new(2.0, colors::BLACK));
            });

            let dt_ms = frame.delta.as_secs_f64() * 1000.0;
            let label = format!(
                "frame {}  ·  dt {:.2} ms  ·  {:.1} s",
                frame.index,
                dt_ms,
                frame.time.as_secs_f64()
            );
            c.fill_text(
                &font,
                &label,
                (28.0, c.height() - 28.0),
                18.0,
                colors::DIM_GRAY,
            );
        })
}
