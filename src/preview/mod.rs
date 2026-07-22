//! Preview system: `PreviewProvider` trait, registry, and built-in providers.
//!
//! The `PreviewProvider` trait is the extensibility mechanism for preview
//! types — new types (PDF, archive, etc.) implement the trait and register
//! at startup without changing the core loop.
// TODO(phase-1): Define PreviewProvider trait and PreviewRegistry.
// TODO(phase-1): Implement directory and text preview providers.
// TODO(phase-5): Implement binary and image preview providers.

pub mod binary;
pub mod directory;
pub mod image;
pub mod provider;
pub mod text;
