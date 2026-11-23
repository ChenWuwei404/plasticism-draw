use plasticism_draw::prelude::*;

fn main() -> anyhow::Result<()> {
    App::new("plasticism · shapes", 900, 640)
        .background(colors::WHITE)
        .draw(|c, _frame, _window| {
            c.fill_r2_rounded_rect(Rect::new(20.0, 20.0, 100.0, 60.0), 20.0, colors::GRAY);
            c.fill_r2_rounded_rect(Rect::new(25.0, 25.0, 55.0, 55.0), 15.0, colors::WHITE);
        })
        .run()
}
