//! Preview system: `PreviewProvider` trait, registry, and built-in providers.
//!
//! The `PreviewProvider` trait is the extensibility mechanism for preview
//! types — new types (PDF, archive, etc.) implement the trait and register
//! at startup without changing the core loop.

pub mod binary;
pub mod directory;
pub mod image;
pub mod provider;
pub mod text;

use crate::preview::directory::DirectoryProvider;
use crate::preview::provider::PreviewRegistry;
use crate::preview::text::TextProvider;

/// Registers all built-in preview providers into `registry` in priority order.
///
/// Called once at startup in `main.rs`. Provider order matters: the first
/// provider whose `can_handle` returns `true` is used.
///
/// Phase 5 adds `BinaryProvider` and `ImageProvider` here.
pub fn register_defaults(registry: &mut PreviewRegistry) {
    // Directory entries are caught first.
    registry.register(Box::new(DirectoryProvider));
    // Text files (detected by content-inspection).
    registry.register(Box::new(TextProvider));
    // TODO(phase-5): register BinaryProvider and ImageProvider here.
}
