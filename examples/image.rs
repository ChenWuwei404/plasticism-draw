use plasticism_draw::prelude::*;

const VIEW_WIDTH: f64 = 960.0;
const VIEW_HEIGHT: f64 = 720.0;

fn main() -> anyhow::Result<()> {
    let photo = Image::open(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/photo.png"))?;
    let star = Image::open(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/star.png"))?;
    let ring = Image::open(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/ring.png"))?;
    let font = Font::from_bytes(include_bytes!("../assets/Inter-Regular.ttf"))?;

    App::new("image", 900, 675)
        .background(colors::WHITE)
        .draw(move |c, frame| {
            let view = fit_view(c);
            c.set_transform(view);

            c.fill_text(
                &font,
                "Bitmap images · scaling · fit · alpha icons",
                (48.0, 54.0),
                30.0,
                colors::SLATE_GRAY,
            );

            let card = Rect::from_origin_size((48.0, 92.0), (274.0, 190.0));
            let gap = 20.0;
            let contain =
                Rect::from_origin_size((card.x1 + gap, card.y0), (card.width(), card.height()));
            let cover =
                Rect::from_origin_size((contain.x1 + gap, card.y0), (card.width(), card.height()));

            card_background(c, card);
            c.draw_image(&photo, card);
            card_label(c, card, &font, "draw_image · stretch");

            card_background(c, contain);
            c.draw_image_fit(&photo, contain, ImageFit::Contain);
            card_label(c, contain, &font, "Contain");

            card_background(c, cover);
            c.with_clip(RoundedRect::from_rect(cover, 22.0), |c| {
                c.draw_image_fit(&photo, cover, ImageFit::Cover);
            });
            c.stroke_rounded_rect(cover, 22.0, Stroke::new(2.0, colors::BLACK));
            card_label(c, cover, &font, "Cover · rounded clip");

            // Scaling is just the destination rectangle: animate it freely.
            let t = frame.time.as_secs_f64();
            let scale = 0.92 + (t * 1.6).sin() * 0.12;
            let animated =
                Rect::from_center_size((VIEW_WIDTH / 2.0, 430.0), (520.0 * scale, 260.0 * scale));
            c.draw_image_fit(&photo, animated, ImageFit::Contain);
            c.stroke_rounded_rect(animated, 18.0, Stroke::new(2.0, colors::CRIMSON));

            c.fill_text(
                &font,
                "same bitmap, animated destination rect",
                (48.0, 590.0),
                18.0,
                colors::DIM_GRAY,
            );

            // Bitmap icons use the alpha channel only; the paint is a tint.
            c.fill_text(
                &font,
                "alpha-mask icons",
                (48.0, 640.0),
                18.0,
                colors::DIM_GRAY,
            );
            for (index, size) in [32.0_f64, 48.0, 64.0, 88.0].into_iter().enumerate() {
                let x = 230.0 + index as f64 * 100.0;
                let rect = Rect::from_center_size((x, 666.0), (size, size));
                let color = [
                    colors::GOLD,
                    colors::CRIMSON,
                    colors::SKY_BLUE,
                    colors::BLACK,
                ][index];
                c.draw_bitmap_icon(&star, rect, color);
            }

            let ring_rect = Rect::from_center_size((650.0, 666.0), (72.0, 72.0));
            c.draw_bitmap_icon(&ring, ring_rect, colors::DARK_ORCHID);

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

fn card_background(c: &mut Canvas, rect: Rect) {
    c.fill_rect(rect, colors::GHOST_WHITE);
}

fn card_label(c: &mut Canvas, rect: Rect, font: &Font, label: &str) {
    // A small translucent plate keeps the text readable over the image.
    let plate = Rect::from_origin_size((rect.x0, rect.y0), (rect.width(), 32.0));
    c.with_opacity(0.82, |c| c.fill_rect(plate, colors::WHITE));
    c.fill_text(
        font,
        label,
        (rect.x0 + 12.0, rect.y0 + 22.0),
        15.0,
        colors::BLACK,
    );
}
