//! A frameless window with a custom title bar.
//!
//! [`App::frameless`] removes the system title bar while keeping the native
//! top-level window: Windows still plays the system minimize, maximize,
//! restore, and snap animations, and the window keeps its drop shadow. The
//! library does not draw a title bar, so this example implements one:
//!
//! - drag the title bar to move the window
//! - double-click the title bar to maximize or restore
//! - drag the window edges to resize
//! - use the minimize, maximize/restore, and close buttons
//!
//! Winit reports pointer positions in physical pixels, so they are divided by
//! the same effective scale factor used by the renderer before hit testing.

use std::time::{Duration, Instant};

use plasticism_draw::prelude::*;
use plasticism_draw::winit::dpi::PhysicalPosition;
use plasticism_draw::winit::event::MouseButton;
use plasticism_draw::winit::window::{CursorIcon, ResizeDirection};

const DPI_MODE: DpiMode = DpiMode::Auto;

const TITLE_BAR_HEIGHT: f64 = 46.0;
const BUTTON_WIDTH: f64 = 46.0;
const RESIZE_BORDER: f64 = 7.0;
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
const DOUBLE_CLICK_DISTANCE: f64 = 4.0;

const CONTENT_MARGIN: f64 = 22.0;
const FOOTER_HEIGHT: f64 = 58.0;

fn main() -> anyhow::Result<()> {
    let font = Font::from_bytes(include_bytes!("../assets/Inter-Regular.ttf"))?;

    App::new("plasticism · frameless", 960, 640)
        // Show no system title bar, but keep the native top-level window and
        // its DWM animations and shadow.
        .frameless(true)
        .background(rgb(0xF4, 0xF5, 0xF7))
        .dpi_mode(DPI_MODE)
        .state(Demo::default())
        .on_event(|state, event, event_loop, window| {
            state.maximized = window.is_maximized();
            let layout = Layout::new(canvas_size(window));

            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::CursorMoved { position, .. } => {
                    let point = canvas_point(*position, window);
                    state.pointer = Some(point);
                    state.hovered = hovered_action(&layout, point, state.maximized);
                    update_cursor(state, window, &layout);
                }
                WindowEvent::CursorLeft { .. } => {
                    state.pointer = None;
                    state.hovered = None;
                    state.cursor = Some(CursorIcon::Default);
                    window.set_cursor(CursorIcon::Default);
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    ..
                } => {
                    let Some(point) = state.pointer else {
                        return;
                    };

                    // The resize border sits on top of the title bar and its
                    // buttons, matching the native hit-testing order.
                    if let Some(direction) = layout.resize_direction(point, state.maximized) {
                        if let Err(error) = window.drag_resize_window(direction) {
                            eprintln!("failed to start window resize: {error}");
                        }
                        return;
                    }

                    if let Some(action) = layout.action_at(point) {
                        state.hovered = None;
                        match action {
                            Action::Minimize => window.set_minimized(true),
                            Action::Maximize => window.set_maximized(!state.maximized),
                            Action::Close => event_loop.exit(),
                        }
                        return;
                    }

                    if layout.title_bar().contains(point) {
                        let now = Instant::now();
                        let double_click = state.last_title_click.is_some_and(|(then, at)| {
                            now.duration_since(then) <= DOUBLE_CLICK
                                && (at.x - point.x).hypot(at.y - point.y) <= DOUBLE_CLICK_DISTANCE
                        });

                        if double_click {
                            state.last_title_click = None;
                            window.set_maximized(!state.maximized);
                        } else {
                            state.last_title_click = Some((now, point));
                            if let Err(error) = window.drag_window() {
                                eprintln!("failed to start window drag: {error}");
                            }
                        }
                    }
                }
                WindowEvent::ScaleFactorChanged { .. } => {
                    // Cached canvas coordinates are stale after a DPI change.
                    state.pointer = None;
                    state.hovered = None;
                    state.last_title_click = None;
                    state.cursor = Some(CursorIcon::Default);
                    window.set_cursor(CursorIcon::Default);
                }
                _ => {}
            }
        })
        .on_draw(move |state, canvas, frame, _window| {
            state.phase += frame.delta.as_secs_f64();
            draw(state, canvas, &font);
        })
        .run()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Minimize,
    Maximize,
    Close,
}

#[derive(Default)]
struct Demo {
    pointer: Option<Point>,
    hovered: Option<Action>,
    last_title_click: Option<(Instant, Point)>,
    cursor: Option<CursorIcon>,
    maximized: bool,
    phase: f64,
}

#[derive(Clone, Copy)]
struct Layout {
    width: f64,
    height: f64,
}

impl Layout {
    fn new(size: Size) -> Self {
        Self {
            width: size.width.max(1.0),
            height: size.height.max(1.0),
        }
    }

    fn title_bar(&self) -> Rect {
        Rect::new(0.0, 0.0, self.width, TITLE_BAR_HEIGHT.min(self.height))
    }

