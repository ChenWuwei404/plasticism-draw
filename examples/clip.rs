//! A rounded-rectangle clip containing moving text and shapes.

use std::f64::consts::TAU;

use plasticism_draw::prelude::*;

fn main() -> anyhow::Result<()> {
    let font = Font::from_bytes(include_bytes!("../assets/Inter-Regular.ttf"))?;
    let mut phase = 0.0_f64;

    App::new("plasticism · clip", 960, 680)
        .background(colors::WHITE_SMOKE)
        .draw(move |c, frame| {
            phase += frame.delta.as_secs_f64();

            let center = (c.width() / 2.0, c.height() / 2.0);
            let card = Rect::from_center_size(center, (740.0, 430.0));
            let radius = 58.0;

            let backdrop =
                Gradient::new_linear((card.x0, card.y0), (card.x1, card.y1)).with_stops([
                    (0.0, colors::DARK_SLATE_BLUE),
                    (0.55, colors::INDIGO),
                    (1.0, colors::BLACK),
                ]);

            c.with_clip(RoundedRect::from_rect(card, radius), |c| {
                c.fill_rect(card, &backdrop);

                // A large rotating block crosses the clip edges.
                c.with_transform(Affine::rotate_about(phase * 0.35, center), |c| {
                    c.fill_rounded_rect(
                        Rect::from_center_size(center, (420.0, 230.0)),
                        52.0,
                        Paint::solid(colors::DEEP_PINK).with_alpha(0.36),
                    );
                });

                // Orbiting dots also pass through the rounded clip boundary.
                for i in 0..10 {
                    let angle = phase * 0.9 + f64::from(i) * TAU / 10.0;
                    let position = (
                        center.0 + angle.cos() * 340.0,
                        center.1 + angle.sin() * 200.0,
                    );
                    let dot = 12.0 + (phase * 2.0 + f64::from(i)).sin().abs() * 16.0;
                    c.fill_circle(position, dot, colors::GOLD);
                }

                // Marquee text: one copy is enough because the card is narrower
                // than the text plus its travel distance.
                let title = "MOTION";
                let title_size = 168.0;
                let title_width = font.measure(title, title_size).width;
                let travel = card.width() + title_width;
                let title_x = card.x0 - title_width + (phase * 140.0).rem_euclid(travel);
                c.fill_text(
                    &font,
                    title,
                    (title_x, center.1 + 66.0),
                    title_size,
                    Paint::solid(colors::WHITE).with_alpha(0.92),
                );

                // A second line moves the other way, behind the first.
                let subtitle = "rounded-rect clip";
                let subtitle_size = 38.0;
                let subtitle_width = font.measure(subtitle, subtitle_size).width;
                let travel_back = card.width() + subtitle_width;
                let subtitle_x = card.x1 - (phase * 85.0).rem_euclid(travel_back);
                c.fill_text(
                    &font,
                    subtitle,
                    (subtitle_x, card.y0 + 86.0),
                    subtitle_size,
                    colors::LIGHT_CYAN,
                );

                // A fixed label inside the clip, for contrast with the moving text.
                c.fill_text(
                    &font,
                    "inside the rounded rectangle",
                    (card.x0 + 36.0, card.y1 - 34.0),
                    18.0,
                    Paint::solid(colors::WHITE).with_alpha(0.72),
                );
            });

            // The border is drawn outside the clip so the boundary stays crisp.
            c.stroke_rounded_rect(card, radius, Stroke::new(4.0, colors::BLACK));

            c.fill_text(
                &font,
                "everything above is clipped by a RoundedRect",
                (card.x0, card.y1 + 54.0),
                20.0,
                colors::DIM_GRAY,
            );
        })
        .run()
}
