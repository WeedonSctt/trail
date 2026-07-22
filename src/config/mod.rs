//! Configuration loading and schema definitions.
//!
//! TOML config loaded once at startup, resolved by the input handler and
//! theme module. Strict-mode deserialization rejects unknown keys.
// TODO(phase-7): Implement schema.rs, TOML loading, and default.toml resolution.

pub mod schema;
