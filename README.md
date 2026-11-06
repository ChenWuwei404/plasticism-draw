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
cargo run --example image          # bitmap loading, scaling, fit modes and alpha icons
cargo run --example icons          # vector SVG icons and alpha-mask bitmap icons
cargo run --example events         # raw winit events
cargo run --example dpi -- 1.0     # DPI modes（Auto / Fixed）
cargo run --example generic_state  # App<S> generic
```

## Images and icons

```rust
use plasticism_draw::prelude::*;

fn main() -> anyhow::Result<()> {
    let photo = Image::open("photo.png")?;
    let star = Image::open("star.png")?; // an icon with transparency
    let home = VectorIcon::from_svg_file("home.svg")?;

    App::new("images", 800, 600)
        .draw(move |c, _frame| {
            // Stretch a bitmap to a rectangle.
            c.draw_image(&photo, Rect::from_origin_size((40.0, 40.0), (300.0, 200.0)));

            // Preserve the aspect ratio and center inside the rectangle.
            c.draw_image_fit(
                &photo,
                Rect::from_origin_size((380.0, 40.0), (300.0, 200.0)),
                ImageFit::Contain,
            );

            // Fill a vector icon with a solid color or a gradient.
            c.draw_icon(
                &home,
                Rect::from_center_size((400.0, 360.0), (96.0, 96.0)),
                colors::BLACK,
            );

            // Tint a bitmap icon through its alpha channel.
            c.draw_bitmap_icon(
                &star,
                Rect::from_center_size((600.0, 360.0), (72.0, 72.0)),
                colors::GOLD,
            );
        })
        .run()
}
```

`Image::open` decodes PNG, JPEG, WebP, BMP, GIF, ICO and TIFF files. Pixels are uploaded automatically before the first frame that uses them and are cached on the GPU. An image created inside the draw callback becomes visible on the next frame.

`ImageFit::Fill` stretches, `Contain` letterboxes and `Cover` crops. Vector icons accept SVG path data with `VectorIcon::from_svg_path`, or a complete SVG document with `from_svg`, `from_svg_bytes` and `from_svg_file`. SVG transforms and strokes are converted into path geometry; colors from the SVG are ignored, so every part is filled with the supplied `Paint`. Path data has no fill rule of its own, so use `with_fill_rule(Fill::EvenOdd)` for even-odd holes. Bitmap icons use only the alpha channel of the bitmap and a solid tint color.

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
- Bitmap images
  - PNG, JPEG, WebP, BMP, GIF, ICO and TIFF loading
  - Scaling through the destination rectangle
  - `Fill`, `Contain` and `Cover` fitting
  - Automatic upload and GPU caching
- Vector icons
  - SVG path data or complete `.svg` files
  - `viewBox`, transforms and stroke outlines
  - Filled with any `Paint`, including gradients
- Alpha-mask bitmap icons
  - Uses the bitmap alpha channel as a mask
  - Tinted with a solid color
- Raw winit events
