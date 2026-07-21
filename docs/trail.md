# Trail

> A terminal-first workspace for navigating, inspecting and acting on the filesystem without leaving the shell.

## Overview

Trail is a keyboard-driven terminal application designed to make filesystem navigation as fast and fluid as possible. Rather than behaving as a file picker or fuzzy finder, Trail acts as an interactive workspace where users can browse directories, inspect files, execute common filesystem operations and return to the shell in the directory where they finished working.

The application is intended to become part of the user's daily shell workflow. Instead of repeatedly changing directories, listing files and opening editors manually, users perform those actions from a single interface while remaining inside the terminal.

Trail is designed around one principle:

> **Navigation is the primary task. Every other feature exists to support it without interrupting the user's flow.**

---

# Interface

Trail occupies the entire terminal window while running.

The interface is divided into three main areas.

## Navigation Panel

Located on the left side of the interface.

Displays the contents of the current directory.

Features:

- Directories listed before files.
- Keyboard selection.
- Incremental filtering.
- Visual indicators.
- Hidden file support.
- Git repository indicators.
- Optional Git status indicators.
- Optional icons or alternative visual markers.
- Automatic refresh after filesystem changes.

The selected entry is always highlighted.

---

## Preview Panel

Located on the right side.

Displays contextual information about the currently selected entry.

The preview depends on the selected object's type.

### Directory

- Directory contents
- File count
- Directory count
- Hidden files
- Git information
- Additional metadata

### Text file

- File contents
- Syntax highlighting
- Line numbers
- Read-only preview

### Binary file

- Metadata
- File size
- Optional hexadecimal preview

### Image

- Metadata
- Resolution
- Dimensions

Additional preview providers may be added without changing the navigation workflow.

Changing the selection immediately updates the preview.

---

## Status Bar

Displayed at the bottom.

Contains contextual information including:

- Current path
- Current mode
- Active filter
- Current Git branch (when applicable)
- Number of visible entries
- Command input

---

# Navigation

Navigation is entirely keyboard driven.

Users move through directories by selecting entries and entering directories directly from the interface.

The current directory always represents the workspace location.

The interface supports:

- Enter directory
- Return to parent directory
- Navigate navigation history
- Refresh current directory
- Preserve current selection when possible

The user never needs to manually type paths during normal navigation.

---

# Filtering

Trail provides incremental fuzzy filtering for the current directory.

Filtering is local to the displayed directory.

As the user types:

- Matching entries remain visible.
- Results are continuously reordered.
- Selection updates automatically.

Removing the filter restores the original directory listing.

---

# Preview

Selecting an entry immediately updates the preview.

Preview generation is automatic and read-only.

Previewing never modifies filesystem contents.

Large files may display only an initial portion of their contents to maintain responsiveness.

---

# Actions

Most actions operate on the currently selected entry.

Typical actions include:

- Open in configured editor
- Open with operating system
- Copy absolute path
- Copy filename
- Copy relative path
- Rename
- Move
- Duplicate
- Delete
- Create file
- Create directory
- Reveal metadata
- Refresh current view

Actions should require as little typing as possible.

Frequently used operations are intended to be single-key actions.

Operations requiring parameters use Command Mode.

---

# Modes

Trail is organized around interaction modes.

## Navigation Mode

Default mode.

Used for:

- Moving through entries
- Entering directories
- Returning to parent directory
- Selecting files

This mode is optimized for speed and minimal hand movement.

---

## Search Mode

Activated when the user begins a search.

Provides incremental fuzzy filtering for the current directory.

Leaving Search Mode restores the complete directory listing.

---

## Command Mode

Used for operations that require arguments.

Examples include:

- Create file
- Create directory
- Rename
- Execute shell command
- Git operations
- Configuration
- Future extensions

Command Mode supports:

- History
- Completion
- Validation

---

# Shell Commands

Trail allows shell commands to be executed relative to the currently displayed directory.

Command execution does not terminate the workspace.

Once execution finishes, the interface is restored and the current navigation state remains intact.

This allows users to perform filesystem navigation and command execution from a single workspace.

---

# Session Behavior

Trail maintains an internal current directory throughout the session.

When the user exits normally, the shell continues in the directory currently displayed by Trail.

When the session is cancelled, the shell returns to the directory from which Trail was launched.

This allows users to explore freely without committing to a directory change unless they explicitly finish the session.

---

# Extensibility

Trail is designed to support future extensions without changing the navigation workflow.

Potential extensions include:

- Bookmarks
- Recent directories
- Workspace history
- Multiple tabs
- Split navigation
- Archive preview
- Image preview
- PDF preview
- Git integration
- Plugin system
- Configurable themes
- Custom key bindings
- User-defined commands

---

# Design Philosophy

Trail is not intended to replace the shell.

It is intended to become the shell's navigation workspace.

The shell remains responsible for command execution and scripting, while Trail provides an efficient environment for exploring and manipulating the filesystem.

The selected filesystem entry is the center of interaction. Users navigate first, inspect second and act last. Most operations therefore require little or no typing, allowing the interface to remain focused on speed, discoverability and uninterrupted workflow.
 
