//! Font loading, text styling, shaping, line breaking, and layout.
//!
//! Text layout is powered by [parley](https://crates.io/crates/parley):
//! fontique discovers and falls back across system fonts, harfrust performs
//! shaping (kerning, ligatures, variable axes, synthetic bold/italic), and
//! ICU handles bidirectional text and line breaking. The resulting positioned
//! glyph runs are flattened into a [`TextLayout`] that [`Canvas`] can render
//! directly through glifo.
//!
//! [`Canvas`]: crate::Canvas

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::ops::{Bound, Range, RangeBounds};
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use parley::fontique::FontInfoOverride;
use parley::layout::{PositionedLayoutItem, RunMetrics as ParleyRunMetrics};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontData, FontFamily, FontFamilyName, FontFeatures,
    FontVariations, Layout, LayoutContext, LineHeight, RangedBuilder, StyleProperty,
};
use skrifa::string::StringId;
use skrifa::{FontRef, MetadataProvider};

use crate::Color;
use crate::kurbo::Size;
use crate::peniko::Blob;

pub use parley::setting::Tag;
pub use parley::style::{
    FontFeature, FontStyle, FontVariation, FontWeight, FontWidth, GenericFamily,
};

const LAYOUT_CACHE_CAPACITY: usize = 128;
const DEFAULT_LINE_HEIGHT_FACTOR: f64 = 1.2;
const SYNTHETIC_BOLD_SCALE: f32 = 0.02;
const UNNAMED_FAMILY: &str = "plasticism-draw-font";

/// How horizontal lines are aligned relative to the anchor point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TextAlign {
    /// Align to the start edge: left for LTR text, right for RTL text.
    #[default]
    Start,
    /// Align to the end edge: right for LTR text, left for RTL text.
    End,
    /// Align to the physical left edge.
    Left,
    /// Center each line on the anchor point.
    Center,
    /// Align to the physical right edge.
    Right,
    /// Stretch each line except the last to the layout width.
    Justify,
}

/// A font family or ordered family stack.
///
/// A `Font` created from bytes registers that font in the current thread's
/// text engine. Loading several faces with the same family name groups them,
/// so [`TextStyle::weight`] and [`TextStyle::font_style`] can select the
/// matching face. Missing glyphs fall back to the system fonts discovered by
/// fontique.
///
/// The layout cache and the registered font data are shared cheaply when a
/// `Font` is cloned. The first layout on a thread may discover system fonts;
/// later layouts reuse the thread-local font and layout contexts.
#[derive(Clone)]
pub struct Font {
    inner: Arc<FontInner>,
}

impl fmt::Debug for Font {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Font")
            .field("family", &self.inner.family_label)
            .field("fallbacks", &self.inner.fallbacks)
            .field("loaded_data", &self.inner.data.is_some())
            .finish()
    }
}

#[derive(Debug)]
struct FontInner {
    primary: FontFamilyName<'static>,
    fallbacks: Vec<FontFamilyName<'static>>,
    family_label: String,
    data: Option<FontData>,
    registration_override: Option<String>,
    cache: Mutex<LayoutCache>,
}

impl Font {
    /// Loads the first face of an in-memory font.
    pub fn from_bytes(bytes: impl AsRef<[u8]>) -> Result<Self> {
        Self::from_bytes_index(bytes, 0)
    }

    /// Loads a specific face from an in-memory font or font collection.
    pub fn from_bytes_index(bytes: impl AsRef<[u8]>, index: u32) -> Result<Self> {
        let bytes = bytes.as_ref().to_vec();
        let family = {
            let font = FontRef::from_index(&bytes, index)
                .with_context(|| format!("invalid font data (collection index {index})"))?;
            font_family_name(&font)
        };

        let (label, registration_override) = match family {
            Some(family) => (family, None),
            None => (UNNAMED_FAMILY.to_owned(), Some(UNNAMED_FAMILY.to_owned())),
        };

        Ok(Self {
            inner: Arc::new(FontInner {
                primary: FontFamilyName::Named(Cow::Owned(label.clone())),
                fallbacks: Vec::new(),
                family_label: label,
                data: Some(FontData::new(Blob::new(Arc::new(bytes)), index)),
                registration_override,
                cache: Mutex::new(LayoutCache::default()),
            }),
        })
    }

