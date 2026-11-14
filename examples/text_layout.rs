//! A complete text-layout demo: shaping, fallback, wrapping, alignment, rich
//! text spans, and metrics.

use plasticism_draw::prelude::*;

const BODY: &str = "Parley shapes text with kerning, ligatures, variable axes, and automatic font fallback. \
这段中文会自动回退到系统字体，并在窄宽度下按 Unicode 规则折行。";
const RICH: &str = "Rich text can mix weights, italics, sizes, families, and colors in one layout.\n\
这是同一个 TextLayout 里的第二段。";

fn main() -> anyhow::Result<()> {
    let font = Font::from_bytes(include_bytes!("../assets/Inter-Regular.ttf"))?;

    let headline_style = TextStyle::new(88.0)
        .weight(FontWeight::BLACK)
        .letter_spacing(-2.0)
        .align(TextAlign::Center);
    let body_style = TextStyle::new(18.0)
        .line_height_factor(1.55)
        .wrap_width(500.0);
    let rich_base = TextStyle::new(18.0)
        .line_height_factor(1.55)
        .wrap_width(520.0);

    let mut rich = RichText::new(rich_base.clone());
    if let Some(start) = RICH.find("italics") {
        rich = rich
            .span_with(start..start + "italics".len(), |style| {
                style.font_style(FontStyle::Italic)
            })
            .color(start..start + "Rich text".len(), colors::CRIMSON);
    }
    if let Some(start) = RICH.find("weights") {
        rich = rich
            .span_with(start..start + "weights".len(), |style| {
                style.weight(FontWeight::BOLD)
            })
            .color(start..start + "weights".len(), colors::ROYAL_BLUE);
    }
    if let Some(start) = RICH.find("colors") {
        rich = rich
            .span_with(start..start + "colors".len(), |style| {
                style.size(26.0).letter_spacing(1.0)
            })
            .color(start..start + "colors".len(), rgb(0xB8, 0x7A, 0x1E));
    }
    if let Some(start) = RICH.find("第二段") {
        rich = rich
            .span_with(start..start + "第二段".len(), |style| {
                style.weight(FontWeight::SEMI_BOLD)
            })
            .color(start..start + "第二段".len(), rgb(0x2E, 0x7D, 0x32));
    }

    let headline = font.layout("PARLEY TEXT", &headline_style);
    let body = font.layout(BODY, &body_style);
    let rich_layout = font.layout_rich(RICH, &rich);

    let font_metrics = font.metrics(&body_style);
    let body_metrics = body.metrics();
    let rich_metrics = rich_layout.metrics();
    let body_lines = body.line_metrics().to_vec();
    let body_runs = body.run_metrics().len();
    let family = font.family_name().to_owned();
    let status_font = font.clone();

    App::new("plasticism · text layout", 1200, 800)
        .background(rgb(0xF4, 0xF5, 0xF7))
        .draw(move |canvas, _frame| {
            let width = canvas.width();

            canvas.fill_text(
                &status_font,
                "TEXT / PARLEY",
                (48.0, 46.0),
                12.0,
                colors::DIM_GRAY,
            );
            canvas.fill_text(
                &status_font,
                "shaping · fallback · wrapping · rich text · metrics",
                (48.0, 68.0),
                12.0,
                colors::LIGHT_SLATE_GRAY,
            );

            canvas.fill_text_layout(&headline, (width / 2.0, 172.0), colors::BLACK);
            canvas.line(
                (48.0, 198.0),
                (width - 48.0, 198.0),
                Stroke::new(1.0, rgb(0xD8, 0xDC, 0xE3)),
            );

            canvas.fill_text(
                &status_font,
                "SINGLE STYLE / WRAP + FALLBACK",
                (48.0, 238.0),
                11.0,
                colors::DIM_GRAY,
            );
            canvas.fill_text_layout(&body, (48.0, 268.0), colors::BLACK);

            canvas.fill_text(
                &status_font,
                "RICH TEXT / SPANS + COLORS",
                (620.0, 238.0),
                11.0,
                colors::DIM_GRAY,
            );
            canvas.fill_text_layout(&rich_layout, (620.0, 268.0), colors::BLACK);

            let card = Rect::new(48.0, 470.0, width - 48.0, 706.0);
            canvas.fill_rounded_rect(card, 18.0, colors::WHITE);
            canvas.stroke_rounded_rect(
                card,
                18.0,
                Stroke::new(1.0, rgb(0xD8, 0xDC, 0xE3)),
            );

            canvas.fill_text(
                &status_font,
                "METRICS",
                (76.0, 504.0),
                11.0,
                colors::DIM_GRAY,
            );

            let font_text = format!(
                "font      ascent {:.1}   descent {:.1}   gap {:.1}   line {:.1}\n\
                 cap {:.1}   x-height {:.1}   underline {:.1} / {:.1}\n\
                 primary family: {}",
                font_metrics.ascent,
                font_metrics.descent,
                font_metrics.line_gap,
                font_metrics.line_height,
                font_metrics.cap_height.unwrap_or(0.0),
                font_metrics.x_height.unwrap_or(0.0),
                font_metrics.underline_offset.unwrap_or(0.0),
                font_metrics.underline_size.unwrap_or(0.0),
                family,
            );
            canvas.fill_text(
                &status_font,
                &font_text,
                (76.0, 536.0),
                13.0,
                colors::SLATE_GRAY,
            );

            let layout_text = format!(
                "body      {:.0} × {:.0}   lines {}   runs {}\n\
                 rich      {:.0} × {:.0}   lines {}   rtl {}\n\
                 line widths: {}",
                body_metrics.width,
                body_metrics.height,
                body_metrics.line_count,
                body_runs,
                rich_metrics.width,
                rich_metrics.height,
                rich_metrics.line_count,
                rich_layout.is_rtl(),
                body_lines
                    .iter()
                    .take(4)
                    .enumerate()
                    .map(|(index, line)| format!("L{} {:.0}", index + 1, line.width))
                    .collect::<Vec<_>>()
                    .join("   "),
            );
            canvas.fill_text(
                &status_font,
                &layout_text,
                (620.0, 536.0),
                13.0,
                colors::SLATE_GRAY,
            );

            canvas.fill_text(
                &status_font,
                "The paragraph above mixes font fallback, kerning, ligatures and Unicode line breaking. \
The right column is one TextLayout with per-span styles and colors.",
                (76.0, 650.0),
                12.5,
                colors::DIM_GRAY,
            );

            canvas.fill_text(
                &status_font,
                "text layout is cached by text + style; drawing reuses the same glyph runs",
                (48.0, 758.0),
                11.5,
                colors::LIGHT_SLATE_GRAY,
            );
        })
        .run()
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}
