//! The plugin API end to end: a real `PluginEngine` inside a real `AppState`,
//! driven the way the event loop drives it — fire, then settle.
//!
//! Each test names the promise it protects from `docs/plugin_guide.md`. The engine's
//! internals have unit tests of their own; these are about what a plugin
//! author can rely on.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use tempfile::TempDir;
use tokio::sync::mpsc;

use trail::actions::{self, Action};
use trail::app::state::AppState;
use trail::input::command_parser::ParsedCommand;
use trail::plugin::{host, ActionOutcome, PluginEngine};
use trail::workers::{self, WorkerMsg};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// `<tmp>/{alpha.txt, beta.txt, gamma.md, sub/}`.
fn fixture() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("alpha.txt"), "a").unwrap();
    fs::write(dir.path().join("beta.txt"), "bb").unwrap();
    fs::write(dir.path().join("gamma.md"), "# g").unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    dir
}

/// A state over `dir` with one plugin, `test`, loaded from `source`.
fn with_plugin(dir: &Path, source: &str) -> AppState {
    let mut state = AppState::new(dir.to_owned()).unwrap();
    let mut engine = PluginEngine::new().unwrap();
    engine
        .load_plugin_str("test", source)
        .expect("plugin loads");
    state.plugin_engine = Some(engine);
    state
}

/// Applies everything queued, the way the event loop does after each event.
fn settle(state: &mut AppState, tx: &mpsc::Sender<WorkerMsg>) {
    host::settle(state, tx, &mut |_| {});
}

/// Runs `:plugin <name> <arg>` and settles, as a keystroke bound to it would.
fn run_action(state: &mut AppState, tx: &mpsc::Sender<WorkerMsg>, name: &str, arg: &str) {
    actions::apply(
        Action::ExecuteCommand(ParsedCommand::Plugin {
            name: name.to_owned(),
            arg: arg.to_owned(),
        }),
        state,
    )
    .unwrap();
    settle(state, tx);
}

fn notice(state: &AppState) -> String {
    state
        .notice
        .as_ref()
        .map(|n| n.text.clone())
        .unwrap_or_default()
}

fn fire_select(state: &AppState) {
    let entry = state.selected_entry().cloned().unwrap();
    state
        .plugin_engine
        .as_ref()
        .unwrap()
        .fire_on_select(state, &entry);
}

fn key(ch: char) -> KeyEvent {
    KeyEvent {
        code: KeyCode::Char(ch),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    }
}

// ── Writes ────────────────────────────────────────────────────────────────────

#[test]
fn an_action_can_navigate_and_report() {
    let dir = fixture();
    let (tx, _rx) = workers::channel();
    let mut state = with_plugin(
        dir.path(),
        r#"
trail.register_action("go", function(arg)
    trail.navigate(arg)
    trail.notify("went to " .. arg)
end)
"#,
    );
    run_action(&mut state, &tx, "go", "sub");
    assert!(state.cwd.ends_with("sub"), "cwd is {:?}", state.cwd);
    assert_eq!(notice(&state), "went to sub");
}

#[test]
fn select_move_sort_and_hidden_reach_the_listing() {
    let dir = fixture();
    fs::write(dir.path().join(".hidden"), "").unwrap();
    let (tx, _rx) = workers::channel();
    let mut state = with_plugin(
        dir.path(),
        r#"
trail.register_action("pick", function(name) trail.select(name) end)
trail.register_action("down", function() trail.move(1) end)
trail.register_action("by_size", function() trail.set_sort{ by = "size" } end)
trail.register_action("reveal", function() trail.set_hidden(true) end)
"#,
    );
    run_action(&mut state, &tx, "pick", "beta.txt");
    assert_eq!(state.selected_entry().unwrap().file_name, "beta.txt");
    run_action(&mut state, &tx, "down", "");
    assert_eq!(state.selected_entry().unwrap().file_name, "gamma.md");
    run_action(&mut state, &tx, "by_size", "");
    assert_eq!(state.sort.by, trail::app::sort::SortBy::Size);
    run_action(&mut state, &tx, "reveal", "");
    assert!(state.show_hidden);
}

