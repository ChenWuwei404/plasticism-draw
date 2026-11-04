//! Basic shapes, paths, strokes, and transforms.

use plasticism_draw::prelude::*;

fn main() -> anyhow::Result<()> {
    App::new("plasticism · shapes", 900, 640)
        .background(colors::WHITE)
        .draw(|c, frame| {
            let t = frame.time.as_secs_f64();

            let rect = Rect::from_origin_size((48.0, 48.0), (280.0, 170.0));
            c.fill_rect(rect, colors::SKY_BLUE);
            c.stroke_rect(rect, Stroke::new(4.0, colors::BLACK));

            c.fill_rounded_rect(
                Rect::from_origin_size((420.0, 48.0), (260.0, 170.0)),
                28.0,
                colors::LIGHT_GREEN,
            );
            c.stroke_rounded_rect(
                Rect::from_origin_size((420.0, 48.0), (260.0, 170.0)),
                28.0,
                Stroke::new(3.0, colors::BLACK),
            );

            c.fill_circle((210.0, 380.0), 92.0, colors::CRIMSON);
            c.stroke_circle((210.0, 380.0), 112.0, Stroke::new(3.0, colors::BLACK));

            c.fill_ellipse((520.0, 380.0), (120.0, 62.0), colors::GOLD);
            c.stroke_ellipse(
                (520.0, 380.0),
                (120.0, 62.0),
                Stroke::new(3.0, colors::BLACK),
            );

            c.line(
                (70.0, 550.0),
                (830.0, 550.0),
                Stroke::new(3.0, colors::DIM_GRAY).dash(0.0, [16.0, 10.0]),
            );

            c.fill_polygon(
                [(680.0, 500.0), (760.0, 500.0), (720.0, 570.0)],
                colors::MEDIUM_PURPLE,
            );
            c.stroke_polygon(
                [(680.0, 500.0), (760.0, 500.0), (720.0, 570.0)],
                Stroke::new(2.0, colors::BLACK),
            );

            let mut path = BezPath::new();
            path.move_to((120.0, 300.0));
            path.quad_to((180.0, 220.0), (250.0, 300.0));
            path.curve_to((300.0, 340.0), (350.0, 260.0), (400.0, 300.0));
            c.stroke_path(&path, Stroke::new(5.0, colors::DARK_ORANGE));

            c.with_transform(Affine::rotate_about(t * 0.6, (780.0, 230.0)), |c| {
                c.fill_rect(
                    Rect::from_center_size((780.0, 230.0), (70.0, 70.0)),
                    colors::TOMATO,
                );
            });
        })
        .run()
}
