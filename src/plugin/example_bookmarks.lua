-- Trail bookmarks example plugin
--
-- A thin plugin over Trail's built-in bookmark store, loaded when "bookmarks"
-- is in `[plugins] enabled`. `:bookmark` and `:jump` already exist as
-- commands; what this adds is keys for them, and a way to see the list.
--
--   `:plugin bookmark_add [name]`  bookmark the current directory
--   `:plugin bookmark_jump <name>` go to a bookmark
--   `b`                            bookmark the current directory, named
--                                  after it
--
-- The names `bookmark_add` and `bookmark_jump` are kept from the first
-- version of this plugin, which only logged; they now do what they say.
--
-- Demonstrates: `trail.command` (reusing a Trail command rather than
-- reimplementing it), `trail.bind`, an action returning `false, reason`.

trail.register_action("bookmark_add", function(name)
    trail.command("bookmark " .. (name or ""))
end)

trail.register_action("bookmark_jump", function(name)
    if name == nil or name == "" then
        return false, "which bookmark? `:plugin bookmark_jump <name>`"
    end
    trail.command("jump " .. name)
end)

trail.bind("b", "bookmark_add")
