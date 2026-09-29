-- jump_back.lua — `cd -` for Trail.
--
--   g-   jump to the directory you were in before this one
--
-- Pressing it again jumps back, so two directories you are working between
-- are one keystroke apart. Unlike `u` (history back), it does not walk a
-- stack: it always toggles between the last two places.
--
-- Demonstrates: `on_enter_dir` keeping plugin state between events,
-- `trail.navigate`, an action that fails politely.

local current = nil
local previous = nil

trail.on_enter_dir(function(path)
    if path ~= current then
        previous = current
        current = path
    end
end)

trail.register_action("jump_back", function()
    if not previous then
        return false, "no previous directory yet"
    end
    trail.navigate(previous)
end)

trail.bind("g-", "jump_back")
