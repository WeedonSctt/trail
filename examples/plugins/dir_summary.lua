-- dir_summary.lua — how much is in this directory?
--
-- `gs` (or `:plugin dir_summary`) counts the files and directories in the
-- listing you are looking at and adds up the files' sizes. Pure Lua over
-- `trail.entries()`, which is already in memory: no disk access, so it is
-- instant even in a large directory.
--
-- It respects what is on screen: hidden files count only while they are
-- shown, and during a search only the matches count.
--
-- Demonstrates: a registered action that reads the listing and *returns* its
-- message, which Trail shows as a notice.

local function human(bytes)
    local units = { "B", "KiB", "MiB", "GiB", "TiB" }
    local i = 1
    while bytes >= 1024 and i < #units do
        bytes = bytes / 1024
        i = i + 1
    end
    if i == 1 then
        return string.format("%d %s", bytes, units[i])
    end
    return string.format("%.1f %s", bytes, units[i])
end

local function plural(n, word)
    if n == 1 then
        return n .. " " .. word
    end
    if word:sub(-1) == "y" then
        return n .. " " .. word:sub(1, -2) .. "ies"
    end
    return n .. " " .. word .. "s"
end

trail.register_action("dir_summary", function()
    local files, dirs, bytes = 0, 0, 0
    for _, entry in ipairs(trail.entries()) do
        if entry.kind == "dir" then
            dirs = dirs + 1
        else
            files = files + 1
            bytes = bytes + (entry.size or 0)
        end
    end
    return plural(files, "file") .. ", " .. plural(dirs, "directory") .. ", " .. human(bytes)
end)

trail.bind("gs", "dir_summary")
