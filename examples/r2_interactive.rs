//! Interactive R2 clipping demo.
//!
//! Two stacked R2 rounded rectangles clip animated content. The mouse wheel
//! changes their corner radius, and the button beside them switches the first
//! card between a grey fill and a black outline.
//!
//! All layout and hit testing happens in canvas units. Winit reports pointer
//! positions in physical pixels, so they are divided by the same effective
//! scale factor used by the renderer. This keeps clicks accurate at 125%,
//! 150%, 200%, and when the window moves to another monitor.

use plasticism_draw::prelude::*;
use plasticism_draw::winit::dpi::PhysicalPosition;
use plasticism_draw::winit::event::{MouseButton, MouseScrollDelta};
use plasticism_draw::winit::window::CursorIcon;

/// Use one DPI mode for both the renderer and event-coordinate conversion.
const DPI_MODE: DpiMode = DpiMode::Auto;

const MIN_RADIUS: f64 = 6.0;
const MAX_RADIUS: f64 = 120.0;
const WHEEL_STEP: f64 = 5.0;

const MARGIN: f64 = 56.0;
const HEADER_BOTTOM: f64 = 112.0;
const FOOTER_SPACE: f64 = 64.0;
const CARD_GAP: f64 = 24.0;
const COLUMN_GAP: f64 = 40.0;
const BUTTON_WIDTH: f64 = 230.0;

fn main() -> anyhow::Result<()> {
    let font = Font::from_bytes(include_bytes!("../assets/Inter-Regular.ttf"))?;

    App::new("plasticism · interactive R2 clip", 960, 660)
        .background(rgb(0xF3, 0xF4, 0xF6))
        .dpi_mode(DPI_MODE)
        .state(Demo::default())
        .on_event(|state, event, event_loop, window| {
            let layout = Layout::new(canvas_size(window));

            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::ScaleFactorChanged { .. } => {
                    // The pointer cache stores canvas coordinates, which are
                    // stale after a DPI change. Wait for the next CursorMoved.
                    state.pointer = None;
                    state.button_hover = false;
                    window.set_cursor(CursorIcon::Default);
                }
                WindowEvent::CursorMoved { position, .. } => {
                    let point = canvas_point(*position, window);
                    state.pointer = Some(point);

                    let hover = layout.button.contains(point);
                    if hover != state.button_hover {
                        state.button_hover = hover;
                        window.set_cursor(if hover {
                            CursorIcon::Pointer
                        } else {
                            CursorIcon::Default
                        });
                    }
                }
                WindowEvent::CursorLeft { .. } => {
                    state.pointer = None;
                    state.button_hover = false;
                    window.set_cursor(CursorIcon::Default);
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    ..
                } => {
                    if state
                        .pointer
                        .is_some_and(|pointer| layout.button.contains(pointer))
                    {
                        state.style = state.style.toggled();
                    }
                }
                WindowEvent::MouseWheel { delta, .. } => {
                    let steps = match delta {
                        MouseScrollDelta::LineDelta(_, y) => f64::from(*y),
                        // Pixel deltas are physical pixels, so convert them to
                        // canvas units before using them in a radius change.
                        MouseScrollDelta::PixelDelta(position) => {
                            position.y / (60.0 * effective_scale(window))
                        }
                    };

                    if steps != 0.0 {
                        state.radius =
                            (state.radius + steps * WHEEL_STEP).clamp(MIN_RADIUS, MAX_RADIUS);
                    }
                }
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed =>
                {
                    if matches!(event.logical_key, Key::Named(NamedKey::Escape)) {
                        event_loop.exit();
                    }
                }
                _ => {}
            }
        })
        .on_draw(move |state, canvas, frame| {
            state.phase += frame.delta.as_secs_f64();
            draw(state, canvas, &font);
        })
        .run()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CardStyle {
    #[default]
    GrayFill,
    BlackStroke,
}

