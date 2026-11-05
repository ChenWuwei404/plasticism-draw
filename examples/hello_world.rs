use plasticism_draw::prelude::*;

fn main() -> anyhow::Result<()> {
    let font = Font::from_bytes(include_bytes!("../assets/Inter-Regular.ttf"))?;

    App::new("Hello, plasticism", 560, 360)
        .background(colors::GHOST_WHITE)
        .draw(move |c, _frame| {
            let measured = font.measure("Hello, plasticism", 64.0);
            let x = (c.width() - measured.width) / 2.0;

            c.fill_text(&font, "Hello, plasticism", (x, 160.0), 64.0, colors::BLACK);
        })
        .run()
}