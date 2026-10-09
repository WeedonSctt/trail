//! "Is this shell running a command?" — answered from the process tree.
//!
//! Shells do not announce when a command starts or finishes in any way common
//! to all of them, but every command they run is a child process. A shell with
//! no children is at its prompt; a shell with one is running something.
//!
//! Two kinds of child do not count. A process that only hosts the console
//! (`conhost`, `OpenConsole`) is plumbing, not a command. And a child that is
//! itself a shell counts only if *it* has children: Git Bash's `bin\bash.exe`
//! always runs `usr\bin\bash.exe` beneath it, so without this every Git Bash
//! would look permanently busy.
//!
//! Where the answer cannot be had — no process id, a process table that will
//! not read — the shell is treated as busy. The spec's rule: ask a question the
//! user did not need rather than lose a running command.

use std::collections::HashMap;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// Programs that are shells, compared without `.exe` and case.
const SHELLS: &[&str] = &[
    "bash",
    "sh",
    "zsh",
    "fish",
    "dash",
    "ksh",
    "nu",
    "elvish",
    "xonsh",
    "pwsh",
    "powershell",
    "cmd",
];

/// Processes that host a console rather than run a command.
const PLUMBING: &[&str] = &["conhost", "openconsole"];

/// For each pid in `shells`, whether that shell is running a command.
///
/// Reads the process table once for the whole batch. That read takes tens of
/// milliseconds on Windows, which is why it happens only when a confirmation
/// is about to be decided, never per frame.
#[must_use]
pub fn busy(shells: &[Option<u32>]) -> Vec<bool> {
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());

    let mut tree = ProcessTree::default();
    for (pid, process) in system.processes() {
        tree.add(
            pid.as_u32(),
            process.parent().map(Pid::as_u32),
            &process.name().to_string_lossy(),
        );
    }

    shells
        .iter()
        .map(|pid| match pid {
            Some(pid) => tree.is_busy(*pid),
            None => true,
        })
        .collect()
}

/// A snapshot of which process is whose child, by pid.
#[derive(Debug, Default)]
struct ProcessTree {
    children: HashMap<u32, Vec<u32>>,
    names: HashMap<u32, String>,
}

impl ProcessTree {
    fn add(&mut self, pid: u32, parent: Option<u32>, name: &str) {
        let name = name.to_ascii_lowercase();
        let name = name.strip_suffix(".exe").unwrap_or(&name).to_owned();
        self.names.insert(pid, name);
        if let Some(parent) = parent {
            self.children.entry(parent).or_default().push(pid);
        }
    }

    /// Whether `shell` has a descendant that is a command.
    ///
    /// A shell that is no longer in the table has exited, and is not busy.
    fn is_busy(&self, shell: u32) -> bool {
        if !self.names.contains_key(&shell) {
            return false;
        }
        self.has_command_below(shell, 0)
    }

    fn has_command_below(&self, pid: u32, depth: usize) -> bool {
        // Pids are reused, so a stale parent link can make a cycle; a real
        // shell nesting is never this deep.
        if depth > 16 {
            return true;
        }
        let Some(children) = self.children.get(&pid) else {
            return false;
        };
        children.iter().any(|child| {
            let name = self.names.get(child).map(String::as_str).unwrap_or("");
            if PLUMBING.contains(&name) {
                false
            } else if SHELLS.contains(&name) {
                self.has_command_below(*child, depth + 1)
            } else {
                true
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(entries: &[(u32, Option<u32>, &str)]) -> ProcessTree {
        let mut tree = ProcessTree::default();
        for (pid, parent, name) in entries {
            tree.add(*pid, *parent, name);
        }
        tree
    }

    #[test]
    fn a_shell_at_its_prompt_is_idle() {
        let t = tree(&[(10, Some(1), "pwsh.exe")]);
        assert!(!t.is_busy(10));
    }

    #[test]
    fn a_shell_with_a_command_running_is_busy() {
        let t = tree(&[(10, Some(1), "pwsh.exe"), (11, Some(10), "cargo.exe")]);
        assert!(t.is_busy(10));
    }

    #[test]
    fn console_hosts_do_not_count() {
        let t = tree(&[(10, Some(1), "cmd.exe"), (11, Some(10), "conhost.exe")]);
        assert!(!t.is_busy(10));
    }

    /// Git Bash's launcher always has the real bash under it.
    #[test]
    fn a_shell_under_a_shell_counts_only_when_it_is_busy() {
        let idle = tree(&[(10, Some(1), "bash.exe"), (11, Some(10), "bash.exe")]);
        assert!(!idle.is_busy(10));

        let busy = tree(&[
            (10, Some(1), "bash.exe"),
            (11, Some(10), "bash.exe"),
            (12, Some(11), "make.exe"),
        ]);
        assert!(busy.is_busy(10));
    }

    #[test]
    fn a_shell_that_is_gone_is_not_busy() {
        let t = tree(&[(10, Some(1), "pwsh")]);
        assert!(!t.is_busy(99));
    }

    #[test]
    fn an_unknown_pid_is_treated_as_busy() {
        assert_eq!(busy(&[None]), vec![true]);
    }

    /// The live table: a pid no process can have is a shell that has exited.
    #[test]
    fn reads_the_live_process_table() {
        assert_eq!(busy(&[Some(u32::MAX)]), vec![false]);
    }
}