    /// Reads and loads the first face of a font file.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_file_index(path, 0)
    }

    /// Reads and loads a specific face of a font file or collection.
    pub fn from_file_index(path: impl AsRef<Path>, index: u32) -> Result<Self> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read font {}", path.display()))?;
        Self::from_bytes_index(bytes, index)
            .with_context(|| format!("failed to load font {}", path.display()))
    }

    /// Uses a system font family by name.
    ///
    /// Generic CSS names such as `"sans-serif"` are parsed as generic
    /// families. The family is resolved lazily at layout time, so a missing
    /// family simply falls back to the platform defaults.
    pub fn from_system(family: impl Into<String>) -> Self {
        let family = family.into();
        let family = family.trim();
        let label = family.to_owned();
        let primary = family_spec_to_name(&parse_family_spec(family));
        Self {
            inner: Arc::new(FontInner {
                primary,
                fallbacks: Vec::new(),
                family_label: label,
                data: None,
                registration_override: None,
                cache: Mutex::new(LayoutCache::default()),
            }),
        }
    }

    /// Uses a CSS generic font family.
    pub fn from_generic(family: GenericFamily) -> Self {
        Self {
            inner: Arc::new(FontInner {
                primary: FontFamilyName::Generic(family),
                fallbacks: Vec::new(),
                family_label: family.to_string(),
                data: None,
                registration_override: None,
                cache: Mutex::new(LayoutCache::default()),
            }),
        }
    }

    /// Appends explicit fallback families, tried before automatic system
    /// fallback.
    #[must_use]
    pub fn with_fallbacks<I, S>(self, families: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut fallbacks = self.inner.fallbacks.clone();
        fallbacks.extend(
            families
                .into_iter()
                .map(|family| family_spec_to_name(&parse_family_spec(&family.into()))),
        );

        Self {
            inner: Arc::new(FontInner {
                primary: self.inner.primary.clone(),
                fallbacks,
                family_label: self.inner.family_label.clone(),
                data: self.inner.data.clone(),
                registration_override: self.inner.registration_override.clone(),
                cache: Mutex::new(LayoutCache::default()),
            }),
        }
    }

    /// Returns the primary family name used for layout.
    pub fn family_name(&self) -> &str {
        &self.inner.family_label
    }

    /// Returns the font metrics for the face selected by `style`.
    ///
    /// The metrics describe the primary resolved face at `style`'s size,
    /// weight, style, width, and variation settings. For text that mixes
    /// fallback fonts, use [`TextLayout::line_metrics`] or
    /// [`TextLayout::run_metrics`] instead.
    pub fn metrics(&self, style: &TextStyle) -> FontMetrics {
        let layout = self.layout(" ", style);
        let Some(run) = layout.inner.runs.first() else {
            return FontMetrics {
                size: style.size,
                ..FontMetrics::default()
            };
        };

        let metrics = run.metrics;
        FontMetrics {
            size: style.size,
            ascent: f64::from(metrics.ascent),
            descent: f64::from(metrics.descent),
            line_gap: f64::from(metrics.leading),
            line_height: f64::from(metrics.ascent + metrics.descent + metrics.leading),
            cap_height: metrics.cap_height.map(f64::from),
            x_height: metrics.x_height.map(f64::from),
            underline_offset: Some(f64::from(metrics.underline_offset)),
            underline_size: Some(f64::from(metrics.underline_size)),
            strikethrough_offset: Some(f64::from(metrics.strikethrough_offset)),
            strikethrough_size: Some(f64::from(metrics.strikethrough_size)),
        }
    }

    /// Shapes and lays out `text` with a single style.
    ///
    /// The result is cached; calling this repeatedly with the same text and
    /// style only returns a cheap handle to the existing layout.
    pub fn layout(&self, text: &str, style: &TextStyle) -> TextLayout {
        let key = LayoutKey::Simple(style.key());
        if let Some(inner) = self.inner.cache_get(text, &key) {
            return TextLayout { inner };
        }

        let inner = Arc::new(build_simple_layout(&self.inner, text, style));
        self.inner.cache_insert(text, key, Arc::clone(&inner));
        TextLayout { inner }
    }

    /// Shapes and lays out `text` with per-range rich-text styles and colors.
    ///
    /// Layout-level properties such as alignment and wrap width come from the
    /// rich text's base style. Spans override font attributes, line height, and
    /// spacing; colors apply to filled glyph runs.
    pub fn layout_rich(&self, text: &str, rich: &RichText) -> TextLayout {
        let normalized = rich.normalized(text);
        let key = LayoutKey::Rich(RichTextKey::new(&normalized));
        if let Some(inner) = self.inner.cache_get(text, &key) {
            return TextLayout { inner };
        }

        let inner = Arc::new(build_rich_layout(&self.inner, text, &normalized));
        self.inner.cache_insert(text, key, Arc::clone(&inner));
        TextLayout { inner }
    }

    /// Measures multi-line text at `size` canvas units per em.
    ///
    /// This is the default style: 1.2 times the font size per line, left
    /// aligned, no wrapping, and no extra letter spacing.
    pub fn measure(&self, text: &str, size: f64) -> Size {
        self.layout(text, &TextStyle::new(size)).size()
    }
}

impl FontInner {
    fn cache_get(&self, text: &str, key: &LayoutKey) -> Option<Arc<TextLayoutInner>> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(text, key)
    }

    fn cache_insert(&self, text: &str, key: LayoutKey, layout: Arc<TextLayoutInner>) {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(text, key, layout);
    }

    fn family_for(&self, override_family: Option<&FamilySpec>) -> FontFamily<'static> {
        let mut families = Vec::with_capacity(1 + self.fallbacks.len());
        families.push(match override_family {
            Some(family) => family_spec_to_name(family),
            None => self.primary.clone(),
        });
        families.extend(self.fallbacks.iter().cloned());
        FontFamily::List(Cow::Owned(families))
    }
}

