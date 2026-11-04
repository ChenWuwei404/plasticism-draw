//! Selection of how canvas coordinates are scaled to physical pixels.

/// How canvas coordinates are mapped to physical pixels.
///
/// The default, [`DpiMode::Auto`], follows the window's scale factor and keeps
/// canvas coordinates in logical pixels, matching the operating system's DPI
/// setting. [`DpiMode::Fixed`] overrides that mapping with an explicit scale.
///
/// Set the mode on the application with [`App::dpi_mode`](crate::App::dpi_mode).
/// The window itself is always created with a logical size; the mode only
/// changes the canvas coordinate space and the scene scale.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum DpiMode {
    /// Follow the window's current scale factor (the operating system DPI
    /// setting).
    ///
    /// One canvas unit maps to `scale_factor` physical pixels, so canvas
    /// coordinates stay in logical pixels and `canvas.width()` equals the
    /// window's logical width.
    #[default]
    Auto,
    /// Use a fixed scale factor regardless of the monitor's DPI setting.
    ///
    /// One canvas unit maps to the provided number of physical pixels. For
    /// example, `Fixed(1.0)` maps one canvas unit to one physical pixel (a
    /// pixel-perfect canvas), while `Fixed(2.0)` always renders at 200%
    /// regardless of the monitor. Values that are not finite and positive are
    /// treated as `1.0`.
    ///
    /// Because the window keeps its logical size, the canvas size follows the
    /// window's physical size divided by this factor. On a 150% display,
    /// `Fixed(1.0)` in an 800x600 logical window yields a canvas of about
    /// 1200x900 units.
    Fixed(f64),
}

impl DpiMode {
    /// Returns the number of physical pixels per canvas unit.
    ///
    /// `window_scale_factor` is the window's current scale factor, for example
    /// from `Window::scale_factor()`. It is only used by [`DpiMode::Auto`].
    /// Invalid scale factors fall back to `1.0`.
    pub fn effective_scale(self, window_scale_factor: f64) -> f64 {
        match self {
            Self::Auto => normalize_scale(window_scale_factor),
            Self::Fixed(scale) => normalize_scale(scale),
        }
    }
}

pub(crate) fn normalize_scale(scale_factor: f64) -> f64 {
    if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::{DpiMode, normalize_scale};

    #[test]
    fn auto_follows_the_window_scale_factor() {
        assert_eq!(DpiMode::Auto.effective_scale(1.0), 1.0);
        assert_eq!(DpiMode::Auto.effective_scale(1.5), 1.5);
        assert_eq!(DpiMode::Auto.effective_scale(2.0), 2.0);
    }

    #[test]
    fn fixed_ignores_the_window_scale_factor() {
        assert_eq!(DpiMode::Fixed(1.0).effective_scale(2.0), 1.0);
        assert_eq!(DpiMode::Fixed(2.0).effective_scale(1.0), 2.0);
    }

    #[test]
    fn invalid_scale_factors_fall_back_to_one() {
        assert_eq!(DpiMode::Auto.effective_scale(0.0), 1.0);
        assert_eq!(DpiMode::Auto.effective_scale(f64::NAN), 1.0);
        assert_eq!(DpiMode::Fixed(f64::INFINITY).effective_scale(2.0), 1.0);
        assert_eq!(normalize_scale(-2.0), 1.0);
    }

    #[test]
    fn default_mode_is_auto() {
        assert_eq!(DpiMode::default(), DpiMode::Auto);
    }
}