#[test]
fn selecting_something_that_is_not_there_is_reported() {
    let dir = fixture();
    let (tx, _rx) = workers::channel();
    let mut state = with_plugin(
        dir.path(),
        r#"trail.register_action("pick", function(name) trail.select(name) end)"#,
    );
    run_action(&mut state, &tx, "pick", "nope.txt");
    assert!(
        notice(&state).contains("no 'nope.txt'"),
        "{}",
        notice(&state)
    );
}

#[test]
fn a_trail_command_goes_through_the_command_line() {
    let dir = fixture();
    let (tx, _rx) = workers::channel();
    let mut state = with_plugin(
        dir.path(),
        r#"trail.register_action("mk", function(name) trail.command(":mkdir " .. name) end)"#,
    );
    run_action(&mut state, &tx, "mk", "made-by-plugin");
    assert!(dir.path().join("made-by-plugin").is_dir());
}

#[test]
fn set_status_persists_until_cleared() {
    let dir = fixture();
    let (tx, _rx) = workers::channel();
    let mut state = with_plugin(
        dir.path(),
        r#"
trail.register_action("on", function() trail.set_status("building…") end)
trail.register_action("off", function() trail.set_status(nil) end)
"#,
    );
    run_action(&mut state, &tx, "on", "");
    assert_eq!(state.plugin_status.as_deref(), Some("building…"));
    state.clear_notice();
    assert_eq!(state.plugin_status.as_deref(), Some("building…"));
    run_action(&mut state, &tx, "off", "");
    assert_eq!(state.plugin_status, None);
}

#[test]
fn set_config_behaves_like_set() {
    let dir = fixture();
    let (tx, _rx) = workers::channel();
    let mut state = with_plugin(
        dir.path(),
        r#"trail.register_action("wide", function() trail.set_config("entry_details", "size") end)"#,
    );
    run_action(&mut state, &tx, "wide", "");
    assert_eq!(state.config.navigation.entry_details, "size");
}

#[test]
fn run_queues_a_terminal_command_with_the_requested_pause() {
    let dir = fixture();
    let (tx, _rx) = workers::channel();
    let mut state = with_plugin(
        dir.path(),
        r#"trail.register_action("r", function() trail.run({ "git", "status" }, { pause = "never" }) end)"#,
    );
    run_action(&mut state, &tx, "r", "");
    let Some(Action::RunExternal { argv, pause, .. }) = state.pending_external.clone() else {
        panic!("a RunExternal is pending");
    };
    assert_eq!(argv, vec!["git".to_owned(), "status".to_owned()]);
    assert_eq!(pause, actions::shell_exec::ShellPause::Never);
}

// ── Reads ─────────────────────────────────────────────────────────────────────

#[test]
fn an_action_reads_the_selection_the_listing_and_the_config() {
    let dir = fixture();
    let (tx, _rx) = workers::channel();
    let mut state = with_plugin(
        dir.path(),
        r#"
trail.register_action("describe", function()
    local s = trail.selection()
    local n = #trail.entries()
    return s.name .. "|" .. s.kind .. "|" .. n .. "|" .. trail.config("editor")
        .. "|" .. trail.cwd().sort.by .. "|" .. trail.mode() .. "|" .. trail.tabs().count
end)
"#,
    );
    run_action(&mut state, &tx, "describe", "");
    // Directories first: `sub/` is the first entry.
    assert_eq!(notice(&state), "sub|dir|4|nvim|name|navigation|1");
}

#[test]
fn a_file_entry_carries_its_size_and_a_directory_does_not() {
    let dir = fixture();
    let (tx, _rx) = workers::channel();
    let mut state = with_plugin(
        dir.path(),
        r#"
trail.on_select(function(path, e)
    trail.set_status(e.name .. "=" .. tostring(e.size))
end)
"#,
    );
    fire_select(&state);
    settle(&mut state, &tx);
    assert_eq!(state.plugin_status.as_deref(), Some("sub=nil"));
    state.move_down();
    fire_select(&state);
    settle(&mut state, &tx);
    assert_eq!(state.plugin_status.as_deref(), Some("alpha.txt=1"));
}