impl CardStyle {
    fn toggled(self) -> Self {
        match self {
            Self::GrayFill => Self::BlackStroke,
            Self::BlackStroke => Self::GrayFill,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::GrayFill => "GRAY FILL",
            Self::BlackStroke => "BLACK STROKE",
        }
    }
}

struct Demo {
    radius: f64,
    style: CardStyle,
    phase: f64,
    pointer: Option<Point>,
    button_hover: bool,
}

impl Default for Demo {
    fn default() -> Self {
        Self {
            radius: 40.0,
            style: CardStyle::GrayFill,
            phase: 0.0,
            pointer: None,
            button_hover: false,
        }
    }
}

#[derive(Clone, Copy)]
struct Layout {
    top: Rect,
    bottom: Rect,
    button: Rect,
}

impl Layout {
    fn new(size: Size) -> Self {
        // The right column keeps the button at a usable canvas-unit width. At
        // high DPI this layout still works because size is in canvas units, not
        // physical pixels.
        let card_right = (size.width - MARGIN - COLUMN_GAP - BUTTON_WIDTH).max(MARGIN + 280.0);
        let card_bottom = (size.height - FOOTER_SPACE).max(HEADER_BOTTOM + 280.0);
        let card_height = ((card_bottom - HEADER_BOTTOM - CARD_GAP) / 2.0).max(130.0);

        let top = Rect::new(
            MARGIN,
            HEADER_BOTTOM,
            card_right,
            HEADER_BOTTOM + card_height,
        );
        let bottom = Rect::new(
            MARGIN,
            top.y1 + CARD_GAP,
            card_right,
            top.y1 + CARD_GAP + card_height,
        );

        let button_x0 = top.x1 + COLUMN_GAP;
        let button_x1 = (size.width - MARGIN).max(button_x0 + 160.0);
        let button = Rect::new(
            button_x0,
            top.center().y - 36.0,
            button_x1,
            top.center().y + 36.0,
        );

        Self {
            top,
            bottom,
            button,
        }
    }
}

fn draw(state: &Demo, canvas: &mut Canvas, font: &Font) {
    let layout = Layout::new(canvas.size());
    let top = R2RoundedRect::from_rect(layout.top, state.radius);
    let radius = top.radius();
    let phase = state.phase;

    draw_header(canvas, font);

    draw_top_card(canvas, font, layout.top, state.style, radius, phase);
    draw_bottom_card(canvas, font, layout.bottom, radius, phase);
    draw_sidebar(
        canvas,
        font,
        &layout,
        state.style,
        state.button_hover,
        radius,
        canvas.scale_factor(),
    );
    draw_footer(canvas, font);
}

fn draw_header(canvas: &mut Canvas, font: &Font) {
    canvas.fill_text(font, "R2 / LIVE CLIP", (MARGIN, 58.0), 32.0, colors::BLACK);
    canvas.fill_text(
        font,
        "Two R2 rounded rectangles clipping live motion. Scroll to reshape, click to restyle.",
        (MARGIN, 84.0),
        15.0,
        colors::DIM_GRAY,
    );

    // Visible proof that the canvas is in scaled units while the window itself
    // is backed by more physical pixels on a high-DPI display.
    let dpi = format!(
        "DPI ×{:.2}   canvas {:.0} × {:.0}",
        canvas.scale_factor(),
        canvas.width(),
        canvas.height()
    );
    let width = font.measure(&dpi, 13.0).width;
    canvas.fill_text(
        font,
        &dpi,
        (canvas.width() - MARGIN - width, 58.0),
        13.0,
        colors::DIM_GRAY,
    );
}

