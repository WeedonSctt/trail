//! Terminal inline-image capability detection and protocol construction.
//!
//! Trail draws real pixel previews through [`ratatui_image`]. Which escape
//! sequences the terminal understands cannot be discovered portably, so this
//! module resolves the protocol from a user override first and from
//! environment variables second, and it keeps the resulting
//! [`Picker`] as process-wide state so every preview reuses one decision.
//!
//! # Why environment variables, and why a Halfblocks floor
//!
//! The reliable way to identify graphics support is to write a query escape
//! sequence to the tty and read the reply. That needs a raw, non-blocking read
//! on the same stdin the event loop later owns; on Windows it is not available
//! at all without dropping to the console API, which
//! `#![forbid(unsafe_code)]` rules out. Environment variables are therefore the
//! detection mechanism, and every terminal Trail cannot identify falls back to
//! [`ImageProtocol::Halfblocks`] — Unicode half-blocks with foreground and
//! background colour, which need no protocol support at all. The result is that
//! an image always renders as an image; the only question is fidelity.
//!
//! # Cell size
//!
//! Pixel protocols are told how big the image is *in pixels*, and the terminal
//! lays it out by dividing that by its own character cell size. Trail therefore
//! has to know the cell size to fit an image to the preview pane, and the
//! answer is only sometimes available: [`measure_cell_size`] gets it exactly
//! where the platform reports it, but `crossterm::terminal::window_size` is
//! explicitly unimplemented on Windows and `ratatui-image`'s own probes are
//! `#[cfg(unix)]`, so on Windows there is nothing to ask.
//!
//! When it cannot be measured, [`DEFAULT_CELL_SIZE`] is assumed, and the
//! assumption is deliberately biased *small*, because the two ways of being
//! wrong are not equally bad. An image is encoded at
//! `pane_cells × assumed_cell` pixels and the terminal draws it across
//! `encoded_pixels / real_cell` cells, so:
//!
//! - Assuming cells are **larger** than they are makes the image cover *more*
//!   cells than the pane reserved. Trail paints the surrounding UI over the
//!   overflow, and terminals drop the entire image rather than the covered
//!   part of it — the preview disappears.
//! - Assuming cells are **smaller** than they are makes the image cover *fewer*
//!   cells than the pane. It draws correctly, just with a margin around it.
//!
//! A missing preview is far worse than a small one, so the fallback
//! underestimates. `[preview] image_cell_width` / `image_cell_height` override
//! it for anyone who wants the pane filled exactly, and the preview caption
//! prints the values in use so they can be tuned by eye.

use std::env;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use image::DynamicImage;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::StatefulProtocol;

/// Character cell size, in pixels, assumed when the terminal will not report
/// its own.
///
/// Chosen to be no larger than the smallest cell a real terminal is likely to
/// have, so that an unmeasurable terminal errs towards a small image rather
/// than an invisible one. See the module documentation for why that direction
/// is the safe one.
pub const DEFAULT_CELL_SIZE: (u16, u16) = (7, 14);

/// The `[preview]` cell size that asks Trail to work the value out itself.
///
/// Zero is not a size, so it is free to carry this meaning; config validation
/// admits it for exactly that reason.
pub const AUTO_CELL_SIZE: (u16, u16) = (0, 0);

/// Longest edge, in pixels, an image is scaled down to before it is handed to
/// the render path.
///
/// Resizing and encoding happen on the UI thread every time the preview pane
/// changes size, so a 6000-pixel-wide photo would stall rendering. Downscaling
/// once here, on the worker thread, bounds that cost. The limit is well above
/// any realistic preview pane, so nothing visible is lost.
const MAX_SOURCE_EDGE: u32 = 1920;

// ── Protocol ──────────────────────────────────────────────────────────────────

