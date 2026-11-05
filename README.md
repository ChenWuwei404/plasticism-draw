# plasticism-draw

A simple way to draw not only simple shapes on canvas.

Based on `winit` and `vello`, `plasticism-draw` provides raw winit event handler and kurbo drawing functions.

## Quick Start

```rust
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
```

Run these examples to explore features:

```bash
cargo run --example shapes         # basic shapes
cargo run --example gradient       # gradient, clip and opacity
cargo run --example clip           # rounded rect clipped text
cargo run --example r2_rounded     # R2 rounded rect
cargo run --example r2_interactive # interactive R2 rounded rect
cargo run --example animation      # animation based on delta
cargo run --example text           # text
cargo run --example events         # raw winit events
cargo run --example dpi -- 1.0     # DPI modes（Auto / Fixed）
cargo run --example generic_state  # App<S> generic
```

## Features

- Configurable DPI mode
- Draw functions on canvas
  - Rect
  - Rounded rect
  - R2 rounded rect
  - Circle
  - Line
  - Polygon
  - Bezier path
- Fill and Stroke
- Transform, clip and opacity
- Vsync render
- Basic text render
- Raw winit events