fn draw_top_card(
    canvas: &mut Canvas,
    font: &Font,
    rect: Rect,
    style: CardStyle,
    radius: f64,
    phase: f64,
) {
    let shape = R2RoundedRect::from_rect(rect, radius);
    let path = shape.to_path(0.1);
    let center = rect.center();

    canvas.with_clip(&path, |canvas| {
        if style == CardStyle::GrayFill {
            canvas.fill_rect(rect, rgb(0xBF, 0xC5, 0xCD));
        }

        // Rotated light bands run past every edge of the card.
        let band_paint = match style {
            CardStyle::GrayFill => Paint::solid(colors::WHITE).with_alpha(0.32),
            CardStyle::BlackStroke => Paint::solid(colors::BLACK).with_alpha(0.07),
        };
        canvas.with_transform(Affine::rotate_about(-0.19, center), |canvas| {
            let span = rect.width() + rect.height() + 320.0;
            for i in 0..10 {
                let x = rect.x0 - 180.0 + (phase * 74.0 + f64::from(i) * 112.0).rem_euclid(span);
                canvas.fill_rect(
                    Rect::new(x, rect.y0 - 180.0, x + 58.0, rect.y1 + 180.0),
                    band_paint.clone(),
                );
            }
        });

        // A large moving disc crosses the rounded clip boundary.
        let disc = (
            center.x + (phase * 0.85).sin() * (rect.width() * 0.34),
            center.y + (phase * 1.15).cos() * (rect.height() * 0.30),
        );
        match style {
            CardStyle::GrayFill => {
                canvas.fill_circle(disc, 92.0, Paint::solid(colors::WHITE).with_alpha(0.52));
                canvas.stroke_circle(
                    disc,
                    124.0,
                    Stroke::new(1.5, Paint::solid(colors::BLACK).with_alpha(0.16)),
                );
            }
            CardStyle::BlackStroke => {
                canvas.fill_circle(
                    disc,
                    92.0,
                    Paint::solid(rgb(0xE3, 0xA8, 0x4C)).with_alpha(0.72),
                );
                canvas.stroke_circle(
                    disc,
                    124.0,
                    Stroke::new(1.5, Paint::solid(colors::BLACK).with_alpha(0.25)),
                );
            }
        }

        // Repeating large text. One copy leaves while the next enters, so the
        // rounded mask is always visibly cutting the word.
        let text = "R2 CLIP / MOTION / ";
        let text_size = 96.0;
        let text_width = font.measure(text, text_size).width;
        let travel = text_width + 80.0;
        let start = rect.center().x - text_width * 0.5 - (phase * 62.0).rem_euclid(travel);
        let copies = ((rect.width() / travel).ceil() as usize) + 2;
        for i in 0..copies {
            canvas.fill_text(
                font,
                text,
                (start + f64::from(i as u32) * travel, center.y + 34.0),
                text_size,
                Paint::solid(colors::BLACK).with_alpha(0.82),
            );
        }

        draw_centered_label(
            canvas,
            font,
            &format!("01 / STYLE PREVIEW / {}", style.label()),
            rect.center().x,
            rect.y0 + 38.0,
            12.0,
            Paint::solid(colors::BLACK).with_alpha(0.58),
        );
    });

    // The outline belongs outside the clip so its full width stays visible.
    if style == CardStyle::BlackStroke {
        canvas.stroke_path(&path, Stroke::new(4.0, colors::BLACK));
    }
}