#[test]
fn hooks_still_receive_the_path_string_first() {
    // Every plugin written against the old API concatenates its argument.
    let dir = fixture();
    let (tx, _rx) = workers::channel();
    let mut state = with_plugin(
        dir.path(),
        r#"trail.on_select(function(path) trail.notify("at " .. path) end)"#,
    );
    fire_select(&state);
    settle(&mut state, &tx);
    assert!(notice(&state).starts_with("at "));
    assert!(notice(&state).ends_with("sub"));
}

#[test]
fn reads_are_refused_outside_a_call_with_a_clear_message() {
    let mut engine = PluginEngine::new().unwrap();
    let err = engine
        .load_plugin_str("early", "local s = trail.selection()")
        .unwrap_err();
    assert!(
        err.to_string().contains("only available inside a hook"),
        "{err}"
    );
}

// ── Outcomes and errors ───────────────────────────────────────────────────────

#[test]
fn an_action_reports_what_it_returns() {
    let dir = fixture();
    let (tx, _rx) = workers::channel();
    let mut state = with_plugin(
        dir.path(),
        r#"
trail.register_action("ok", function() return "all good" end)
trail.register_action("quiet", function() end)
trail.register_action("no", function() return false, "not today" end)
trail.register_action("boom", function() error("kaboom") end)
"#,
    );
    let engine = state.plugin_engine.as_ref().unwrap();
    assert_eq!(
        engine.fire_action(&state, "ok", ""),
        ActionOutcome::Done(Some("all good".to_owned()))
    );
    assert_eq!(
        engine.fire_action(&state, "quiet", ""),
        ActionOutcome::Done(None)
    );
    assert_eq!(
        engine.fire_action(&state, "no", ""),
        ActionOutcome::Failed("not today".to_owned())
    );
    let ActionOutcome::Failed(reason) = engine.fire_action(&state, "boom", "") else {
        panic!("an error is a failure");
    };
    assert!(reason.contains("kaboom"), "{reason}");

    run_action(&mut state, &tx, "no", "");
    assert_eq!(notice(&state), "plugin no: not today");
}

#[test]
fn a_hook_error_reaches_the_status_bar_attributed() {
    let dir = fixture();
    let (tx, _rx) = workers::channel();
    let mut state = with_plugin(
        dir.path(),
        r#"trail.on_select(function() error("hook broke") end)"#,
    );
    fire_select(&state);
    settle(&mut state, &tx);
    let text = notice(&state);
    assert!(text.starts_with("plugin test: on_select:"), "{text}");
    assert!(text.contains("hook broke"), "{text}");
}

#[test]
fn a_runaway_hook_is_stopped_by_the_budget() {
    let dir = fixture();
    let mut state = with_plugin(
        dir.path(),
        r#"trail.register_action("spin", function() while true do end end)"#,
    );
    state
        .plugin_engine
        .as_mut()
        .unwrap()
        .set_budget(Duration::from_millis(20));
    let started = Instant::now();
    let outcome = state
        .plugin_engine
        .as_ref()
        .unwrap()
        .fire_action(&state, "spin", "");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the loop was cut off"
    );
    let ActionOutcome::Failed(reason) = outcome else {
        panic!("a runaway action fails");
    };
    assert!(reason.contains("budget"), "{reason}");
}

#[test]
fn hooks_answering_each_other_are_cut_off() {
    // Ping-pong: entering either directory navigates to the other.
    let dir = fixture();
    let (tx, _rx) = workers::channel();
    let mut state = with_plugin(
        dir.path(),
        r#"
trail.on_enter_dir(function(path)
    if path:match("sub$") then trail.navigate("..") else trail.navigate("sub") end
end)
"#,
    );
    settle(&mut state, &tx);
    assert!(
        notice(&state).contains("stopped after"),
        "{}",
        notice(&state)
    );
}