/// The mechanism used to draw pixel previews in the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageProtocol {
    /// Kitty graphics protocol — Kitty, Ghostty, Konsole, WezTerm.
    Kitty,
    /// iTerm2 inline images — iTerm2, WezTerm, mintty, Tabby, Hyper, VS Code.
    Iterm2,
    /// Sixel graphics — mlterm, foot, xterm built with sixel support.
    Sixel,
    /// Unicode half-blocks. Needs no terminal support, so it is the floor
    /// rather than a failure case.
    Halfblocks,
    /// Image previews disabled; show metadata only.
    None,
}

impl ImageProtocol {
    /// The name used for this protocol in config files and in the preview
    /// caption.
    pub fn label(self) -> &'static str {
        match self {
            ImageProtocol::Kitty => "kitty",
            ImageProtocol::Iterm2 => "iterm2",
            ImageProtocol::Sixel => "sixel",
            ImageProtocol::Halfblocks => "halfblocks",
            ImageProtocol::None => "none",
        }
    }

    /// Parses a `[preview] image_protocol` value.
    ///
    /// Returns `None` for an unrecognised name so the config layer can report
    /// it. `"auto"` is not handled here — it is the absence of an override.
    pub fn parse(value: &str) -> Option<ImageProtocol> {
        match value.trim().to_ascii_lowercase().as_str() {
            "kitty" => Some(ImageProtocol::Kitty),
            "iterm2" | "iterm" => Some(ImageProtocol::Iterm2),
            "sixel" => Some(ImageProtocol::Sixel),
            "halfblocks" | "halfblock" => Some(ImageProtocol::Halfblocks),
            "none" | "off" => Some(ImageProtocol::None),
            _ => None,
        }
    }

    /// The `ratatui-image` protocol backing this variant, if it draws pixels.
    fn picker_type(self) -> Option<ProtocolType> {
        match self {
            ImageProtocol::Kitty => Some(ProtocolType::Kitty),
            ImageProtocol::Iterm2 => Some(ProtocolType::Iterm2),
            ImageProtocol::Sixel => Some(ProtocolType::Sixel),
            ImageProtocol::Halfblocks => Some(ProtocolType::Halfblocks),
            ImageProtocol::None => None,
        }
    }
}

// ── Detection ─────────────────────────────────────────────────────────────────

/// Identifies the host terminal's best inline-image protocol from the
/// environment.
///
/// Kitty is probed before iTerm2 because terminals that speak both (WezTerm,
/// Konsole) advertise themselves through the kitty variables only when they are
/// actually kitty-compatible, while `TERM_PROGRAM` is set unconditionally.
/// Never returns [`ImageProtocol::None`]: an unrecognised terminal gets
/// [`ImageProtocol::Halfblocks`], which always works.
pub fn detect_from_env() -> ImageProtocol {
    let term = env::var("TERM").unwrap_or_default().to_ascii_lowercase();
    let term_program = env::var("TERM_PROGRAM")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let lc_terminal = env::var("LC_TERMINAL")
        .unwrap_or_default()
        .to_ascii_lowercase();

    if is_set("KITTY_WINDOW_ID") || term.contains("kitty") {
        return ImageProtocol::Kitty;
    }

    if is_set("ITERM_SESSION_ID") || term_program.contains("iterm") || lc_terminal.contains("iterm")
    {
        return ImageProtocol::Iterm2;
    }

    // WezTerm speaks both the kitty and the iTerm2 protocols. It is identified
    // by WEZTERM_EXECUTABLE (set on every platform, including Windows) and by
    // TERM_PROGRAM=WezTerm, while TERM stays the generic xterm-256color — which
    // is why a TERM-only probe misses it entirely.
    if is_set("WEZTERM_EXECUTABLE") || term_program.contains("wezterm") {
        return ImageProtocol::Iterm2;
    }

    // Terminals that implement the iTerm2 sequence without a dedicated marker.
    if ["mintty", "vscode", "tabby", "hyper", "rio", "warpterminal"]
        .iter()
        .any(|name| term_program.contains(name))
    {
        return ImageProtocol::Iterm2;
    }

    if term.contains("sixel") || term == "mlterm" || term == "yaft-256color" || term == "foot" {
        return ImageProtocol::Sixel;
    }

    ImageProtocol::Halfblocks
}

