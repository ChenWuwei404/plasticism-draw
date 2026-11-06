//! Bitmap image loading and image layout helpers.

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use anyhow::{Context, Result, bail};
use vello_gpu::peniko::ImageAlphaType;
use vello_gpu::{ImageId, PixelMetadata, Pixmap};

use crate::kurbo::{Rect, Size};

/// Largest bitmap dimension accepted by the default Vello GPU image atlas.
const MAX_DIMENSION: u32 = 4096;

/// Images registered by [`Image`] that still need to be uploaded to the GPU.
static PENDING: Mutex<Vec<Weak<ImageInner>>> = Mutex::new(Vec::new());
/// Image atlas slots that can be released once the GPU is available again.
static DESTROYED: Mutex<Vec<ImageId>> = Mutex::new(Vec::new());

/// A decoded bitmap that can be drawn with
/// [`Canvas::draw_image`](crate::Canvas::draw_image) and
/// [`Canvas::draw_bitmap_icon`](crate::Canvas::draw_bitmap_icon).
///
/// The pixels are kept in memory. They are uploaded to the GPU automatically at
/// the start of a frame; an image created and drawn for the first time inside a
/// draw callback becomes visible on the next frame. Images with straight
/// (unpremultiplied) RGBA data are premultiplied by Vello when they are
/// uploaded.
///
/// Cloning an `Image` is cheap and shares the decoded pixels.
#[derive(Clone, Debug)]
pub struct Image {
    inner: Arc<ImageInner>,
}

#[derive(Debug)]
pub(crate) struct ImageInner {
    pub(crate) pixmap: Pixmap,
    pub(crate) id: OnceLock<ImageId>,
}

impl Drop for ImageInner {
    fn drop(&mut self) {
        if let Some(id) = self.id.get() {
            lock(&DESTROYED).push(*id);
        }
    }
}

impl Image {
    /// Loads and decodes an image file.
    ///
    /// The format is detected from the file contents, so PNG, JPEG, WebP, GIF,
    /// BMP, ICO, and TIFF files are supported by the enabled decoders.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)
            .with_context(|| format!("failed to read image {}", path.display()))?;
        Self::from_bytes(bytes)
            .with_context(|| format!("failed to decode image {}", path.display()))
    }

    /// Decodes an encoded image from memory.
    pub fn from_bytes(bytes: impl AsRef<[u8]>) -> Result<Self> {
        let image = image::load_from_memory(bytes.as_ref()).context("unsupported image data")?;
        Self::from_dynamic_image(image)
    }

    /// Creates an image from straight (unpremultiplied) RGBA8 pixels.
    ///
    /// `pixels` must contain exactly `width * height * 4` bytes in RGBA order.
    /// The alpha channel may be fully opaque or partially transparent.
    pub fn from_rgba8(width: u32, height: u32, pixels: impl Into<Vec<u8>>) -> Result<Self> {
        if width == 0 || height == 0 {
            bail!("image dimensions must be non-zero, got {width}x{height}");
        }
        if width > MAX_DIMENSION || height > MAX_DIMENSION {
            bail!(
                "image dimensions must not exceed {MAX_DIMENSION}x{MAX_DIMENSION}, got {width}x{height}"
            );
        }

        let pixels = pixels.into();
        let expected = width as usize * height as usize * 4;
        if pixels.len() != expected {
            bail!(
                "expected {expected} RGBA bytes for a {width}x{height} image, got {}",
                pixels.len()
            );
        }
        // `Pixmap::from_parts` casts the vector without copying and requires a
        // capacity divisible by the pixel size. Reallocate only when needed.
        let pixels = if pixels.capacity() % 4 == 0 {
            pixels
        } else {
            pixels.to_vec()
        };

        let pixmap = Pixmap::from_parts(
            pixels,
            width as u16,
            height as u16,
            PixelMetadata::new(ImageAlphaType::Alpha, true),
        );
        Ok(Self::from_pixmap(pixmap))
    }

    fn from_dynamic_image(image: image::DynamicImage) -> Result<Self> {
        let image = image.into_rgba8();
        let (width, height) = image.dimensions();
        Self::from_rgba8(width, height, image.into_raw())
    }

    fn from_pixmap(pixmap: Pixmap) -> Self {
        let inner = Arc::new(ImageInner {
            pixmap,
            id: OnceLock::new(),
        });
        register(&inner);
        Self { inner }
    }

    /// Returns the image width in pixels.
    pub fn width(&self) -> u32 {
        u32::from(self.inner.pixmap.width())
    }

    /// Returns the image height in pixels.
    pub fn height(&self) -> u32 {
        u32::from(self.inner.pixmap.height())
    }

    /// Returns the image size in pixels.
    pub fn size(&self) -> Size {
        Size::new(f64::from(self.width()), f64::from(self.height()))
    }

    /// Returns whether the image has (or may have) non-opaque pixels.
    pub fn may_have_transparency(&self) -> bool {
        self.inner.pixmap.may_have_transparency()
    }

    pub(crate) fn gpu_id(&self) -> Option<ImageId> {
        self.inner.id.get().copied()
    }

    /// Ensures the image is uploaded even if the image was created after the
    /// current frame's upload pass.
    pub(crate) fn request_upload(&self) {
        if self.inner.id.get().is_none() {
            register(&self.inner);
        }
    }
}

