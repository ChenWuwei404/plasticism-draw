//! Explicit use of the generic `App<S>` type.
//!
//! `build_app` returns `App<BallState>`, and that state is mutably shared by
//! the raw winit event handler and the drawing callback.

use plasticism_draw::prelude::*;

const BALL_RADIUS: f64 = 46.0;
const FLOOR_MARGIN: f64 = 82.0;

struct BallState {
    position: (f64, f64),
    velocity: (f64, f64),
    bounces: u32,
    paused: bool,
}

impl Default for BallState {
    fn default() -> Self {
        Self {
            position: (240.0, 180.0),
            velocity: (360.0, 280.0),
            bounces: 0,
            paused: false,
        }
    }
}

fn build_app(state: BallState) -> anyhow::Result<App<BallState>> {
    let font = Font::from_bytes(include_bytes!("../assets/Inter-Regular.ttf"))?;

    Ok(App::new("plasticism · generic App<S>", 820, 560)
        .background(colors::WHITE_SMOKE)
        .state(state)
        .on_event(|state, event, event_loop, _window| match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && matches!(&event.logical_key, Key::Named(NamedKey::Space)) =>
            {
                state.paused = !state.paused;
            }
            _ => {}
        })
        .on_draw(move |state, canvas, frame| {
            let width = canvas.width();
            let height = canvas.height();
            let floor = height - FLOOR_MARGIN;

            if !state.paused {
                let dt = frame.delta.as_secs_f64();
                state.position.0 += state.velocity.0 * dt;
                state.position.1 += state.velocity.1 * dt;

                if state.position.0 < BALL_RADIUS {
                    state.position.0 = BALL_RADIUS;
                    state.velocity.0 = state.velocity.0.abs();
                    state.bounces += 1;
                } else if state.position.0 > width - BALL_RADIUS {
                    state.position.0 = width - BALL_RADIUS;
                    state.velocity.0 = -state.velocity.0.abs();
                    state.bounces += 1;
                }

                if state.position.1 < BALL_RADIUS {
                    state.position.1 = BALL_RADIUS;
                    state.velocity.1 = state.velocity.1.abs();
                    state.bounces += 1;
                } else if state.position.1 > floor - BALL_RADIUS {
                    state.position.1 = floor - BALL_RADIUS;
                    state.velocity.1 = -state.velocity.1.abs();
                    state.bounces += 1;
                }
            }

            canvas.fill_rect(
                Rect::from_origin_size((0.0, 0.0), (width, height)),
                colors::WHITE_SMOKE,
            );
            canvas.line(
                (0.0, floor),
                (width, floor),
                Stroke::new(3.0, colors::DIM_GRAY),
            );

            if !state.paused {
                canvas.stroke_circle(
                    state.position,
                    BALL_RADIUS + 10.0,
                    Stroke::new(2.0, colors::DARK_ORANGE).dash(0.0, [8.0, 8.0]),
                );
            }

            canvas.fill_circle(state.position, BALL_RADIUS, colors::CRIMSON);
            canvas.stroke_circle(state.position, BALL_RADIUS, Stroke::new(3.0, colors::BLACK));

            let status = format!(
                "App<BallState>  ·  bounces: {}  ·  {}  ·  Space: pause/resume",
                state.bounces,
                if state.paused { "paused" } else { "running" },
            );
            canvas.fill_text(
                &font,
                &status,
                (28.0, height - 28.0),
                18.0,
                colors::DIM_GRAY,
            );
        }))
}

fn main() -> anyhow::Result<()> {
    let app: App<BallState> = build_app(BallState::default())?;
    app.run()
}
