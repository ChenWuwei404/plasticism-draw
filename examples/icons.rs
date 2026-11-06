use plasticism_draw::prelude::*;

const VIEW_WIDTH: f64 = 960.0;
const VIEW_HEIGHT: f64 = 640.0;

fn main() -> anyhow::Result<()> {
    let home = VectorIcon::from_svg_file(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/home.svg"))?;
    let spark =
        VectorIcon::from_svg_file(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/spark.svg"))?;
    let ring = VectorIcon::from_svg_file(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/ring.svg"))?;
    let heart = VectorIcon::from_svg_path(
        "M12 21s-7.5-4.7-9.7-9.2C.6 8.3 2.7 4.5 6.6 4.5c2 0 3.5 1.1 4.4 2.4 \
         .9-1.3 2.4-2.4 4.4-2.4 3.9 0 6 3.8 4.3 7.3C19.5 16.3 12 21 12 21z",
    )?
    .with_view_box(24.0, 24.0);

    let star = Image::open(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/star.png"))?;
    let ring_image = Image::open(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/ring.png"))?;
    let font = Font::from_bytes(include_bytes!("../assets/Inter-Regular.ttf"))?;

    App::new("icons", 900, 600)
        .background(colors::WHITE)
        .draw(move |c, frame| {
            c.set_transform(fit_view(c));

            c.fill_text(
                &font,
                "Vector icons · SVG paths · alpha-mask bitmap icons",
                (48.0, 54.0),
                30.0,
                colors::SLATE_GRAY,
            );

            c.fill_text(
                &font,
                "VectorIcon (filled with Paint)",
                (48.0, 104.0),
                18.0,
                colors::DIM_GRAY,
            );

            // The same icon at several sizes and colors.
            let icons = [
                (&home, colors::BLACK),
                (&heart, colors::CRIMSON),
                (&ring, colors::SKY_BLUE),
                (&spark, colors::DARK_ORCHID),
            ];
            for (index, (icon, color)) in icons.iter().enumerate() {
                let x = 90.0 + index as f64 * 105.0;
                let size = [42.0_f64, 56.0, 70.0, 84.0][index];
                let rect = Rect::from_center_size((x, 178.0), (size, size));
                c.fill_rect(rect.inflate(10.0, 10.0), colors::GHOST_WHITE);
                c.draw_icon(icon, rect, *color);
            }

            // Vector icons accept gradients too.
            let gradient = Gradient::new_linear((420.0, 0.0), (900.0, 0.0)).with_stops([
                (0.0, colors::CRIMSON),
                (0.5, colors::GOLD),
                (1.0, colors::SKY_BLUE),
            ]);
            let big = Rect::from_center_size((690.0, 178.0), (128.0, 128.0));
            c.fill_rounded_rect(big.inflate(12.0, 12.0), 28.0, colors::GHOST_WHITE);
            c.draw_icon(&home, big, &gradient);

            c.fill_text(
                &font,
                "from_svg_file keeps viewBox, paths, transforms and stroke outlines",
                (48.0, 286.0),
                18.0,
                colors::DIM_GRAY,
            );

            // A full-SVG icon with only stroke geometry: strokes are expanded
            // to outlines by VectorIcon, so draw_icon can fill it with any paint.
            let t = frame.time.as_secs_f64();
            let center = (180.0, 400.0);
            c.with_transform(Affine::rotate_about(t, center), |c| {
                c.draw_icon(
                    &spark,
                    Rect::from_center_size(center, (120.0, 120.0)),
                    colors::CRIMSON,
                );
            });
            c.draw_icon(
                &ring,
                Rect::from_center_size((360.0, 400.0), (120.0, 120.0)),
                colors::BLACK,
            );

            c.fill_text(
                &font,
                "rotating stroke icon",
                (120.0, 482.0),
                16.0,
                colors::DIM_GRAY,
            );

            c.fill_text(
                &font,
                "Bitmap icons (alpha channel only)",
                (48.0, 536.0),
                18.0,
                colors::DIM_GRAY,
            );

            for (index, size) in [36.0_f64, 52.0, 72.0, 96.0].into_iter().enumerate() {
                let x = 120.0 + index as f64 * 100.0;
                let rect = Rect::from_center_size((x, 586.0), (size, size));
                let color = [
                    colors::GOLD,
                    colors::CRIMSON,
                    colors::SKY_BLUE,
                    colors::BLACK,
                ][index];
                c.draw_bitmap_icon(&star, rect, color);
            }

            for (index, color) in [colors::BLACK, colors::CRIMSON, colors::GOLD]
                .into_iter()
                .enumerate()
            {
                let x = 620.0 + index as f64 * 110.0;
                let rect = Rect::from_center_size((x, 586.0), (78.0, 78.0));
                c.draw_bitmap_icon(&ring_image, rect, color);
            }

            c.reset_transform();
        })
        .run()
}

fn fit_view(c: &Canvas) -> Affine {
    let scale = (c.width() / VIEW_WIDTH).min(c.height() / VIEW_HEIGHT);
    let offset = (
        (c.width() - VIEW_WIDTH * scale) / 2.0,
        (c.height() - VIEW_HEIGHT * scale) / 2.0,
    );
    Affine::translate(offset) * Affine::scale(scale)
}