fn register(inner: &Arc<ImageInner>) {
    lock(&PENDING).push(Arc::downgrade(inner));
}

pub(crate) fn take_pending() -> Vec<Arc<ImageInner>> {
    let pending = std::mem::take(&mut *lock(&PENDING));
    pending
        .into_iter()
        .filter_map(|image| image.upgrade())
        .collect()
}

pub(crate) fn take_destroyed() -> Vec<ImageId> {
    std::mem::take(&mut *lock(&DESTROYED))
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// How a source rectangle is fitted into a destination rectangle.
///
/// All modes center the result. `Contain` and `Cover` preserve the source
/// aspect ratio.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageFit {
    /// Stretch the source to fill the destination exactly.
    #[default]
    Fill,
    /// Scale the source to fit inside the destination, leaving empty space.
    Contain,
    /// Scale the source to cover the destination, cropping the overflow.
    Cover,
}

/// Computes the destination rectangle for `source` inside `dest`.
pub(crate) fn fitted_rect(source: Size, dest: Rect, fit: ImageFit) -> Option<Rect> {
    let dest = dest.abs();
    if dest.is_zero_area() {
        return None;
    }
    if source.width <= 0.0 || source.height <= 0.0 {
        return None;
    }

    let dest_size = dest.size();
    let scale = match fit {
        ImageFit::Fill => return Some(dest),
        ImageFit::Contain => (dest_size.width / source.width).min(dest_size.height / source.height),
        ImageFit::Cover => (dest_size.width / source.width).max(dest_size.height / source.height),
    };

    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }

    let size = Size::new(source.width * scale, source.height * scale);
    Some(Rect::from_center_size(dest.center(), size))
}

#[cfg(test)]
mod tests {
    use super::{Image, ImageFit, fitted_rect};
    use crate::kurbo::{Rect, Size};

    #[test]
    fn fill_uses_the_whole_destination() {
        let dest = Rect::new(10.0, 20.0, 110.0, 70.0);
        assert_eq!(
            fitted_rect(Size::new(200.0, 100.0), dest, ImageFit::Fill),
            Some(dest)
        );
    }

    #[test]
    fn contain_preserves_aspect_ratio_and_centers() {
        let dest = Rect::new(0.0, 0.0, 200.0, 100.0);
        let fitted = fitted_rect(Size::new(100.0, 100.0), dest, ImageFit::Contain).unwrap();

        assert_eq!(fitted, Rect::new(50.0, 0.0, 150.0, 100.0));
    }

    #[test]
    fn cover_fills_the_destination_and_centers_overflow() {
        let dest = Rect::new(0.0, 0.0, 200.0, 100.0);
        let fitted = fitted_rect(Size::new(100.0, 100.0), dest, ImageFit::Cover).unwrap();

        assert_eq!(fitted, Rect::new(0.0, -50.0, 200.0, 150.0));
    }

    #[test]
    fn zero_sized_sources_and_destinations_are_ignored() {
        assert!(fitted_rect(Size::ZERO, Rect::new(0.0, 0.0, 10.0, 10.0), ImageFit::Fill).is_none());
        assert!(
            fitted_rect(
                Size::new(10.0, 10.0),
                Rect::new(0.0, 0.0, 0.0, 10.0),
                ImageFit::Fill
            )
            .is_none()
        );
    }

    #[test]
    fn images_validate_raw_pixel_size() {
        assert!(Image::from_rgba8(2, 2, vec![0; 16]).is_ok());
        assert!(Image::from_rgba8(2, 2, vec![0; 15]).is_err());
        assert!(Image::from_rgba8(0, 2, Vec::new()).is_err());
        assert!(Image::from_rgba8(4097, 1, vec![0; 4097 * 4]).is_err());
    }

    #[test]
    fn png_decoding_preserves_size_and_alpha() {
        let icon = Image::from_bytes(include_bytes!("../assets/star.png")).unwrap();
        assert_eq!(icon.width(), 128);
        assert_eq!(icon.height(), 128);
        assert!(icon.may_have_transparency());

        let photo = Image::from_bytes(include_bytes!("../assets/photo.png")).unwrap();
        assert_eq!(photo.width(), 320);
        assert_eq!(photo.height(), 200);
        assert!(!photo.may_have_transparency());
    }
}
