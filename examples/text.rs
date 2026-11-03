//! Filled and stroked text using a bundled Open Font License typeface.

use plasticism_draw::prelude::*;

fn main() -> anyhow::Result<()> {
    let font = Font::from_bytes(include_bytes!("../assets/Inter-Regular.ttf"))?;

    App::new("plasticism · text", 960, 640)
        .background(colors::WHITE)
        .run(move |c, frame| {
            let measured = font.measure("Hello, plasticism", 64.0);
            let x = (c.width() - measured.width) / 2.0;

            c.fill_text(&font, "Hello, plasticism", (x, 250.0), 64.0, colors::BLACK);

            let outline = "rendered by vello + winit";
            let outline_width = font.measure(outline, 32.0).width;
            c.stroke_text(
                &font,
                outline,
                ((c.width() - outline_width) / 2.0, 330.0),
                32.0,
                Stroke::new(1.25, colors::CRIMSON),
            );

            c.fill_text(
                &font,
                "Text supports multiple lines.\nAnd uses actual frame delta.",
                (110.0, 450.0),
                28.0,
                colors::DIM_GRAY,
            );

            let delta_ms = frame.delta.as_secs_f64() * 1000.0;
            let status = format!("delta: {delta_ms:.2} ms");
            c.fill_text(
                &font,
                &status,
                (110.0, 560.0),
                18.0,
                colors::LIGHT_SLATE_GRAY,
            );
        })
}
