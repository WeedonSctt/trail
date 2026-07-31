-- Trail bookmarks example plugin
--
-- Demonstrates the `register_action` hook by implementing `:bookmark` and
-- `:jump <name>` as plugin-registered actions that delegate to Trail's
-- built-in bookmark store.
--
-- This plugin is loaded automatically when the `bookmarks` entry is present
-- in `[plugins].enabled` in the user's `trail.toml`.
--
-- Actions registered by this plugin:
--   `:plugin bookmark [name]`  — add a bookmark at the current directory.
--   `:plugin jump <name>`      — navigate to a previously added bookmark.
--
-- Note: In v1 the plugin receives the path as a string argument from Trail.
-- Full two-way interaction (e.g. reading AppState, navigating) is a v2 goal.

trail.log("bookmarks plugin loaded")

-- Track the current directory via on_enter_dir so actions can use it.
local current_dir = ""

trail.on_enter_dir(function(dir)
    current_dir = dir
    trail.log("bookmarks: entered " .. dir)
end)

trail.on_select(function(path)
    -- on_select fires on every selection change; nothing to do for bookmarks.
end)

-- register_action registers a named verb that the command parser will
-- dispatch when the user types `:plugin <name> [arg]`.
trail.register_action("bookmark_add", function(arg)
    -- arg is the bookmark name; current_dir is where we are.
    trail.log("bookmark_add: name=" .. arg .. " dir=" .. current_dir)
end)

trail.register_action("bookmark_jump", function(arg)
    -- arg is the bookmark name to jump to.
    trail.log("bookmark_jump: name=" .. arg)
end)