/// Whether `name` is present in the environment with a non-empty value.
fn is_set(name: &str) -> bool {
    env::var_os(name).is_some_and(|value| !value.is_empty())
}

/// Asks the platform for the terminal's character cell size, in pixels.
///
/// Returns `None` whenever the answer is missing or unusable rather than
/// guessing from it: the call is unimplemented on Windows, and on Unix the
/// pixel fields of `TIOCGWINSZ` are documented as "unused" and are commonly
/// reported as zero. A `None` here is normal, not an error — the caller falls
/// back to [`DEFAULT_CELL_SIZE`].
pub fn measure_cell_size() -> Option<(u16, u16)> {
    let size = crossterm::terminal::window_size().ok()?;
    if size.columns == 0 || size.rows == 0 {
        return None;
    }
    let cell = (size.width / size.columns, size.height / size.rows);
    (cell.0 > 0 && cell.1 > 0).then_some(cell)
}

/// Resolves the cell size to assume from the `[preview]` configuration.
///
/// Zero means "work it out": measure the terminal if it will say, and fall back
/// to [`DEFAULT_CELL_SIZE`] if it will not. Each axis is resolved on its own so
/// that pinning one — the common case, since terminal cells vary far more in
/// height than in width — does not discard the automatic value for the other.
fn resolve_cell_size(configured: (u16, u16)) -> (u16, u16) {
    resolve_cell_size_with(configured, measure_cell_size())
}

/// The body of [`resolve_cell_size`], with the measurement passed in so that
/// tests can exercise both the measured and the unmeasurable terminal without
/// depending on whichever one happens to be running them.
fn resolve_cell_size_with(configured: (u16, u16), measured: Option<(u16, u16)>) -> (u16, u16) {
    let (auto_width, auto_height) = measured.unwrap_or(DEFAULT_CELL_SIZE);
    (
        if configured.0 == 0 {
            auto_width
        } else {
            configured.0
        },
        if configured.1 == 0 {
            auto_height
        } else {
            configured.1
        },
    )
}

/// Whether this process is running inside tmux, which requires graphics escape
/// sequences to be wrapped in a passthrough envelope.
fn in_tmux() -> bool {
    is_set("TMUX")
        || env::var("TERM").unwrap_or_default().starts_with("tmux")
        || env::var("TERM_PROGRAM").unwrap_or_default() == "tmux"
}

// ── Process-wide state ────────────────────────────────────────────────────────

/// The resolved protocol plus the picker that builds images for it.
struct Graphics {
    /// Protocol in effect for new previews.
    protocol: ImageProtocol,
    /// Cell size the picker was built with, shown in the preview caption.
    cell_size: (u16, u16),
    /// `None` when `protocol` is [`ImageProtocol::None`].
    picker: Option<Picker>,
}

impl Graphics {
    /// Resolves `preference` (a `[preview] image_protocol` value) against the
    /// environment and builds the matching picker.
    ///
    /// `cell_size` is the configured `image_cell_width`/`image_cell_height`
    /// pair, where zero in either axis asks for automatic resolution.
    fn new(preference: &str, cell_size: (u16, u16)) -> Graphics {
        let cell_size = resolve_cell_size(cell_size);
        let protocol = if preference.trim().eq_ignore_ascii_case("auto") {
            detect_from_env()
        } else {
            // An unparseable value is rejected by config validation before it
            // reaches here; treating it as "auto" keeps this total.
            ImageProtocol::parse(preference).unwrap_or_else(detect_from_env)
        };

        let picker = protocol.picker_type().map(|protocol_type| {
            let mut picker = Picker::new(cell_size);
            picker.protocol_type = protocol_type;
            picker.is_tmux = in_tmux();
            picker
        });

        tracing::info!(
            protocol = protocol.label(),
            cell_width = cell_size.0,
            cell_height = cell_size.1,
            preference,
            "resolved inline image protocol"
        );

        Graphics {
            protocol,
            cell_size,
            picker,
        }
    }
}

