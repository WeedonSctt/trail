//! Preview system: `PreviewProvider` trait, registry, and built-in providers.
//!
//! The `PreviewProvider` trait is the extensibility mechanism for preview
//! types — new types (PDF, archive, etc.) implement the trait and register
//! at startup without changing the core loop.
//!
//! Phase 5 registers all four built-in providers: `DirectoryProvider`,
//! `ImageProvider`, `TextProvider`, and `BinaryProvider`.

pub mod binary;
pub mod directory;
pub mod image;
pub mod provider;
pub mod text;

use crate::preview::binary::BinaryProvider;
use crate::preview::directory::DirectoryProvider;
use crate::preview::image::ImageProvider;
use crate::preview::provider::PreviewRegistry;
use crate::preview::text::TextProvider;

/// Registers all built-in preview providers into `registry` in priority order.
///
/// Called once at startup in `main.rs`. Provider order determines dispatch
/// priority — the first provider whose `can_handle` returns `true` is used.
///
/// # Registration order (first-match wins)
///
/// 1. `DirectoryProvider` — catches all directory entries (fastest path).
/// 2. `ImageProvider` — catches image files before the binary provider.
/// 3. `TextProvider` — catches text files (uses `content_inspector`).
/// 4. `BinaryProvider` — catch-all for any remaining regular files.
pub fn register_defaults(registry: &mut PreviewRegistry) {
    // 1. Directories always match `EntryKind::Dir`.
    registry.register(Box::new(DirectoryProvider));
    // 2. Image files (checked by extension before content inspection).
    registry.register(Box::new(ImageProvider));
    // 3. Text files (detected by content-inspection of the first 8 KB).
    registry.register(Box::new(TextProvider));
    // 4. Binary catch-all: every regular file that isn't text or image.
    registry.register(Box::new(BinaryProvider));
}