/// The style used to shape and lay out text.
///
/// The default line height is 1.2 times the font size, matching the historic
/// behavior of this crate. Use [`TextStyle::line_height`] for an absolute line
/// height or [`TextStyle::line_height_factor`] for a different multiple.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    size: f64,
    weight: FontWeight,
    font_style: FontStyle,
    font_width: FontWidth,
    family: Option<FamilySpec>,
    features: Vec<FontFeature>,
    variations: Vec<FontVariation>,
    line_height: LineHeightSpec,
    letter_spacing: f64,
    word_spacing: f64,
    align: TextAlign,
    wrap_width: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum FamilySpec {
    Named(String),
    Generic(GenericFamily),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum LineHeightSpec {
    Factor(f64),
    Absolute(f64),
}

impl Default for TextStyle {
    fn default() -> Self {
        Self::new(16.0)
    }
}

impl TextStyle {
    /// Creates a style with `size` canvas units per em and the default
    /// attributes.
    ///
    /// Non-finite or non-positive sizes produce an empty layout.
    pub fn new(size: f64) -> Self {
        Self {
            size: normalize_size(size),
            weight: FontWeight::NORMAL,
            font_style: FontStyle::Normal,
            font_width: FontWidth::NORMAL,
            family: None,
            features: Vec::new(),
            variations: Vec::new(),
            line_height: LineHeightSpec::Factor(DEFAULT_LINE_HEIGHT_FACTOR),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            align: TextAlign::Start,
            wrap_width: None,
        }
    }

    /// Sets the font size in canvas units per em.
    pub fn size(mut self, size: f64) -> Self {
        self.size = normalize_size(size);
        self
    }

    /// Sets the requested font weight.
    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    /// Sets the requested font style (normal, italic, or oblique).
    pub fn font_style(mut self, style: FontStyle) -> Self {
        self.font_style = style;
        self
    }

    /// Sets the requested font width.
    pub fn font_width(mut self, width: FontWidth) -> Self {
        self.font_width = width;
        self
    }

    /// Overrides the primary family for this style.
    ///
    /// [`Font`]'s family is used when this is not set.
    pub fn family(mut self, family: impl Into<String>) -> Self {
        self.family = Some(parse_family_spec(&family.into()));
        self
    }

    /// Overrides the primary family with a CSS generic family.
    pub fn generic_family(mut self, family: GenericFamily) -> Self {
        self.family = Some(FamilySpec::Generic(family));
        self
    }

    /// Sets raw OpenType font features for the whole style.
    pub fn features(mut self, features: impl IntoIterator<Item = FontFeature>) -> Self {
        self.features = features.into_iter().collect();
        self
    }

    /// Sets raw variable-font variation axes for the whole style.
    pub fn variations(mut self, variations: impl IntoIterator<Item = FontVariation>) -> Self {
        self.variations = variations.into_iter().collect();
        self
    }

    /// Enables or disables the OpenType `kern` feature.
    ///
    /// Kerning is enabled by the shaper by default; this only adds an explicit
    /// feature setting. Later calls and [`TextStyle::features`] take precedence
    /// when they set the same tag.
    pub fn kerning(mut self, enabled: bool) -> Self {
        self.set_feature(Tag::new(b"kern"), enabled);
        self
    }

    /// Enables or disables the OpenType `liga` feature.
    ///
    /// Standard ligatures are enabled by the shaper by default; this only adds
    /// an explicit feature setting. Later calls and [`TextStyle::features`]
    /// take precedence when they set the same tag.
    pub fn ligatures(mut self, enabled: bool) -> Self {
        self.set_feature(Tag::new(b"liga"), enabled);
        self
    }

    /// Sets an absolute line height in canvas units.
    pub fn line_height(mut self, height: f64) -> Self {
        self.line_height = LineHeightSpec::Absolute(if height.is_finite() {
            height.max(0.0)
        } else {
            0.0
        });
        self
    }

    /// Sets the line height as a multiple of the font size.
    ///
    /// The default is 1.2. Non-finite or non-positive factors fall back to 1.2.
    pub fn line_height_factor(mut self, factor: f64) -> Self {
        self.line_height = LineHeightSpec::Factor(if factor.is_finite() && factor > 0.0 {
            factor
        } else {
            DEFAULT_LINE_HEIGHT_FACTOR
        });
        self
    }

    /// Sets extra spacing between letters, in canvas units.
    pub fn letter_spacing(mut self, spacing: f64) -> Self {
        self.letter_spacing = normalize_spacing(spacing);
        self
    }

    /// Sets extra spacing between words, in canvas units.
    pub fn word_spacing(mut self, spacing: f64) -> Self {
        self.word_spacing = normalize_spacing(spacing);
        self
    }

    /// Sets the horizontal alignment of each line.
    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    /// Enables automatic line breaking at `width` canvas units.
    ///
    /// Non-finite or non-positive widths disable wrapping.
    pub fn wrap_width(mut self, width: f64) -> Self {
        self.wrap_width = (width.is_finite() && width > 0.0).then_some(width);
        self
    }

    /// Disables automatic line breaking.
    pub fn no_wrap(mut self) -> Self {
        self.wrap_width = None;
        self
    }

    fn set_feature(&mut self, tag: Tag, enabled: bool) {
        self.features.retain(|feature| feature.tag != tag);
        self.features
            .push(FontFeature::new(tag, if enabled { 1 } else { 0 }));
    }

    fn key(&self) -> TextStyleKey {
        let (style_kind, style_angle) = match self.font_style {
            FontStyle::Normal => (0, None),
            FontStyle::Italic => (1, None),
            FontStyle::Oblique(angle) => (2, angle.map(f32::to_bits)),
        };
        TextStyleKey {
            size_bits: self.size.to_bits(),
            weight_bits: self.weight.value().to_bits(),
            style_kind,
            style_angle,
            width_bits: self.font_width.ratio().to_bits(),
            family: self.family.clone(),
            features: self
                .features
                .iter()
                .map(|feature| (feature.tag.to_bytes(), feature.value))
                .collect(),
            variations: self
                .variations
                .iter()
                .map(|variation| (variation.tag.to_bytes(), variation.value.to_bits()))
                .collect(),
            line_height: match self.line_height {
                LineHeightSpec::Factor(value) => (0, value.to_bits()),
                LineHeightSpec::Absolute(value) => (1, value.to_bits()),
            },
            letter_spacing_bits: self.letter_spacing.to_bits(),
            word_spacing_bits: self.word_spacing.to_bits(),
            align: self.align as u8,
            wrap_width_bits: self.wrap_width.map(f64::to_bits),
        }
    }
}

/// A rich-text string with per-range styles and fill colors.
///
/// The base style controls layout-level properties such as alignment and wrap
/// width. Span styles override font attributes, line height, and spacing.
/// Later spans win where ranges overlap. Ranges are byte offsets into the
/// text; invalid or partial UTF-8 boundaries are clamped to the nearest valid
/// boundary.
///
/// A span with no family override inherits the base family. Spans with empty
/// feature or variation lists inherit those settings from the base style.
/// Colors apply to filled glyph runs; strokes use the single stroke passed to
/// the canvas.
#[derive(Clone, Debug)]
pub struct RichText {
    base: TextStyle,
    spans: Vec<RichSpan>,
    colors: Vec<ColorSpan>,
}

#[derive(Clone, Debug)]
struct RichSpan {
    range: Range<usize>,
    style: TextStyle,
}

#[derive(Clone, Debug)]
struct ColorSpan {
    range: Range<usize>,
    color: Color,
}

impl RichText {
    /// Creates a rich-text value with a base style.
    pub fn new(base: TextStyle) -> Self {
        Self {
            base,
            spans: Vec::new(),
            colors: Vec::new(),
        }
    }

    /// Overrides the complete style of a byte range.
    ///
    /// The span style's `align` and `wrap_width` are ignored; those layout
    /// properties always come from the base style.
    #[must_use]
    pub fn span(mut self, range: impl RangeBounds<usize>, style: TextStyle) -> Self {
        self.spans.push(RichSpan {
            range: raw_range(range),
            style,
        });
        self
    }

    /// Overrides the style of a byte range, starting from the base style.
    ///
    /// The span style's `align` and `wrap_width` are ignored.
    #[must_use]
    pub fn span_with(
        mut self,
        range: impl RangeBounds<usize>,
        modify: impl FnOnce(TextStyle) -> TextStyle,
    ) -> Self {
        let style = modify(self.base.clone());
        self.spans.push(RichSpan {
            range: raw_range(range),
            style,
        });
        self
    }

    /// Sets the fill color of a byte range.
    ///
    /// Color ranges are independent of style ranges and may overlap them.
    /// Later colors win where ranges overlap.
    #[must_use]
    pub fn color(mut self, range: impl RangeBounds<usize>, color: Color) -> Self {
        self.colors.push(ColorSpan {
            range: raw_range(range),
            color,
        });
        self
    }

    fn normalized(&self, text: &str) -> NormalizedRichText {
        NormalizedRichText {
            base: self.base.clone(),
            spans: self
                .spans
                .iter()
                .filter_map(|span| {
                    normalize_range(text, &span.range).map(|range| (range, span.style.clone()))
                })
                .collect(),
            colors: self
                .colors
                .iter()
                .filter_map(|span| {
                    normalize_range(text, &span.range).map(|range| (range, span.color))
                })
                .collect(),
        }
    }
}

/// Font-wide metrics for one resolved face.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FontMetrics {
    /// The requested font size in canvas units.
    pub size: f64,
    /// Distance from the baseline to the top of the alignment box, positive.
    pub ascent: f64,
    /// Distance from the baseline to the bottom of the alignment box, positive.
    pub descent: f64,
    /// Recommended additional spacing between lines.
    pub line_gap: f64,
    /// `ascent + descent + line_gap`.
    pub line_height: f64,
    /// Distance from the baseline to the top of a capital letter.
    pub cap_height: Option<f64>,
    /// Distance from the baseline to the top of a lowercase `x`.
    pub x_height: Option<f64>,
    /// Offset of the underline from the baseline.
    pub underline_offset: Option<f64>,
    /// Thickness of the underline.
    pub underline_size: Option<f64>,
    /// Offset of the strikethrough from the baseline.
    pub strikethrough_offset: Option<f64>,
    /// Thickness of the strikethrough.
    pub strikethrough_size: Option<f64>,
}

