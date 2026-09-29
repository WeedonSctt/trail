-- json_preview.lua — pretty-printed JSON in the preview pane.
--
-- Minified JSON is one enormous line, which the built-in text preview can
-- only show the start of. This hands `.json` files to `jq`, which re-indents
-- them.
--
-- A previewer is declarative: Lua describes it once, at load time, and Trail
-- runs the command itself, off the UI thread, with a timeout. A slow command
-- shows "Loading…" and never freezes navigation. If `jq` is not installed the
-- pane says so instead of showing the file.
--
-- `{path}` is replaced by the selected file's path. Colour output (`jq -C`)
-- would work too — Trail strips the escape codes — but plain output is
-- cheaper.
--
-- Demonstrates: `trail.register_previewer`.

trail.register_previewer{
    name = "jq",
    extensions = { "json", "geojson" },
    names = { ".babelrc", ".eslintrc" },
    command = { "jq", ".", "{path}" },
    timeout_ms = 3000,
}