/// Process-wide graphics state, initialised on first use.
static GRAPHICS: OnceLock<Mutex<Graphics>> = OnceLock::new();

/// Returns the shared state, detecting from the environment if
/// [`configure`] has not run.
///
/// A poisoned lock is recovered rather than propagated: the guarded value is a
/// plain struct with no invariant a panic could have broken, and refusing to
/// render previews afterwards would be a worse outcome than continuing.
fn graphics() -> MutexGuard<'static, Graphics> {
    GRAPHICS
        .get_or_init(|| Mutex::new(Graphics::new("auto", AUTO_CELL_SIZE)))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Applies the `[preview]` configuration, replacing any earlier decision.
///
/// Called at startup once the config is loaded, and again after a runtime
/// `:set preview.…`. Previews already on screen keep the protocol they were
/// built with; the change takes effect on the next selection.
pub fn configure(preference: &str, cell_size: (u16, u16)) {
    let next = Graphics::new(preference, cell_size);

    // Seed the cell directly rather than going through `graphics()`: on the
    // first call that helper would build and log a whole environment-only
    // default just to have it overwritten a line later. If another thread won
    // the race to initialise, fall back to overwriting what it stored.
    if let Err(next) = GRAPHICS.set(Mutex::new(next)) {
        *graphics() = next.into_inner().unwrap_or_else(PoisonError::into_inner);
    }
}

/// The protocol in effect for new previews.
pub fn active() -> ImageProtocol {
    graphics().protocol
}

/// The character cell size, in pixels, currently assumed.
pub fn cell_size() -> (u16, u16) {
    graphics().cell_size
}

/// Builds a renderable protocol state for `image`.
///
/// Returns `None` when image previews are disabled
/// ([`ImageProtocol::None`]), in which case the caller should fall back to a
/// metadata-only preview.
pub fn build(image: DynamicImage) -> Option<Box<dyn StatefulProtocol>> {
    let image = downscale(image);
    let mut state = graphics();
    let picker = state.picker.as_mut()?;
    Some(picker.new_resize_protocol(image))
}

/// Scales `image` down so its longest edge is at most [`MAX_SOURCE_EDGE`].
///
/// Images already within the limit are returned untouched, so the common case
/// of a small icon or screenshot costs nothing.
fn downscale(image: DynamicImage) -> DynamicImage {
    let (width, height) = (image.width(), image.height());
    if width <= MAX_SOURCE_EDGE && height <= MAX_SOURCE_EDGE {
        return image;
    }
    image.resize(
        MAX_SOURCE_EDGE,
        MAX_SOURCE_EDGE,
        image::imageops::FilterType::Triangle,
    )
}

// ── Preview payload ───────────────────────────────────────────────────────────

/// A decoded image ready to be drawn into the preview pane.
///
/// Held in `PreviewContent::Image`. The protocol state is mutable because
/// `ratatui-image` re-encodes the image whenever the pane it is drawn into
/// changes size, caching the result until the next resize.
#[derive(Clone)]
pub struct ImagePreview {
    /// Encoder state for the active protocol.
    pub protocol: Box<dyn StatefulProtocol>,
    /// One-line summary drawn beneath the image (format, dimensions, size and
    /// the protocol in use).
    pub caption: String,
}

impl std::fmt::Debug for ImagePreview {
    /// `StatefulProtocol` is a trait object with no `Debug` bound, so the
    /// encoder state is summarised rather than printed.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImagePreview")
            .field("protocol", &"<encoder>")
            .field("caption", &self.caption)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_labels_round_trip_through_parse() {
        for protocol in [
            ImageProtocol::Kitty,
            ImageProtocol::Iterm2,
            ImageProtocol::Sixel,
            ImageProtocol::Halfblocks,
            ImageProtocol::None,
        ] {
            assert_eq!(ImageProtocol::parse(protocol.label()), Some(protocol));
        }
    }