/// Aggregate metrics for a laid-out string.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextMetrics {
    /// Width of the widest line, excluding trailing whitespace.
    pub width: f64,
    /// Width of the widest line, including trailing whitespace.
    pub full_width: f64,
    /// Total height of all lines.
    pub height: f64,
    /// Number of laid-out lines.
    pub line_count: usize,
    /// The alignment box width: the wrap width, or the widest line when
    /// wrapping is disabled.
    pub box_width: f64,
}

/// Metrics for one laid-out line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LineMetrics {
    /// Typographic ascent of the tallest run on the line.
    pub ascent: f64,
    /// Typographic descent of the deepest run on the line, positive.
    pub descent: f64,
    /// Recommended additional spacing between lines.
    pub line_gap: f64,
    /// Effective line height.
    pub line_height: f64,
    /// Baseline offset from the top of the layout.
    pub baseline: f64,
    /// Line advance excluding trailing whitespace.
    pub width: f64,
    /// Advance of trailing whitespace on the line.
    pub trailing_whitespace: f64,
}

/// Metrics for one shaped glyph run.
///
/// A single laid-out string can contain multiple runs when font fallback or
/// per-range styles split it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RunMetrics {
    /// Font size used by this run, in canvas units.
    pub font_size: f64,
    /// Typographic ascent, positive.
    pub ascent: f64,
    /// Typographic descent, positive.
    pub descent: f64,
    /// Recommended additional spacing between lines.
    pub line_gap: f64,
    /// Effective line height for this run.
    pub line_height: f64,
}

/// A shaped and positioned multi-line text layout.
///
/// Glyph coordinates are relative to the anchor point used when drawing:
/// `anchor.y` is the first baseline, and `anchor.x` is the start, center, or
/// end of the alignment box selected by the style. Cloning a `TextLayout` is
/// cheap.
#[derive(Clone, Debug)]
pub struct TextLayout {
    inner: Arc<TextLayoutInner>,
}

impl TextLayout {
    /// Returns aggregate metrics for the layout.
    pub fn metrics(&self) -> TextMetrics {
        self.inner.metrics
    }

    /// Returns the layout width and height.
    pub fn size(&self) -> Size {
        Size::new(self.inner.metrics.width, self.inner.metrics.height)
    }

    /// Returns metrics for each line.
    pub fn line_metrics(&self) -> &[LineMetrics] {
        &self.inner.lines
    }

    /// Returns metrics for each shaped glyph run.
    pub fn run_metrics(&self) -> &[RunMetrics] {
        &self.inner.run_metrics
    }

    /// Returns `true` when the layout contains no drawable glyphs.
    pub fn is_empty(&self) -> bool {
        self.inner.runs.is_empty()
    }

    /// Returns `true` when the dominant text direction is right-to-left.
    pub fn is_rtl(&self) -> bool {
        self.inner.is_rtl
    }

    pub(crate) fn runs(&self) -> &[TextRunData] {
        &self.inner.runs
    }
}

