-- git_line.lua — the last commit that touched the selected file.
--
-- As you move through a repository, the status bar shows when the selected
-- file was last committed and the commit's subject:
--
--     3 days ago · fix the scroll margin
--
-- Running `git log` for every keystroke would stall the UI if it ran inline,
-- so it runs through `trail.spawn`, off the UI thread. By the time a result
-- comes back the user may have moved on, so the callback checks that the
-- selection is still the file it asked about before showing anything — the
-- pattern every `spawn` callback that reacts to the selection should follow.
--
-- Demonstrates: `on_select`, `trail.spawn`, a stale-result check,
-- `trail.set_status`.

local asked_for = nil

trail.on_select(function(path, entry)
    asked_for = path
    if entry.kind ~= "file" or not trail.cwd().git_branch then
        trail.set_status(nil)
        return
    end
    trail.spawn{
        cmd = { "git", "log", "-1", "--format=%cr · %s", "--", entry.name },
        cwd = trail.cwd().path,
        on_exit = function(result)
            -- Stale: the user has moved on since this was asked.
            if asked_for ~= path then
                return
            end
            local line = result.stdout:gsub("%s+$", "")
            if result.ok and line ~= "" then
                trail.set_status(line)
            else
                trail.set_status(nil)
            end
        end,
    }
end)