fn draw_bottom_card(canvas: &mut Canvas, font: &Font, rect: Rect, radius: f64, phase: f64) {
    let shape = R2RoundedRect::from_rect(rect, radius);
    let path = shape.to_path(0.1);
    let center = rect.center();

    canvas.with_clip(&path, |canvas| {
        let gradient = Gradient::new_linear((rect.x0, rect.y0), (rect.x1, rect.y1)).with_stops([
            (0.0, rgb(0x16, 0x1B, 0x2E)),
            (0.55, rgb(0x25, 0x2F, 0x57)),
            (1.0, rgb(0x0C, 0x10, 0x18)),
        ]);
        canvas.fill_rect(rect, &gradient);

        // Slow diagonal beams.
        canvas.with_transform(Affine::rotate_about(0.16, center), |canvas| {
            let span = rect.width() + rect.height() + 420.0;
            for i in 0..11 {
                let x = rect.x0 - 260.0 + (phase * 112.0 + f64::from(i) * 106.0).rem_euclid(span);
                let color = if i % 2 == 0 {
                    Paint::solid(colors::WHITE).with_alpha(0.055)
                } else {
                    Paint::solid(rgb(0xE3, 0xA8, 0x4C)).with_alpha(0.075)
                };
                canvas.fill_rect(
                    Rect::new(x, rect.y0 - 220.0, x + 54.0, rect.y1 + 220.0),
                    color,
                );
            }
        });

        // Concentric rings drift across the clip edge.
        let ring_center = (
            center.x + (phase * 0.48).sin() * (rect.width() * 0.38),
            center.y + (phase * 0.72).cos() * (rect.height() * 0.22),
        );
        for i in 0..4 {
            canvas.stroke_circle(
                ring_center,
                72.0 + f64::from(i as u32) * 54.0,
                Stroke::new(
                    1.2,
                    Paint::solid(colors::LIGHT_CYAN).with_alpha(0.16 - i as f32 * 0.025),
                ),
            );
        }
        canvas.fill_circle(
            ring_center,
            13.0,
            Paint::solid(rgb(0xE3, 0xA8, 0x4C)).with_alpha(0.95),
        );

        // A second marquee moves in the opposite direction.
        let text = "STAY INSIDE / ";
        let text_size = 58.0;
        let text_width = font.measure(text, text_size).width;
        let travel = text_width + 54.0;
        let start = rect.x1 + 30.0 - (phase * 46.0).rem_euclid(travel);
        let copies = ((rect.width() / travel).ceil() as usize) + 2;
        for i in 0..copies {
            canvas.fill_text(
                font,
                text,
                (start - f64::from(i as u32) * travel, center.y + 21.0),
                text_size,
                Paint::solid(colors::WHITE).with_alpha(0.90),
            );
        }

        draw_centered_label(
            canvas,
            font,
            "02 / LIVE CONTENT / CLIPPED BY R2",
            center.x,
            rect.y0 + 38.0,
            12.0,
            Paint::solid(colors::WHITE).with_alpha(0.62),
        );
    });

    // A faint rim makes the mask legible where the gradient approaches white.
    canvas.stroke_path(
        &path,
        Stroke::new(1.5, Paint::solid(colors::BLACK).with_alpha(0.16)),
    );
}

