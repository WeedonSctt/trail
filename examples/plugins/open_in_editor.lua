-- open_in_editor.lua — hand the selection, or the whole directory, to a GUI
-- editor.
--
--   gv   open the selected file or directory in the editor
--   gV   open the current directory as a project
--
-- `Enter` already opens a file in `[general] editor`, which takes over the
-- terminal. This is for the other kind of editor, the one with its own
-- window: it is started in the background with `trail.spawn`, so Trail keeps
-- running while it opens.
--
-- Change EDITOR to taste: "code", "zed", "subl", "idea".
--
-- Demonstrates: `trail.bind`, `trail.spawn` without a callback, an action
-- returning `false, reason` on failure.

local EDITOR = "code"

local function open(target)
    trail.spawn{
        -- A command *string*, so it runs through `[general] shell`. Editors'
        -- launchers are often scripts (`code.cmd` on Windows), which only a
        -- shell finds; an argv table runs the program directly and would not.
        cmd = EDITOR .. ' "' .. target .. '"',
        on_exit = function(result)
            if not result.ok then
                trail.error("open_in_editor: " .. EDITOR .. " failed: " .. result.stderr)
            end
        end,
    }
    return "opening " .. target .. " in " .. EDITOR
end

trail.register_action("open_selection_in_editor", function()
    local selected = trail.selection()
    if not selected then
        return false, "nothing selected"
    end
    return open(selected.path)
end)

trail.register_action("open_dir_in_editor", function()
    return open(trail.cwd().path)
end)

trail.bind("gv", "open_selection_in_editor")
trail.bind("gV", "open_dir_in_editor")
