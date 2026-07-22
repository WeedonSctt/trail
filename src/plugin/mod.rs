//! Plugin system: Lua scripting surface via `mlua`.
//!
//! Provides hooks (`on_select`, `on_enter_dir`, `register_action`) for
//! user-defined commands and custom preview providers.
// TODO(phase-8): Implement Lua API surface and plugin loading.

pub mod lua_api;