fn draw_sidebar(
    canvas: &mut Canvas,
    font: &Font,
    layout: &Layout,
    style: CardStyle,
    hover: bool,
    radius: f64,
    scale: f64,
) {
    let button = layout.button;
    let x = button.x0;

    canvas.fill_text(
        font,
        "CARD 01 STYLE",
        (x, button.y0 - 24.0),
        11.0,
        colors::DIM_GRAY,
    );

    // A soft offset shadow keeps the button visually separate from the cards.
    let shadow = R2RoundedRect::from_rect(
        Rect::new(button.x0, button.y0 + 6.0, button.x1, button.y1 + 6.0),
        18.0,
    );
    canvas.fill(
        shadow,
        Paint::solid(colors::BLACK).with_alpha(if hover { 0.16 } else { 0.09 }),
    );

    let button_color = if hover {
        rgb(0x25, 0x2C, 0x36)
    } else {
        rgb(0x10, 0x13, 0x18)
    };
    canvas.fill(
        R2RoundedRect::from_rect(button, 18.0),
        Paint::solid(button_color),
    );

    // Small style swatch inside a white tile.
    let icon_tile = Rect::from_center_size((button.x0 + 38.0, button.center().y), (40.0, 40.0));
    canvas.fill(
        R2RoundedRect::from_rect(icon_tile, 10.0),
        Paint::solid(colors::WHITE).with_alpha(0.96),
    );
    let icon = Rect::from_center_size(icon_tile.center(), (20.0, 20.0));
    match style {
        CardStyle::GrayFill => canvas.fill(
            R2RoundedRect::from_rect(icon, 5.0),
            Paint::solid(rgb(0xA9, 0xB0, 0xBA)),
        ),
        CardStyle::BlackStroke => canvas.stroke(
            R2RoundedRect::from_rect(icon, 5.0),
            Stroke::new(2.0, colors::BLACK),
        ),
    }

    let text_x = button.x0 + 68.0;
    canvas.fill_text(
        font,
        style.label(),
        (text_x, button.center().y - 3.0),
        15.0,
        colors::WHITE,
    );
    canvas.fill_text(
        font,
        "click to switch",
        (text_x, button.center().y + 17.0),
        11.0,
        Paint::solid(colors::WHITE).with_alpha(0.55),
    );

    // Radius readout.
    let label_y = button.y1 + 46.0;
    canvas.fill_text(font, "CORNER RADIUS", (x, label_y), 11.0, colors::DIM_GRAY);

    let radius_text = format!("{radius:.0}");
    let value_y = label_y + 56.0;
    canvas.fill_text(font, &radius_text, (x, value_y), 46.0, colors::BLACK);
    let value_width = font.measure(&radius_text, 46.0).width;
    canvas.fill_text(
        font,
        "R",
        (x + value_width + 7.0, value_y - 15.0),
        15.0,
        colors::DIM_GRAY,
    );

    // A simple range indicator for the wheel value.
    let track_y = value_y + 26.0;
    let track = Rect::new(x, track_y, button.x1, track_y + 4.0);
    canvas.fill_rounded_rect(track, 2.0, rgb(0xD6, 0xDA, 0xE0));
    let fraction = ((radius - MIN_RADIUS) / (MAX_RADIUS - MIN_RADIUS)).clamp(0.0, 1.0);
    let filled = Rect::new(x, track_y, x + button.width() * fraction, track_y + 4.0);
    canvas.fill_rounded_rect(filled, 2.0, rgb(0x10, 0x13, 0x18));
    let knob = (x + button.width() * fraction, track_y + 2.0);
    canvas.fill_circle(knob, 7.0, colors::WHITE);
    canvas.stroke_circle(knob, 7.0, Stroke::new(2.0, rgb(0x10, 0x13, 0x18)));

    canvas.fill_text(
        font,
        "SCROLL UP / DOWN",
        (x, track_y + 28.0),
        10.5,
        colors::DIM_GRAY,
    );

    // DPI note under the controls.
    let note_y = track_y + 66.0;
    canvas.fill_text(
        font,
        "Hit testing divides winit's physical\npointer coordinates by the canvas scale.",
        (x, note_y),
        13.0,
        colors::DIM_GRAY,
    );
    let scale_text = format!("effective scale  ×{scale:.2}");
    canvas.fill_text(font, &scale_text, (x, note_y + 37.0), 12.0, colors::BLACK);
}

fn draw_footer(canvas: &mut Canvas, font: &Font) {
    let hint = "WHEEL   adjust both radii        CLICK   toggle card 01 style        ESC   quit";
    canvas.fill_text(
        font,
        hint,
        (MARGIN, canvas.height() - 26.0),
        12.5,
        colors::DIM_GRAY,
    );
}

fn draw_centered_label(
    canvas: &mut Canvas,
    font: &Font,
    text: &str,
    center_x: f64,
    baseline: f64,
    size: f64,
    paint: impl Into<Paint>,
) {
    let width = font.measure(text, size).width;
    canvas.fill_text(font, text, (center_x - width / 2.0, baseline), size, paint);
}

/// The canvas size in canvas units, derived from the window's physical size.
fn canvas_size(window: &Window) -> Size {
    let physical = window.inner_size();
    let scale = effective_scale(window);
    Size::new(
        f64::from(physical.width.max(1)) / scale,
        f64::from(physical.height.max(1)) / scale,
    )
}

/// Converts a winit physical pointer position into canvas units.
fn canvas_point(position: PhysicalPosition<f64>, window: &Window) -> Point {
    let scale = effective_scale(window);
    Point::new(position.x / scale, position.y / scale)
}

fn effective_scale(window: &Window) -> f64 {
    DPI_MODE.effective_scale(window.scale_factor())
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}
