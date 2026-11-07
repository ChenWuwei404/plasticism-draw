//! R2 rounded rectangles: fill, stroke, and clipping.

use plasticism_draw::prelude::*;

fn main() -> anyhow::Result<()> {
    let font = Font::from_bytes(include_bytes!("../assets/Inter-Regular.ttf"))?;
    let mut phase = 0.0_f64;

    App::new("plasticism · R2 rounded rect", 960, 640)
        .background(colors::WHITE_SMOKE)
        .draw(move |c, frame| {
            phase += frame.delta.as_secs_f64();

            let left_shape = R2RoundedRect::new(60.0, 64.0, 440.0, 284.0, 48.0);
            let right_shape = R2RoundedRect::new(520.0, 64.0, 900.0, 284.0, 110.0);
            let banner_shape = R2RoundedRect::new(60.0, 336.0, 900.0, 580.0, 56.0);

            let left = left_shape.rect();
            let right = right_shape.rect();
            let banner = banner_shape.rect();

            let warm = Gradient::new_linear((left.x0, left.y0), (left.x1, left.y1)).with_stops([
                (0.0, colors::GOLD),
                (0.55, colors::ORANGE_RED),
                (1.0, colors::DEEP_PINK),
            ]);
            let cool = Gradient::new_linear((right.x0, right.y0), (right.x1, right.y1))
                .with_stops([(0.0, colors::DARK_SLATE_BLUE), (1.0, colors::LIGHT_CYAN)]);

            // R2RoundedRect implements kurbo::Shape, so the generic Canvas
            // fill/stroke methods accept it directly.
            c.fill(left_shape, &warm);
            c.stroke(left_shape, Stroke::new(4.0, colors::BLACK));

            c.fill(right_shape, &cool);
            c.stroke(
                right_shape,
                Stroke::new(3.0, colors::WHITE).dash(0.0, [12.0, 8.0]),
            );

            // Clip animated stripes and text to the R2 outline.
            let banner_path = banner_shape.to_path(0.1);
            c.with_clip(&banner_path, |c| {
                c.fill_rect(banner, &warm);

                for i in 0..14 {
                    let offset = (phase * 130.0 + f64::from(i) * 92.0)
                        .rem_euclid(banner.width() + 160.0)
                        - 80.0;
                    c.fill_rect(
                        Rect::from_origin_size(
                            (banner.x0 + offset, banner.y0),
                            (34.0, banner.height()),
                        ),
                        Paint::solid(colors::WHITE).with_alpha(0.13),
                    );
                }

                let label = "R2 ROUNDED RECT";
                let label_width = font.measure(label, 76.0).width;
                let label_x =
                    banner.x0 + (banner.width() - label_width) / 2.0 + (phase * 40.0).sin() * 18.0;
                c.fill_text(
                    &font,
                    label,
                    (label_x, banner.center().y + 26.0),
                    76.0,
                    Paint::solid(colors::WHITE).with_alpha(0.92),
                );
            });

            c.stroke(banner_shape, Stroke::new(4.0, colors::BLACK));

            c.fill_text(
                &font,
                "R = 48",
                (left.x0, left.y1 + 46.0),
                22.0,
                colors::DIM_GRAY,
            );
            c.fill_text(
                &font,
                "R = 110 (max)",
                (right.x0, right.y1 + 46.0),
                22.0,
                colors::DIM_GRAY,
            );
            c.fill_text(
                &font,
                "R2 profile · fill / stroke / clip",
                (banner.x0, banner.y1 + 28.0),
                18.0,
                colors::DIM_GRAY,
            );
        })
        .run()
}