#[test]
fn on_enter_dir_fires_for_every_way_of_changing_directory() {
    let dir = fixture();
    let (tx, _rx) = workers::channel();
    let mut state = with_plugin(
        dir.path(),
        r#"
local seen = {}
trail.on_enter_dir(function(path, d)
    table.insert(seen, path:match("sub$") and "sub" or "top")
    trail.set_status(table.concat(seen, ","))
end)
"#,
    );
    // The start directory counts as entered.
    settle(&mut state, &tx);
    let start = state.plugin_status.clone().unwrap();
    // `l` into sub, then `h` back out: `h` never went through `enter_dir`.
    state.enter_dir(dir.path().join("sub")).unwrap();
    settle(&mut state, &tx);
    state.go_parent().unwrap();
    settle(&mut state, &tx);
    // Nothing changed: no event.
    settle(&mut state, &tx);
    let status = state.plugin_status.clone().unwrap();
    assert_eq!(status.matches(',').count(), 2, "{status}");
    assert!(status.starts_with(&start));
    assert!(status.contains(",sub,"), "{status}");
}

#[test]
fn a_plugin_that_fails_to_load_leaves_nothing_behind() {
    let mut engine = PluginEngine::new().unwrap();
    let err = engine.load_plugin_str(
        "half",
        r#"
trail.register_action("orphan", function() end)
error("load failed here")
"#,
    );
    assert!(err.is_err());
    assert!(engine.action_names().is_empty());
}

// ── Keys ──────────────────────────────────────────────────────────────────────

#[test]
fn a_bound_sequence_runs_its_action() {
    let dir = fixture();
    let mut state = with_plugin(
        dir.path(),
        r#"
trail.register_action("zap", function() end)
trail.bind("zz", "zap", "arg")
"#,
    );
    let mut ctx = trail::input::InputCtx::default();
    let first = trail::input::dispatch(key('z'), &state, &mut ctx);
    assert_eq!(first, Some(Action::SetPendingNavKey('z')));
    actions::apply(first.unwrap(), &mut state).unwrap();
    let second = trail::input::dispatch(key('z'), &state, &mut ctx);
    assert_eq!(
        second,
        Some(Action::ExecuteCommand(ParsedCommand::Plugin {
            name: "zap".to_owned(),
            arg: "arg".to_owned(),
        }))
    );
}

#[test]
fn a_plugin_cannot_take_over_a_key_trail_uses() {
    let dir = fixture();
    let state = with_plugin(
        dir.path(),
        r#"
trail.register_action("steal", function() end)
trail.bind("j", "steal")
trail.bind("enter", "steal")
trail.bind("gx", "steal")
trail.bind("zq", "missing")
"#,
    );
    // `j` still moves down.
    let mut ctx = trail::input::InputCtx::default();
    assert_eq!(
        trail::input::dispatch(key('j'), &state, &mut ctx),
        Some(Action::MoveDown)
    );
    let engine = state.plugin_engine.as_ref().unwrap();
    let dead = trail::plugin::dead_bindings(engine, &state.config.keymap);
    let joined = dead.join("\n");
    assert!(joined.contains("`j` can never fire"), "{joined}");
    assert!(joined.contains("`enter` can never fire"), "{joined}");
    assert!(joined.contains("unknown action 'missing'"), "{joined}");
    // `gx` joins the `g` prefix Trail already has, so it can fire.
    assert!(!joined.contains("`gx`"), "{joined}");
}

// ── Jobs and previewers ───────────────────────────────────────────────────────

#[tokio::test]
async fn a_spawned_job_reports_back_through_its_callback() {
    let dir = fixture();
    let (tx, mut rx) = workers::channel();
    let mut state = with_plugin(
        dir.path(),
        r#"
trail.register_action("job", function()
    trail.spawn{
        cmd = "echo from-a-job",
        on_exit = function(r)
            trail.set_status((r.ok and "ok:" or "failed:") .. r.stdout:gsub("%s+$", ""))
        end,
    }
end)
"#,
    );
    run_action(&mut state, &tx, "job", "");
    let msg = tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("the job finishes")
        .expect("a message arrives");
    assert!(matches!(msg, WorkerMsg::PluginJob { .. }));
    workers::merge(msg, &mut state);
    settle(&mut state, &tx);
    assert_eq!(state.plugin_status.as_deref(), Some("ok:from-a-job"));
}

