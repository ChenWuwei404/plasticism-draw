//! DPI mode demo: compare automatic DPI scaling with a fixed scale.
//!
//! Without an argument the example uses `DpiMode::Auto` and follows the
//! monitor. Pass a scale factor to force one instead:
//!
//! ```text
//! cargo run --example dpi -- 1.0   # one canvas unit per physical pixel
//! cargo run --example dpi -- 2.0   # always 200%
//! ```

use plasticism_draw::prelude::*;

fn main() -> anyhow::Result<()> {
    let mode = match std::env::args().nth(1) {
        None => DpiMode::Auto,
        Some(value) => {
            let scale = value
                .parse::<f64>()
                .map_err(|_| anyhow::anyhow!("expected a numeric scale factor, got {value:?}"))?;
            DpiMode::Fixed(scale)
        }
    };

    let font = Font::from_bytes(include_bytes!("../assets/Inter-Regular.ttf"))?;

    App::new(format!("plasticism · dpi · {mode:?}"), 900, 640)
        .dpi_mode(mode)
        .background(colors::WHITE)
        .draw(move |c, _frame| {
            let width = c.width();
            let height = c.height();

            // 50-unit grid across the whole canvas.
            let grid = Stroke::new(1.0, Color::from_rgb8(226, 232, 240));
            let mut x = 0.0;
            while x <= width {
                c.line((x, 0.0), (x, height), &grid);
                x += 50.0;
            }
            let mut y = 0.0;
            while y <= height {
                c.line((0.0, y), (width, y), &grid);
                y += 50.0;
            }

            // A 1-unit checkerboard: compare its crispness between modes.
            let checker_origin = (40.0, 150.0);
            for row in 0..48 {
                for column in 0..48 {
                    if (row + column) % 2 == 0 {
                        c.fill_rect(
                            Rect::from_origin_size(
                                (
                                    checker_origin.0 + f64::from(column),
                                    checker_origin.1 + f64::from(row),
                                ),
                                (1.0, 1.0),
                            ),
                            Color::from_rgb8(148, 163, 184),
                        );
                    }
                }
            }
            c.stroke_rect(
                Rect::from_origin_size(checker_origin, (48.0, 48.0)),
                Stroke::new(1.0, colors::SLATE_GRAY),
            );

            // Shapes relative to the canvas size so every mode stays visible.
            let center = (width * 0.55, height * 0.55);
            let radius = (width.min(height) * 0.16).max(30.0);
            c.fill_circle(center, radius, colors::CRIMSON);
            c.stroke_circle(center, radius + 24.0, Stroke::new(3.0, colors::BLACK));

            let card = Rect::from_center_size(
                (width * 0.55, height * 0.82),
                (width * 0.34, height * 0.14),
            );
            c.fill_r2_rounded_rect(card, 40.0, colors::SKY_BLUE);
            c.stroke_r2_rounded_rect(card, 40.0, Stroke::new(2.0, colors::BLACK));

            let status = [
                format!("mode {mode:?} · scale {:.2}", c.scale_factor()),
                format!("canvas {width:.1} × {height:.1} units"),
                String::from("grid 50 units · checker 1 unit per cell"),
            ];
            for (index, line) in status.iter().enumerate() {
                c.fill_text(
                    &font,
                    line,
                    (40.0, 50.0 + index as f64 * 30.0),
                    20.0,
                    colors::DARK_SLATE_GRAY,
                );
            }
        })
        .run()
}