    fn button_rect(&self, action: Action) -> Rect {
        let from_right = match action {
            Action::Close => 0.0,
            Action::Maximize => 1.0,
            Action::Minimize => 2.0,
        };
        let x1 = (self.width - from_right * BUTTON_WIDTH).max(0.0);
        let x0 = (x1 - BUTTON_WIDTH).max(0.0);
        Rect::new(x0, 0.0, x1, self.title_bar().y1)
    }

    fn action_at(&self, point: Point) -> Option<Action> {
        if !self.title_bar().contains(point) {
            return None;
        }
        [Action::Close, Action::Maximize, Action::Minimize]
            .into_iter()
            .find(|action| self.button_rect(*action).contains(point))
    }

    /// Returns the resize direction for a point on the window border.
    ///
    /// Resizing is disabled while the window is maximized, where the borders
    /// sit against the monitor edge.
    fn resize_direction(&self, point: Point, maximized: bool) -> Option<ResizeDirection> {
        if maximized {
            return None;
        }

        let left = point.x <= RESIZE_BORDER;
        let right = point.x >= self.width - RESIZE_BORDER;
        let top = point.y <= RESIZE_BORDER;
        let bottom = point.y >= self.height - RESIZE_BORDER;

        match (left, right, top, bottom) {
            (true, _, true, _) => Some(ResizeDirection::NorthWest),
            (_, true, true, _) => Some(ResizeDirection::NorthEast),
            (true, _, _, true) => Some(ResizeDirection::SouthWest),
            (_, true, _, true) => Some(ResizeDirection::SouthEast),
            (true, _, _, _) => Some(ResizeDirection::West),
            (_, true, _, _) => Some(ResizeDirection::East),
            (_, _, true, _) => Some(ResizeDirection::North),
            (_, _, _, true) => Some(ResizeDirection::South),
            _ => None,
        }
    }
}

fn hovered_action(layout: &Layout, point: Point, maximized: bool) -> Option<Action> {
    if layout.resize_direction(point, maximized).is_some() {
        None
    } else {
        layout.action_at(point)
    }
}

fn update_cursor(state: &mut Demo, window: &Window, layout: &Layout) {
    let Some(point) = state.pointer else {
        return;
    };

    let cursor = layout
        .resize_direction(point, state.maximized)
        .map_or(CursorIcon::Default, CursorIcon::from);

    if state.cursor != Some(cursor) {
        state.cursor = Some(cursor);
        window.set_cursor(cursor);
    }
}

fn draw(state: &Demo, canvas: &mut Canvas, font: &Font) {
    draw_content(state, canvas, font);
    draw_footer(canvas, font);
    draw_title_bar(state, canvas, font);
}

fn draw_content(state: &Demo, canvas: &mut Canvas, font: &Font) {
    let left = CONTENT_MARGIN;
    let right = (canvas.width() - CONTENT_MARGIN).max(left + 240.0);
    let top = TITLE_BAR_HEIGHT + CONTENT_MARGIN;
    let bottom = (canvas.height() - FOOTER_HEIGHT).max(top + 180.0);
    let card = Rect::new(left, top, right, bottom);

    let shape = R2RoundedRect::from_rect(card, 24.0);
    let path = shape.to_path(0.1);
    let gradient = Gradient::new_linear((card.x0, card.y0), (card.x1, card.y1)).with_stops([
        (0.0, rgb(0x16, 0x1B, 0x2E)),
        (0.55, rgb(0x25, 0x2F, 0x57)),
        (1.0, rgb(0x0C, 0x10, 0x18)),
    ]);
    canvas.fill(shape, &gradient);

    canvas.with_clip(&path, |canvas| {
        // Slow diagonal light bands.
        canvas.with_transform(Affine::rotate_about(0.18, card.center()), |canvas| {
            let span = card.width() + card.height() + 500.0;
            for i in 0..9 {
                let x = card.x0 - 260.0
                    + (state.phase * 62.0 + f64::from(i) * 136.0).rem_euclid(span);
                canvas.fill_rect(
                    Rect::new(x, card.y0 - 240.0, x + 54.0, card.y1 + 240.0),
                    Paint::solid(colors::WHITE).with_alpha(0.035),
                );
            }
        });

        // Animated orbit on the right side of the card.
        let orbit = (card.x1 - 172.0, card.center().y - 8.0);
        for i in 0..4 {
            canvas.stroke_circle(
                orbit,
                54.0 + f64::from(i) * 32.0,
                Stroke::new(
                    1.0,
                    Paint::solid(colors::WHITE).with_alpha(0.13 - i as f32 * 0.025),
                ),
            );
        }

        let disc = (
            orbit.0 + (state.phase * 0.9).sin() * 28.0,
            orbit.1 + (state.phase * 1.3).cos() * 22.0,
        );
        canvas.fill_circle(disc, 46.0, rgb(0xE3, 0xA8, 0x4C));
        canvas.stroke_circle(
            disc,
            66.0,
            Stroke::new(1.0, Paint::solid(colors::WHITE).with_alpha(0.22)),
        );

        let text_x = card.x0 + 40.0;

        canvas.fill_text(
            font,
            "WINDOW CHROME / FRAMELESS",
            (text_x, card.y0 + 54.0),
            11.0,
            rgb(0xE3, 0xA8, 0x4C),
        );

        canvas.fill_text(
            font,
            "Native motion,\ncustom title bar.",
            (text_x, card.y0 + 106.0),
            38.0,
            colors::WHITE,
        );

        canvas.fill_text(
            font,
            "The system title bar is gone, but the native window frame is\nstill there: minimize, maximize, restore and snap keep animating.",
            (text_x, card.y0 + 202.0),
            14.0,
            Paint::solid(colors::WHITE).with_alpha(0.62),
        );

        let mut pill_x = text_x;
        for label in ["DWM MINIMIZE", "DWM MAXIMIZE", "DWM SNAP"] {
            let width = font.measure(label, 10.0).width + 20.0;
            let pill = Rect::new(pill_x, card.y0 + 254.0, pill_x + width, card.y0 + 280.0);
            canvas.fill_rounded_rect(
                pill,
                13.0,
                Paint::solid(colors::WHITE).with_alpha(0.07),
            );
            canvas.stroke_rounded_rect(
                pill,
                13.0,
                Stroke::new(1.0, Paint::solid(colors::WHITE).with_alpha(0.18)),
            );
            canvas.fill_text(
                font,
                label,
                (pill.x0 + 10.0, pill.y0 + 17.0),
                10.0,
                Paint::solid(colors::WHITE).with_alpha(0.75),
            );
            pill_x = pill.x1 + 8.0;
        }

        let status = format!(
            "window: {}   canvas: {:.0} × {:.0}   scale ×{:.2}",
            if state.maximized {
                "maximized"
            } else {
                "normal"
            },
            canvas.width(),
            canvas.height(),
            canvas.scale_factor(),
        );
        canvas.fill_text(
            font,
            &status,
            (text_x, card.y1 - 28.0),
            11.5,
            Paint::solid(colors::WHITE).with_alpha(0.45),
        );
    });
}

