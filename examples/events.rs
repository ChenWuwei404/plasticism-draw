//! Raw winit events with user-owned mutable state.
//!
//! This example shows why the stateful `on_event` / `on_draw` entry exists:
//! both callbacks mutate the same `Demo` value, and closing is handled
//! explicitly by the event callback.

use plasticism_draw::prelude::*;

#[derive(Default)]
struct Demo {
    phase: f64,
    pointer: Option<(f64, f64)>,
    last_key: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let font = Font::from_bytes(include_bytes!("../assets/Inter-Regular.ttf"))?;

    App::new("plasticism · events", 960, 640)
        .background(colors::WHITE_SMOKE)
        .state(Demo::default())
        .on_event(|state, event, event_loop, window| match event {
            WindowEvent::CloseRequested => {
                // No default close policy: the application decides.
                event_loop.exit();
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                state.last_key = match &event.logical_key {
                    Key::Character(text) => Some(text.to_string()),
                    Key::Named(name) => Some(format!("{name:?}")),
                    _ => None,
                };
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = window.scale_factor();
                state.pointer = Some((position.x / scale, position.y / scale));
            }
            WindowEvent::CursorLeft { .. } => {
                state.pointer = None;
            }
            _ => {}
        })
        .on_draw(move |state, canvas, frame| {
            state.phase += frame.delta.as_secs_f64();

            let center = (canvas.width() / 2.0, canvas.height() / 2.0);
            let radius = 90.0 + (state.phase * 2.0).sin() * 18.0;

            canvas.fill_circle(center, radius, colors::CRIMSON);
            canvas.stroke_circle(center, radius + 24.0, Stroke::new(3.0, colors::BLACK));

            if let Some(pointer) = state.pointer {
                canvas.stroke_circle(pointer, 22.0, Stroke::new(2.0, colors::DARK_ORANGE));
                canvas.line(center, pointer, Stroke::new(1.0, colors::DIM_GRAY));
            }

            let key = state.last_key.as_deref().unwrap_or("(none)");
            let status = format!(
                "frame {}   key: {}   close via the window button",
                frame.index, key
            );
            canvas.fill_text(
                &font,
                &status,
                (28.0, canvas.height() - 28.0),
                18.0,
                colors::DIM_GRAY,
            );
        })
        .run()
}