#[tokio::test]
async fn a_registered_previewer_runs_its_command_off_thread() {
    let dir = fixture();
    let (tx, mut rx) = workers::channel();
    let state = with_plugin(
        dir.path(),
        r#"
trail.register_previewer{
    name = "echo",
    extensions = { ".MD" },
    command = "echo previewed-by-plugin",
}
"#,
    );
    let rules = state.plugin_engine.as_ref().unwrap().previewer_rules();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].extensions, vec!["md".to_owned()]);

    let provider = trail::plugin::previewer::CommandPreviewProvider::new(rules, String::new());
    let entry = state
        .entries
        .iter()
        .find(|e| e.file_name == "gamma.md")
        .cloned()
        .unwrap();
    let ctx = trail::preview::provider::PreviewCtx {
        show_hidden: false,
        worker_tx: tx.clone(),
        generation: 7,
        text_sync_threshold_bytes: 1024,
        max_preview_lines: 100,
    };
    use trail::preview::provider::PreviewProvider;
    assert!(provider.can_handle(&entry));
    assert!(!provider.can_handle(
        state
            .entries
            .iter()
            .find(|e| e.file_name == "alpha.txt")
            .unwrap()
    ));
    assert!(matches!(
        provider.preview(&entry, &ctx),
        trail::preview::provider::PreviewOutcome::Deferred
    ));
    let msg = tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("the previewer finishes")
        .expect("a message arrives");
    let WorkerMsg::Preview {
        generation,
        content,
        ..
    } = msg
    else {
        panic!("a preview result");
    };
    assert_eq!(
        generation, 7,
        "the result carries the generation it answers"
    );
    let trail::preview::provider::PreviewContent::Text(lines) = content else {
        panic!("text content");
    };
    assert!(lines[0].contains("previewed-by-plugin"), "{lines:?}");
}

#[test]
fn a_previewer_without_a_match_is_refused_at_load() {
    let mut engine = PluginEngine::new().unwrap();
    let err = engine
        .load_plugin_str(
            "bad",
            r#"trail.register_previewer{ command = "cat {path}" }"#,
        )
        .unwrap_err();
    assert!(err.to_string().contains("extensions"), "{err}");
}

// ── Shipped plugins ───────────────────────────────────────────────────────────

/// Every example in `examples/plugins/` and the built-in bookmarks plugin must
/// at least load against the API they are documenting.
#[test]
fn the_shipped_plugins_load() {
    let examples = [
        ("bookmarks", trail::plugin::EXAMPLE_BOOKMARKS_PLUGIN),
        ("git_line", include_str!("../examples/plugins/git_line.lua")),
        (
            "open_in_editor",
            include_str!("../examples/plugins/open_in_editor.lua"),
        ),
        (
            "dir_summary",
            include_str!("../examples/plugins/dir_summary.lua"),
        ),
        (
            "json_preview",
            include_str!("../examples/plugins/json_preview.lua"),
        ),
        (
            "jump_back",
            include_str!("../examples/plugins/jump_back.lua"),
        ),
    ];
    // All of them at once, as a user enabling every example would: they must
    // not collide with each other or with the default keymap.
    let mut engine = PluginEngine::new().unwrap();
    for (name, source) in examples {
        engine
            .load_plugin_str(name, source)
            .unwrap_or_else(|e| panic!("{name} fails to load: {e}"));
    }
    let keymap = trail::config::load(None).unwrap().keymap;
    let dead = trail::plugin::dead_bindings(&engine, &keymap);
    assert!(dead.is_empty(), "{dead:?}");
    let mut keys: Vec<String> = engine.bindings().into_iter().map(|b| b.keys).collect();
    let bound = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), bound, "two examples bind the same key");
}

#[test]
fn dir_summary_adds_up_the_listing() {
    let dir = fixture();
    let (tx, _rx) = workers::channel();
    let mut state = with_plugin(
        dir.path(),
        include_str!("../examples/plugins/dir_summary.lua"),
    );
    run_action(&mut state, &tx, "dir_summary", "");
    // 1 + 2 + 3 bytes across three files; the directory is not counted.
    assert_eq!(notice(&state), "3 files, 1 directory, 6 B");
}
