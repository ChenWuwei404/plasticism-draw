//! Per-pixel window transparency.
//!
//! The window clears to transparent black by default, so every pixel that is
//! never painted lets the desktop through. The alpha of a paint controls how
//! much of the desktop shows through a shape, and an even-odd path cuts a real
//! hole instead of painting a transparent color.

use plasticism_draw::prelude::*;

fn main() -> anyhow::Result<()> {
    let font = Font::from_bytes(include_bytes!("../assets/Inter-Regular.ttf"))?;

    App::new("transparent", 560, 380)
        .transparent(true)
        .draw(move |c, _frame| {
            let (w, h) = (c.width(), c.height());

            // The card covers most of the window but leaves a transparent
            // margin, so its rounded corners fade into the desktop. The inner
            // circle belongs to the same even-odd path: those pixels are never
            // painted, so they stay fully transparent.
            let card = Rect::from_origin_size((16.0, 16.0), (w - 32.0, h - 32.0));
            let mut card_path = BezPath::new();
            card_path.extend(RoundedRect::from_rect(card, 26.0).path_elements(0.1));
            card_path.extend(Circle::new((452.0, 286.0), 44.0).path_elements(0.1));

            c.set_fill_rule(Fill::EvenOdd);
            c.fill_path(&card_path, Color::from_rgb8(26, 27, 32).with_alpha(0.92));
            c.set_fill_rule(Fill::NonZero);

            // A 35% panel: the desktop behind the window shows through it.
            c.fill_rounded_rect(
                Rect::from_origin_size((44.0, 52.0), (210.0, 132.0)),
                18.0,
                colors::WHITE.with_alpha(0.35),
            );

            // Paint alpha is the whole API: opaque text, then 70% text.
            c.fill_text(&font, "alpha", (66.0, 108.0), 32.0, colors::WHITE);
            c.fill_text(
                &font,
                "35% panel",
                (66.0, 146.0),
                16.0,
                colors::WHITE.with_alpha(0.7),
            );

            // A group opacity multiplies every shape drawn inside it.
            c.with_opacity(0.6, |c| {
                c.fill_circle((388.0, 118.0), 54.0, colors::DEEP_SKY_BLUE);
                c.fill_text(&font, "60%", (358.0, 128.0), 22.0, colors::WHITE);
            });

            c.fill_text(
                &font,
                "the circle is an even-odd hole",
                (64.0, 344.0),
                14.0,
                colors::WHITE.with_alpha(0.55),
            );
        })
        .run()
}