fn draw_footer(canvas: &mut Canvas, font: &Font) {
    let hint = "DRAG TITLE BAR  move     DOUBLE-CLICK  maximize / restore     DRAG EDGES  resize     ALT+F4  close";
    canvas.fill_text(
        font,
        hint,
        (CONTENT_MARGIN, canvas.height() - 22.0),
        11.5,
        colors::DIM_GRAY,
    );
}

fn draw_title_bar(state: &Demo, canvas: &mut Canvas, font: &Font) {
    let layout = Layout::new(canvas.size());
    let bar = layout.title_bar();
    canvas.fill_rect(bar, rgb(0xFB, 0xFB, 0xFC));

    // Application mark.
    canvas.fill_rounded_rect(
        Rect::new(18.0, 13.0, 38.0, 33.0),
        6.0,
        rgb(0x16, 0x1B, 0x2E),
    );
    canvas.fill_circle((28.0, 23.0), 4.0, rgb(0xE3, 0xA8, 0x4C));
    canvas.fill_text(
        font,
        "plasticism / frameless",
        (50.0, 28.0),
        13.5,
        colors::BLACK,
    );

    for action in [Action::Minimize, Action::Maximize, Action::Close] {
        draw_button(state, canvas, &layout, action);
    }

    canvas.line(
        (0.0, bar.y1 - 0.5),
        (canvas.width(), bar.y1 - 0.5),
        Stroke::new(1.0, rgb(0xE2, 0xE4, 0xE8)),
    );
}

fn draw_button(state: &Demo, canvas: &mut Canvas, layout: &Layout, action: Action) {
    let rect = layout.button_rect(action);
    let active = state.hovered == Some(action);
    let background = match (action, active) {
        (Action::Close, true) => rgb(0xC4, 0x2B, 0x1C),
        (_, true) => rgb(0xE9, 0xEB, 0xEF),
        _ => rgb(0xFB, 0xFB, 0xFC),
    };
    canvas.fill_rect(rect, background);

    let color = if action == Action::Close && active {
        colors::WHITE
    } else {
        rgb(0x20, 0x24, 0x2A)
    };
    let center = rect.center();
    let stroke = Stroke::new(1.2, color);

    match action {
        Action::Minimize => {
            canvas.line(
                (center.x - 5.0, center.y + 5.0),
                (center.x + 5.0, center.y + 5.0),
                stroke,
            );
        }
        Action::Maximize if state.maximized => {
            let back = Rect::from_center_size((center.x + 2.0, center.y - 2.0), (9.0, 9.0));
            let front = Rect::from_center_size((center.x - 2.0, center.y + 2.0), (9.0, 9.0));
            canvas.stroke_rect(back, stroke.clone());
            canvas.fill_rect(front, background);
            canvas.stroke_rect(front, stroke);
        }
        Action::Maximize => {
            canvas.stroke_rect(
                Rect::from_center_size(center, (10.0, 10.0)),
                Stroke::new(1.2, color),
            );
        }
        Action::Close => {
            canvas.line(
                (center.x - 5.0, center.y - 5.0),
                (center.x + 5.0, center.y + 5.0),
                stroke.clone(),
            );
            canvas.line(
                (center.x - 5.0, center.y + 5.0),
                (center.x + 5.0, center.y - 5.0),
                stroke,
            );
        }
    }
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