    #[test]
    fn parse_rejects_unknown_names() {
        assert_eq!(ImageProtocol::parse("sixels"), None);
        assert_eq!(ImageProtocol::parse(""), None);
        // "auto" is the absence of an override, not a protocol.
        assert_eq!(ImageProtocol::parse("auto"), None);
    }

    #[test]
    fn parse_is_case_and_whitespace_insensitive() {
        assert_eq!(ImageProtocol::parse("  KiTTy "), Some(ImageProtocol::Kitty));
    }

    #[test]
    fn detection_never_reports_no_support() {
        // Whatever terminal the tests run under — including none at all, as on
        // CI — detection must land on a protocol that can draw something.
        assert_ne!(detect_from_env(), ImageProtocol::None);
    }

    #[test]
    fn downscale_leaves_small_images_untouched() {
        let image = DynamicImage::new_rgb8(64, 32);
        let out = downscale(image);
        assert_eq!((out.width(), out.height()), (64, 32));
    }

    #[test]
    fn downscale_bounds_the_longest_edge_and_keeps_aspect() {
        let image = DynamicImage::new_rgb8(MAX_SOURCE_EDGE * 2, MAX_SOURCE_EDGE);
        let out = downscale(image);
        assert_eq!(out.width(), MAX_SOURCE_EDGE);
        assert_eq!(out.height(), MAX_SOURCE_EDGE / 2);
    }

    #[test]
    fn disabled_protocol_builds_no_encoder() {
        // Uses its own Graphics rather than the process-wide one so the test
        // cannot race other tests through the shared OnceLock.
        let state = Graphics::new("none", DEFAULT_CELL_SIZE);
        assert_eq!(state.protocol, ImageProtocol::None);
        assert!(state.picker.is_none());
    }

    #[test]
    fn unmeasurable_terminal_falls_back_to_the_default_cell_size() {
        assert_eq!(
            resolve_cell_size_with(AUTO_CELL_SIZE, None),
            DEFAULT_CELL_SIZE
        );
    }

    #[test]
    fn a_measured_cell_size_beats_the_default() {
        assert_eq!(
            resolve_cell_size_with(AUTO_CELL_SIZE, Some((9, 19))),
            (9, 19)
        );
    }

    #[test]
    fn configured_axes_win_and_are_resolved_independently() {
        // Pinning the width must not throw away a measured height.
        assert_eq!(resolve_cell_size_with((8, 0), Some((9, 19))), (8, 19));
        assert_eq!(resolve_cell_size_with((0, 16), Some((9, 19))), (9, 16));
        assert_eq!(resolve_cell_size_with((8, 16), Some((9, 19))), (8, 16));
        assert_eq!(resolve_cell_size_with((8, 16), None), (8, 16));
    }

    #[test]
    fn the_default_cell_size_errs_small() {
        // Guessing high is the failure that hides the image: the terminal
        // spreads the encoded pixels over more cells than the preview pane
        // reserved, Trail paints the UI over the overflow, and the image is
        // dropped whole. The default must therefore stay at or below the
        // smallest cell a real terminal is likely to have.
        const SMALLEST_REALISTIC_CELL: (u16, u16) = (7, 14);
        assert!(DEFAULT_CELL_SIZE.0 <= SMALLEST_REALISTIC_CELL.0);
        assert!(DEFAULT_CELL_SIZE.1 <= SMALLEST_REALISTIC_CELL.1);
    }

    #[test]
    fn explicit_preference_overrides_the_environment() {
        let state = Graphics::new("halfblocks", (7, 15));
        assert_eq!(state.protocol, ImageProtocol::Halfblocks);
        assert_eq!(state.cell_size, (7, 15));
        assert!(state.picker.is_some());
    }
}