#[derive(Debug, Default)]
struct LayoutCache {
    by_text: HashMap<String, Vec<(LayoutKey, Arc<TextLayoutInner>)>>,
    len: usize,
}

impl LayoutCache {
    fn get(&self, text: &str, key: &LayoutKey) -> Option<Arc<TextLayoutInner>> {
        self.by_text
            .get(text)?
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, layout)| Arc::clone(layout))
    }

    fn insert(&mut self, text: &str, key: LayoutKey, layout: Arc<TextLayoutInner>) {
        if self.len >= LAYOUT_CACHE_CAPACITY {
            self.by_text.clear();
            self.len = 0;
        }

        let entries = self.by_text.entry(text.to_owned()).or_default();
        if let Some((_, existing)) = entries.iter_mut().find(|(candidate, _)| *candidate == key) {
            *existing = layout;
        } else {
            entries.push((key, layout));
            self.len += 1;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum LayoutKey {
    Simple(TextStyleKey),
    Rich(RichTextKey),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TextStyleKey {
    size_bits: u64,
    weight_bits: u32,
    style_kind: u8,
    style_angle: Option<u32>,
    width_bits: u32,
    family: Option<FamilySpec>,
    features: Vec<([u8; 4], u16)>,
    variations: Vec<([u8; 4], u32)>,
    line_height: (u8, u64),
    letter_spacing_bits: u64,
    word_spacing_bits: u64,
    align: u8,
    wrap_width_bits: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct RichTextKey {
    base: TextStyleKey,
    spans: Vec<(Range<usize>, TextStyleKey)>,
    colors: Vec<(Range<usize>, [u32; 4])>,
}

impl RichTextKey {
    fn new(rich: &NormalizedRichText) -> Self {
        Self {
            base: rich.base.key(),
            spans: rich
                .spans
                .iter()
                .map(|(range, style)| (range.clone(), style.key()))
                .collect(),
            colors: rich
                .colors
                .iter()
                .map(|(range, color)| (range.clone(), color.components.map(f32::to_bits)))
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
struct NormalizedRichText {
    base: TextStyle,
    spans: Vec<(Range<usize>, TextStyle)>,
    colors: Vec<(Range<usize>, Color)>,
}

#[derive(Debug)]
struct TextLayoutInner {
    runs: Vec<TextRunData>,
    run_metrics: Vec<RunMetrics>,
    lines: Vec<LineMetrics>,
    metrics: TextMetrics,
    is_rtl: bool,
}

impl TextLayoutInner {
    fn empty(style: &TextStyle) -> Self {
        Self {
            runs: Vec::new(),
            run_metrics: Vec::new(),
            lines: Vec::new(),
            metrics: TextMetrics {
                box_width: style.wrap_width.unwrap_or(0.0),
                ..TextMetrics::default()
            },
            is_rtl: false,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TextRunData {
    pub(crate) font: FontData,
    pub(crate) font_size: f32,
    pub(crate) normalized_coords: Vec<i16>,
    pub(crate) embolden: Option<f32>,
    pub(crate) skew: Option<f32>,
    pub(crate) color: Option<Color>,
    pub(crate) glyphs: Vec<TextGlyph>,
    metrics: ParleyRunMetrics,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TextGlyph {
    pub(crate) id: u32,
    pub(crate) x: f32,
    pub(crate) y: f32,
}

struct TextEngine {
    fcx: FontContext,
    lcx: LayoutContext<Option<Color>>,
    registered: Vec<(u64, u32)>,
}

impl TextEngine {
    fn new() -> Self {
        Self {
            fcx: FontContext::new(),
            lcx: LayoutContext::new(),
            registered: Vec::new(),
        }
    }

    fn register(&mut self, font: &FontInner) {
        let Some(data) = &font.data else {
            return;
        };

        let key = (data.data.id(), data.index);
        if self.registered.contains(&key) {
            return;
        }

        let override_info =
            font.registration_override
                .as_ref()
                .map(|family_name| FontInfoOverride {
                    family_name: Some(family_name.as_str()),
                    ..FontInfoOverride::default()
                });
        self.fcx
            .collection
            .register_fonts(data.data.clone(), override_info);
        self.registered.push(key);
    }
}

thread_local! {
    static ENGINE: RefCell<TextEngine> = RefCell::new(TextEngine::new());
}

fn build_simple_layout(font: &FontInner, text: &str, style: &TextStyle) -> TextLayoutInner {
    if text.is_empty() || style.size <= 0.0 || !style.size.is_finite() {
        return TextLayoutInner::empty(style);
    }

    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        engine.register(font);
        let family = font.family_for(style.family.as_ref());

        let TextEngine { fcx, lcx, .. } = &mut *engine;
        let mut builder = lcx.ranged_builder(fcx, text, 1.0, false);
        push_base_style(&mut builder, style, family);
        let mut layout = builder.build(text);
        finish_layout(&mut layout, style);
        flatten_layout(&layout, style)
    })
}

fn build_rich_layout(font: &FontInner, text: &str, rich: &NormalizedRichText) -> TextLayoutInner {
    if text.is_empty() || rich.base.size <= 0.0 || !rich.base.size.is_finite() {
        return TextLayoutInner::empty(&rich.base);
    }

    ENGINE.with(|engine| {
        let mut engine = engine.borrow_mut();
        engine.register(font);
        let base_family = font.family_for(rich.base.family.as_ref());

        let TextEngine { fcx, lcx, .. } = &mut *engine;
        let mut builder = lcx.ranged_builder(fcx, text, 1.0, false);
        push_base_style(&mut builder, &rich.base, base_family);

        for (range, style) in &rich.spans {
            let family = style
                .family
                .as_ref()
                .map(|family| font.family_for(Some(family)));
            push_span_style(&mut builder, style, family, range.clone());
        }

        for (range, color) in &rich.colors {
            builder.push(StyleProperty::Brush(Some(*color)), range.clone());
        }

        let mut layout = builder.build(text);
        finish_layout(&mut layout, &rich.base);
        flatten_layout(&layout, &rich.base)
    })
}

fn push_base_style(
    builder: &mut RangedBuilder<'_, Option<Color>>,
    style: &TextStyle,
    family: FontFamily<'static>,
) {
    builder.push_default(family);
    builder.push_default(StyleProperty::FontSize(style.size as f32));
    builder.push_default(StyleProperty::FontWeight(style.weight));
    builder.push_default(StyleProperty::FontStyle(style.font_style));
    builder.push_default(StyleProperty::FontWidth(style.font_width));
    builder.push_default(StyleProperty::LineHeight(parley_line_height(style)));
    builder.push_default(StyleProperty::LetterSpacing(style.letter_spacing as f32));
    builder.push_default(StyleProperty::WordSpacing(style.word_spacing as f32));
    if !style.features.is_empty() {
        builder.push_default(StyleProperty::FontFeatures(FontFeatures::List(
            Cow::Borrowed(&style.features),
        )));
    }
    if !style.variations.is_empty() {
        builder.push_default(StyleProperty::FontVariations(FontVariations::List(
            Cow::Borrowed(&style.variations),
        )));
    }
    builder.push_default(StyleProperty::Brush(None));
}

fn push_span_style(
    builder: &mut RangedBuilder<'_, Option<Color>>,
    style: &TextStyle,
    family: Option<FontFamily<'static>>,
    range: Range<usize>,
) {
    if let Some(family) = family {
        builder.push(family, range.clone());
    }
    builder.push(StyleProperty::FontSize(style.size as f32), range.clone());
    builder.push(StyleProperty::FontWeight(style.weight), range.clone());
    builder.push(StyleProperty::FontStyle(style.font_style), range.clone());
    builder.push(StyleProperty::FontWidth(style.font_width), range.clone());
    builder.push(
        StyleProperty::LineHeight(parley_line_height(style)),
        range.clone(),
    );
    builder.push(
        StyleProperty::LetterSpacing(style.letter_spacing as f32),
        range.clone(),
    );
    builder.push(
        StyleProperty::WordSpacing(style.word_spacing as f32),
        range.clone(),
    );
    if !style.features.is_empty() {
        builder.push(
            StyleProperty::FontFeatures(FontFeatures::List(Cow::Borrowed(&style.features))),
            range.clone(),
        );
    }
    if !style.variations.is_empty() {
        builder.push(
            StyleProperty::FontVariations(FontVariations::List(Cow::Borrowed(&style.variations))),
            range,
        );
    }
}

fn finish_layout(layout: &mut Layout<Option<Color>>, style: &TextStyle) {
    layout.break_all_lines(style.wrap_width.map(|width| width as f32));
    layout.align(parley_alignment(style.align), AlignmentOptions::default());
}

fn flatten_layout(layout: &Layout<Option<Color>>, style: &TextStyle) -> TextLayoutInner {
    let box_width = style
        .wrap_width
        .unwrap_or_else(|| f64::from(layout.width())) as f32;
    let first_baseline = layout
        .get(0)
        .map(|line| line.metrics().baseline)
        .unwrap_or(0.0);
    let origin_x = alignment_origin(style.align, layout.is_rtl(), box_width);
    let origin_y = -first_baseline;

    let mut runs = Vec::new();
    let mut run_metrics = Vec::new();
    let mut lines = Vec::new();

    for line in layout.lines() {
        let metrics = line.metrics();
        lines.push(LineMetrics {
            ascent: f64::from(metrics.ascent),
            descent: f64::from(metrics.descent),
            line_gap: f64::from(metrics.leading),
            line_height: f64::from(metrics.line_height),
            baseline: f64::from(metrics.baseline),
            width: f64::from((metrics.advance - metrics.trailing_whitespace).max(0.0)),
            trailing_whitespace: f64::from(metrics.trailing_whitespace),
        });

        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let run = glyph_run.run();
            let metrics = *run.metrics();
            let synthesis = run.synthesis();

            let mut glyphs = Vec::with_capacity(run.len().max(1));
            for glyph in glyph_run.positioned_glyphs() {
                glyphs.push(TextGlyph {
                    id: glyph.id,
                    x: glyph.x + origin_x,
                    y: glyph.y + origin_y,
                });
            }
            if glyphs.is_empty() {
                continue;
            }

            runs.push(TextRunData {
                font: run.font().clone(),
                font_size: run.font_size(),
                normalized_coords: run.normalized_coords().to_vec(),
                embolden: synthesis
                    .embolden()
                    .then_some(run.font_size() * SYNTHETIC_BOLD_SCALE),
                skew: synthesis.skew(),
                color: glyph_run.style().brush,
                glyphs,
                metrics,
            });
            run_metrics.push(RunMetrics {
                font_size: f64::from(run.font_size()),
                ascent: f64::from(metrics.ascent),
                descent: f64::from(metrics.descent),
                line_gap: f64::from(metrics.leading),
                line_height: f64::from(metrics.line_height),
            });
        }
    }

    TextLayoutInner {
        runs,
        run_metrics,
        lines,
        metrics: TextMetrics {
            width: f64::from(layout.width()),
            full_width: f64::from(layout.full_width()),
            height: f64::from(layout.height()),
            line_count: layout.len(),
            box_width: f64::from(box_width),
        },
        is_rtl: layout.is_rtl(),
    }
}

fn parley_alignment(align: TextAlign) -> Alignment {
    match align {
        TextAlign::Start => Alignment::Start,
        TextAlign::End => Alignment::End,
        TextAlign::Left => Alignment::Left,
        TextAlign::Center => Alignment::Center,
        TextAlign::Right => Alignment::Right,
        TextAlign::Justify => Alignment::Justify,
    }
}

fn alignment_origin(align: TextAlign, is_rtl: bool, box_width: f32) -> f32 {
    match align {
        TextAlign::Start if is_rtl => -box_width,
        TextAlign::End if !is_rtl => -box_width,
        TextAlign::Left | TextAlign::Start | TextAlign::End | TextAlign::Justify => 0.0,
        TextAlign::Center => -box_width / 2.0,
        TextAlign::Right => -box_width,
    }
}

fn parley_line_height(style: &TextStyle) -> LineHeight {
    match style.line_height {
        LineHeightSpec::Factor(factor) => LineHeight::FontSizeRelative(factor as f32),
        LineHeightSpec::Absolute(height) => LineHeight::Absolute(height as f32),
    }
}

fn font_family_name(font: &FontRef<'_>) -> Option<String> {
    font.localized_strings(StringId::TYPOGRAPHIC_FAMILY_NAME)
        .english_or_first()
        .or_else(|| {
            font.localized_strings(StringId::FAMILY_NAME)
                .english_or_first()
        })
        .map(|name| name.to_string().trim().to_owned())
        .filter(|name| !name.is_empty())
}

fn parse_family_spec(family: &str) -> FamilySpec {
    let trimmed = family.trim();
    match GenericFamily::parse(trimmed) {
        Some(generic) => FamilySpec::Generic(generic),
        None => FamilySpec::Named(trimmed.to_owned()),
    }
}

fn family_spec_to_name(family: &FamilySpec) -> FontFamilyName<'static> {
    match family {
        FamilySpec::Named(name) => FontFamilyName::Named(Cow::Owned(name.clone())),
        FamilySpec::Generic(generic) => FontFamilyName::Generic(*generic),
    }
}

fn raw_range(range: impl RangeBounds<usize>) -> Range<usize> {
    let start = match range.start_bound() {
        Bound::Included(start) => *start,
        Bound::Excluded(start) => start.saturating_add(1),
        Bound::Unbounded => 0,
    };
    let end = match range.end_bound() {
        Bound::Included(end) => end.saturating_add(1),
        Bound::Excluded(end) => *end,
        Bound::Unbounded => usize::MAX,
    };
    start..end
}

fn normalize_range(text: &str, range: &Range<usize>) -> Option<Range<usize>> {
    let start = floor_char_boundary(text, range.start.min(text.len()));
    let end = floor_char_boundary(text, range.end.min(text.len()));
    (start < end).then_some(start..end)
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn normalize_size(size: f64) -> f64 {
    if size.is_finite() && size > 0.0 {
        size
    } else {
        0.0
    }
}

fn normalize_spacing(spacing: f64) -> f64 {
    if spacing.is_finite() { spacing } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::colors;

    const INTER: &[u8] = include_bytes!("../assets/Inter-Regular.ttf");

    fn font() -> Font {
        Font::from_bytes(INTER).unwrap()
    }

    fn width_f32(layout: &TextLayout) -> f32 {
        layout.metrics().width as f32
    }

    #[test]
    fn family_and_metrics_are_available() {
        let font = font();
        assert_eq!(font.family_name(), "Inter");

        let metrics = font.metrics(&TextStyle::new(32.0));
        assert!(metrics.ascent > 0.0);
        assert!(metrics.descent > 0.0);
        assert!(metrics.line_height > 0.0);
        assert!(metrics.cap_height.is_some());
        assert!(metrics.x_height.is_some());
    }

    #[test]
    fn default_style_keeps_1_2_line_height() {
        let font = font();
        let metrics = font.layout("Hello\nworld", &TextStyle::new(20.0)).metrics();

        assert_eq!(metrics.line_count, 2);
        assert!((metrics.height - 48.0).abs() < 0.01);
        assert!(metrics.width > 0.0);
    }

    #[test]
    fn absolute_line_height_controls_baselines() {
        let font = font();
        let layout = font.layout("a\nb", &TextStyle::new(20.0).line_height(40.0));
        let lines = layout.line_metrics();

        assert_eq!(lines.len(), 2);
        assert!((lines[1].baseline - lines[0].baseline - 40.0).abs() < 0.01);
    }

    #[test]
    fn wrapping_splits_long_text() {
        let font = font();
        let style = TextStyle::new(20.0).wrap_width(120.0);
        let layout = font.layout("Lorem ipsum dolor sit amet, consectetur", &style);

        assert!(layout.metrics().line_count > 1);
        for line in layout.line_metrics() {
            assert!(line.width <= 120.5, "line too wide: {line:?}");
        }
    }

    #[test]
    fn letter_spacing_increases_width() {
        let font = font();
        let base = font.layout("Hello", &TextStyle::new(32.0)).metrics().width;
        let spaced = font
            .layout("Hello", &TextStyle::new(32.0).letter_spacing(4.0))
            .metrics()
            .width;

        assert!(spaced > base);
    }

    #[test]
    fn alignment_places_lines_around_the_anchor() {
        let font = font();
        let style = TextStyle::new(32.0);
        let left = font.layout("Hello", &style);
        let center = font.layout("Hello", &style.clone().align(TextAlign::Center));
        let right = font.layout("Hello", &style.align(TextAlign::Right));
        let width = width_f32(&center);

        let first_x = |layout: &TextLayout| layout.inner.runs[0].glyphs[0].x;
        assert!(first_x(&left).abs() < 0.01);
        assert!((first_x(&center) + width / 2.0).abs() < 0.1);
        assert!((first_x(&right) + width).abs() < 0.1);
    }

    #[test]
    fn rich_text_splits_runs_and_keeps_colors() {
        let font = font();
        let rich = RichText::new(TextStyle::new(16.0))
            .span_with(0..5, |style| style.size(32.0))
            .color(0..5, colors::CRIMSON);
        let layout = font.layout_rich("Hello world", &rich);

        assert!(layout.inner.runs.len() >= 2);
        assert!(
            layout
                .inner
                .runs
                .iter()
                .any(|run| run.font_size == 32.0 && run.color == Some(colors::CRIMSON))
        );
        assert!(
            layout
                .inner
                .runs
                .iter()
                .any(|run| run.font_size == 16.0 && run.color.is_none())
        );
    }

    #[test]
    fn kerning_can_be_disabled() {
        let font = font();
        let kerned = font.layout("AV", &TextStyle::new(64.0)).metrics().width;
        let unkerned = font
            .layout("AV", &TextStyle::new(64.0).kerning(false))
            .metrics()
            .width;

        assert!(unkerned > kerned, "kerned={kerned} unkerned={unkerned}");
    }

    #[test]
    fn cache_reuses_the_same_layout() {
        let font = font();
        let style = TextStyle::new(24.0);
        let first = font.layout("Hello", &style);
        let second = font.layout("Hello", &style);
        let different = font.layout("Hello", &style.line_height(30.0));

        assert!(Arc::ptr_eq(&first.inner, &second.inner));
        assert!(!Arc::ptr_eq(&first.inner, &different.inner));
    }

    #[test]
    fn empty_text_has_zero_metrics() {
        let font = font();
        let layout = font.layout("", &TextStyle::new(24.0));

        assert!(layout.is_empty());
        assert_eq!(layout.metrics().line_count, 0);
        assert_eq!(layout.size(), Size::new(0.0, 0.0));
    }

    #[test]
    fn rich_text_ranges_are_clamped_to_char_boundaries() {
        let font = font();
        let rich = RichText::new(TextStyle::new(16.0)).span_with(1..3, |style| style.size(24.0));
        let layout = font.layout_rich("你好", &rich);

        assert!(!layout.line_metrics().is_empty());
    }

    #[test]
    fn overlapping_rich_spans_use_the_later_style() {
        let font = font();
        let rich = RichText::new(TextStyle::new(16.0))
            .span_with(0..5, |style| style.size(20.0))
            .span_with(0..5, |style| style.size(30.0));
        let layout = font.layout_rich("Hello", &rich);

        assert!(layout.inner.runs.iter().all(|run| run.font_size == 30.0));
    }

    #[test]
    fn synthetic_weight_and_italic_are_layoutable() {
        let font = font();
        let style = TextStyle::new(32.0)
            .weight(FontWeight::BOLD)
            .font_style(FontStyle::Italic);
        let layout = font.layout("Bold italic", &style);

        assert!(!layout.is_empty());
        assert!(layout.inner.runs.iter().any(|run| run.embolden.is_some()));
        assert!(layout.inner.runs.iter().any(|run| run.skew.is_some()));
    }

    #[test]
    fn ligature_feature_produces_a_distinct_layout() {
        let font = font();
        let style = TextStyle::new(64.0);
        let with_ligatures = font.layout("fi", &style);
        let without_ligatures = font.layout("fi", &style.ligatures(false));

        assert!(!without_ligatures.is_empty());
        assert!(!Arc::ptr_eq(
            &with_ligatures.inner,
            &without_ligatures.inner
        ));
    }

    #[test]
    fn text_types_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<Font>();
        assert_send_sync::<TextStyle>();
        assert_send_sync::<RichText>();
        assert_send_sync::<TextLayout>();
    }

    #[test]
    fn system_and_generic_families_are_layoutable() {
        let generic = Font::from_generic(GenericFamily::SansSerif);
        assert!(
            !generic
                .layout("Generic family", &TextStyle::new(24.0))
                .is_empty()
        );

        let stack = font().with_fallbacks(["sans-serif"]);
        assert!(!stack.layout("Hello 世界", &TextStyle::new(24.0)).is_empty());
    }

    #[test]
    fn justify_is_supported() {
        let font = font();
        let style = TextStyle::new(20.0)
            .wrap_width(180.0)
            .align(TextAlign::Justify);
        let layout = font.layout("one two three four five six", &style);

        assert!(layout.metrics().line_count > 1);
    }

    #[test]
    fn rich_text_can_override_the_family() {
        let font = font();
        let rich = RichText::new(TextStyle::new(16.0))
            .span_with(0..5, |style| style.generic_family(GenericFamily::SansSerif));
        let layout = font.layout_rich("Hello world", &rich);

        assert!(!layout.is_empty());
    }

    #[test]
    fn glyphs_are_relative_to_the_first_baseline() {
        let font = font();
        let layout = font.layout("a\nb", &TextStyle::new(20.0));
        let runs = &layout.inner.runs;

        assert!(runs[0].glyphs[0].y.abs() < 0.01);
        assert!((runs[1].glyphs[0].y - 24.0).abs() < 0.1);
    }

    #[test]
    fn later_rich_colors_win_over_overlapping_ranges() {
        let font = font();
        let rich = RichText::new(TextStyle::new(16.0))
            .color(0..5, colors::CRIMSON)
            .color(0..5, colors::ROYAL_BLUE);
        let layout = font.layout_rich("Hello", &rich);

        assert!(
            layout
                .inner
                .runs
                .iter()
                .all(|run| run.color == Some(colors::ROYAL_BLUE))
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn missing_cjk_glyphs_use_a_fallback_run() {
        let font = font();
        let layout = font.layout("Hello 世界", &TextStyle::new(24.0));

        assert!(
            layout.inner.runs.len() >= 2,
            "expected a Latin run and a CJK fallback run"
        );
    }

    #[test]
    fn invalid_span_size_does_not_panic() {
        let font = font();
        let rich = RichText::new(TextStyle::new(16.0)).span(0..5, TextStyle::new(0.0));
        let layout = font.layout_rich("Hello world", &rich);

        assert!(layout.metrics().line_count > 0);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn cjk_text_wraps_by_character_opportunities() {
        let font = font();
        let style = TextStyle::new(20.0).wrap_width(100.0);
        let layout = font.layout("这是一段需要自动折行的中文文本", &style);

        assert!(layout.metrics().line_count > 1);
    }
}
