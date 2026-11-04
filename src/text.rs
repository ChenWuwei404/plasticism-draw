//! Font loading and a small left-to-right text layout implementation.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use glifo::Glyph;
use skrifa::instance::{LocationRef, Size as SkrifaSize};
use skrifa::{FontRef, GlyphId, MetadataProvider};
use vello_gpu::peniko::{Blob, FontData};

use crate::kurbo::Size;

/// A font that can be passed to [`Canvas::fill_text`](crate::Canvas::fill_text)
/// and [`Canvas::stroke_text`](crate::Canvas::stroke_text).
///
/// The font data is kept in memory and can be shared cheaply by cloning the
/// value. For a TrueType/OpenType collection, use one of the `*_index`
/// constructors to select a face.
#[derive(Clone, Debug)]
pub struct Font {
    data: FontData,
}

impl Font {
    /// Loads the first face of an in-memory font.
    pub fn from_bytes(bytes: impl AsRef<[u8]>) -> Result<Self> {
        Self::from_bytes_index(bytes, 0)
    }

    /// Loads a specific face from an in-memory font or font collection.
    pub fn from_bytes_index(bytes: impl AsRef<[u8]>, index: u32) -> Result<Self> {
        let bytes = bytes.as_ref().to_vec();
        FontRef::from_index(&bytes, index)
            .with_context(|| format!("invalid font data (collection index {index})"))?;

        Ok(Self {
            data: FontData::new(Blob::new(Arc::new(bytes)), index),
        })
    }

    /// Reads and loads the first face of a font file.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_file_index(path, 0)
    }

    /// Reads and loads a specific face from a font file or collection.
    pub fn from_file_index(path: impl AsRef<Path>, index: u32) -> Result<Self> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read font {}", path.display()))?;
        Self::from_bytes_index(bytes, index)
            .with_context(|| format!("failed to load font {}", path.display()))
    }

    /// Measures multi-line text at `size` canvas units per em.
    ///
    /// The returned height uses a line height of `1.2 * size` per line.
    pub fn measure(&self, text: &str, size: f64) -> Size {
        let layout = self.layout(text, size);
        Size::new(layout.width, layout.height)
    }

    pub(crate) fn data(&self) -> &FontData {
        &self.data
    }

    pub(crate) fn layout(&self, text: &str, size: f64) -> TextLayout {
        if text.is_empty() || !size.is_finite() || size <= 0.0 {
            return TextLayout {
                glyphs: Vec::new(),
                width: 0.0,
                height: 0.0,
            };
        }

        let size = size as f32;
        let line_height = size * 1.2;
        let font = self.font_ref();
        let charmap = font.charmap();
        let metrics = font.glyph_metrics(SkrifaSize::new(size), LocationRef::default());
        let notdef = GlyphId::new(0);
        let space_advance = charmap
            .map(' ')
            .and_then(|glyph| metrics.advance_width(glyph))
            .unwrap_or(0.0);

        let mut glyphs = Vec::with_capacity(text.chars().count());
        let mut x = 0.0_f32;
        let mut line = 0_u32;
        let mut max_width = 0.0_f32;

        for ch in text.chars() {
            match ch {
                '\n' => {
                    max_width = max_width.max(x);
                    x = 0.0;
                    line += 1;
                }
                '\r' => {}
                '\t' => {
                    x += space_advance * 4.0;
                }
                _ => {
                    let glyph_id = charmap.map(ch).unwrap_or(notdef);
                    let advance = metrics.advance_width(glyph_id).unwrap_or(0.0);
                    glyphs.push(Glyph {
                        id: glyph_id.to_u32(),
                        x,
                        y: line as f32 * line_height,
                    });
                    x += advance;
                }
            }
        }

        max_width = max_width.max(x);
        let line_count = line + 1;

        TextLayout {
            glyphs,
            width: f64::from(max_width),
            height: f64::from(line_count as f32 * line_height),
        }
    }

    fn font_ref(&self) -> FontRef<'_> {
        FontRef::from_index(self.data.data.data(), self.data.index)
            .expect("font data was validated when it was created")
    }
}

pub(crate) struct TextLayout {
    pub(crate) glyphs: Vec<Glyph>,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

#[cfg(test)]
mod tests {
    use super::Font;

    const INTER: &[u8] = include_bytes!("../assets/Inter-Regular.ttf");

    #[test]
    fn basic_layout_advances_glyphs() {
        let font = Font::from_bytes(INTER).unwrap();
        let layout = font.layout("Hello", 32.0);

        assert_eq!(layout.glyphs.len(), 5);
        assert_eq!(layout.glyphs[0].x, 0.0);
        assert!(layout.glyphs[1].x > layout.glyphs[0].x);
        assert!(layout.width > 0.0);
        assert!(layout.height > 0.0);
    }

    #[test]
    fn newline_resets_x_and_increases_y() {
        let font = Font::from_bytes(INTER).unwrap();
        let layout = font.layout("A\nB", 20.0);

        assert_eq!(layout.glyphs.len(), 2);
        assert_eq!(layout.glyphs[1].x, 0.0);
        assert!(layout.glyphs[1].y > layout.glyphs[0].y);
    }

    #[test]
    fn missing_glyph_falls_back_to_notdef() {
        let font = Font::from_bytes(INTER).unwrap();
        let layout = font.layout("\u{10FFFF}", 20.0);

        assert_eq!(layout.glyphs.len(), 1);
        assert_eq!(layout.glyphs[0].id, 0);
    }

    #[test]
    fn measure_matches_layout_extents() {
        let font = Font::from_bytes(INTER).unwrap();
        let size = font.measure("Hello\nworld", 24.0);

        assert!(size.width > 0.0);
        assert!(size.height > 24.0);
    }

    #[test]
    fn invalid_font_is_rejected() {
        assert!(Font::from_bytes([0_u8, 1, 2, 3]).is_err());
    }
}
