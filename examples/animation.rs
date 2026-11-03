//! A frame-rate independent animation driven by `Frame::delta`.

use plasticism_draw::prelude::*;

fn main() -> anyhow::Result<()> {
    let font = Font::from_bytes(include_bytes!("../assets/Inter-Regular.ttf"))?;
    let mut pulse = 0.0_f64;
    let mut hue_shift = 0.0_f64;

    App::new("plasticism · animation", 900, 640)
        .background(colors::BLACK)
        .run(move |c, frame| {
            let dt = frame.delta.as_secs_f64();
            pulse = (pulse + dt * 2.2).rem_euclid(std::f64::consts::TAU);
            hue_shift = (hue_shift + dt * 0.06).fract();

            let center = (c.width() / 2.0, c.height() / 2.0);

            for i in 0..18 {
                let phase = f64::from(i) * 0.37;
                let angle = frame.time.as_secs_f64() * 0.35 + phase;
                let distance = 110.0 + (pulse + phase).sin() * 55.0;
                let position = (
                    center.0 + angle.cos() * distance,
                    center.1 + angle.sin() * distance,
                );
                let radius = 12.0 + (pulse * 1.7 + phase).sin().abs() * 14.0;
                let t = (hue_shift + f64::from(i) / 18.0).fract();
                c.fill_circle(position, radius, hsla(t, 0.72, 0.58, 0.9));
            }

            c.stroke_circle(
                center,
                220.0 + pulse.sin() * 16.0,
                Stroke::new(2.0, colors::WHITE).dash(0.0, [8.0, 10.0]),
            );

            let label = format!(
                "frame {:>6}   delta {:>6.2} ms   time {:>6.2} s",
                frame.index,
                dt * 1000.0,
                frame.time.as_secs_f64()
            );
            c.fill_text(
                &font,
                &label,
                (28.0, c.height() - 28.0),
                18.0,
                colors::LIGHT_GRAY,
            );
        })
}

fn hsla(h: f64, s: f64, l: f64, a: f64) -> Color {
    let h = h.rem_euclid(1.0);
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);
    let a = a.clamp(0.0, 1.0);

    if s == 0.0 {
        return Color::new([l as f32, l as f32, l as f32, a as f32]);
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let channel = |t: f64| {
        let t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };

    Color::new([
        channel(h + 1.0 / 3.0) as f32,
        channel(h) as f32,
        channel(h - 1.0 / 3.0) as f32,
        a as f32,
    ])
}
