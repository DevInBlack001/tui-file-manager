// app.rs - Top-level application state machine.
//
// Owns all runtime state (current directory, selection, clipboard, preview
// cache, active input mode) and translates key events into calls against the
// core/fs/preview/theme layers. Never touches the terminal directly - that
// stays in main.rs, which also handles the one action (`RunForeground`) that
// needs to suspend the TUI.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;

use crate::config::{Config, SortKey};
use crate::core::bookmarks::{self, Bookmark, Bookmarks};
use crate::core::{ClipMode, Clipboard, Entry, Job, Listing, Recents, TransferClient, TransferError};
use crate::fs::desktop_apps::{self, DesktopApp};
use crate::fs::ops::{self, OpResult};
use crate::fs::xdg::XdgDirs;
use crate::preview::{PreviewContent, Previewer};
use crate::theme::Theme;

// ---------------------------------------------------------------------------
// Action - things app.rs cannot do itself because it does not own the
// terminal.
// ---------------------------------------------------------------------------

pub enum Action {
    None,
    Quit,
    /// Leave the alternate screen, run `program` with `args` in the
    /// foreground, wait for it to exit, then restore the TUI. Used for
    /// `$EDITOR`.
    RunForeground(PathBuf, Vec<String>),
}

// ---------------------------------------------------------------------------
// Mode - what the status bar / key handler currently does with input.
// ---------------------------------------------------------------------------

pub enum PromptKind {
    Rename,
    Mkdir,
    Touch,
    SymlinkTarget,
    SymlinkName,
    OpenWith,
    SendRemote,
}

impl PromptKind {
    pub fn label(&self) -> &'static str {
        match self {
            PromptKind::Rename => "rename",
            PromptKind::Mkdir => "new directory",
            PromptKind::Touch => "new file",
            PromptKind::SymlinkTarget => "symlink target",
            PromptKind::SymlinkName => "symlink name",
            PromptKind::OpenWith => "open with",
            PromptKind::SendRemote => "send to (user@host:/path)",
        }
    }
}

pub enum Mode {
    Normal,
    Search,
    GotoPath(String),
    Prompt(PromptKind, String),
    ConfirmDelete(String),
    /// `nvim` (fim's preferred default editor) is missing; ask before
    /// installing it via `sudo pacman -S neovim`.
    ConfirmInstallEditor,
    Help,
    /// "Open with" app picker: applications discovered via `.desktop`
    /// `MimeType=` associations that claim to handle the focused file. `/`
    /// switches to the free-text `Prompt(OpenWith, _)` for a custom command.
    OpenWithPicker { apps: Vec<DesktopApp>, selected: usize },
}

pub struct StatusMsg {
    pub text: String,
    pub is_error: bool,
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

pub struct App {
    pub config: Config,
    pub theme: Theme,
    pub xdg: XdgDirs,

    pub cwd: PathBuf,
    pub listing: Listing,
    pub cursor: usize,
    pub selected: HashSet<PathBuf>,
    /// Tree view only: the one directory (if any) whose immediate children
    /// are peeked inline, toggled with `z`. Reset whenever the listing
    /// reloads so it never points at a directory no longer on screen.
    pub tree_expanded: Option<PathBuf>,

    pub show_hidden: bool,
    pub sort_key: SortKey,
    pub sort_reverse: bool,
    pub filter: String,
    filter_snapshot: String,

    pub clipboard: Clipboard,
    pub recents: Recents,
    pub bookmarks: Bookmarks,
    pub transfer: TransferClient,
    mtp_mount_attempts: HashSet<String>,

    /// Whether the sidebar (rather than the file list) currently has
    /// keyboard focus - `b` toggles this. `1`-`9` jump directly to a
    /// bookmark/device regardless of focus, but with more than nine
    /// sidebar entries (easy once several devices are plugged in) the rest
    /// are only reachable by focusing the sidebar and using `j`/`k`/`Enter`.
    pub sidebar_focused: bool,
    pub sidebar_cursor: usize,

    previewer: Previewer,
    pub preview_content: PreviewContent,
    preview_path: Option<PathBuf>,
    pub preview_area: Rect,
    pub focused_mime: Option<String>,

    pub focused_job: Option<Job>,
    job_poll_at: Instant,
    mounts_poll_at: Instant,
    mtp_poll_at: Instant,

    pub viewing_recents: bool,
    pub mode: Mode,
    pending_symlink_target: Option<String>,
    pending_delete: Vec<PathBuf>,
    pending_open_path: Option<PathBuf>,

    pub status: Option<StatusMsg>,
    pub should_quit: bool,
}

impl App {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    pub fn new(config: Config, mut xdg: XdgDirs) -> Self {
        let theme = crate::theme::load(&config.theme.source);

        // `[bookmarks] work_dir` in config.toml overrides the auto-detected
        // $HOME/Work; if it doesn't exist, Work is hidden from the sidebar
        // (never silently falls back to a hardcoded path).
        if let Some(ref wd) = config.bookmarks.work_dir {
            xdg.work = if wd.is_dir() { Some(wd.clone()) } else { None };
        }

        let custom: Vec<bookmarks::CustomBookmark> = config
            .bookmarks
            .custom
            .iter()
            .map(|c| bookmarks::CustomBookmark {
                name: c.name.clone(),
                path: c.path.clone(),
            })
            .collect();
        let mut bookmarks = bookmarks::build(&xdg, &custom);
        bookmarks.devices = crate::core::mounts::detect();

        let cwd = std::env::current_dir().unwrap_or_else(|_| xdg.home.clone());
        let show_hidden = config.ui.show_hidden;
        let sort_key = config.ui.sort_key;
        let sort_reverse = config.ui.sort_reverse;
        let listing = crate::core::read_dir(&cwd, show_hidden, sort_key, sort_reverse);

        let mut recents = Recents::load(config.recents.max_entries);
        recents.push(cwd.clone());
        recents.save();

        let transfer = TransferClient::new(config.transfer.ftctl_path.as_deref());

        let mut app = Self {
            config,
            theme,
            xdg,
            cwd,
            listing,
            cursor: 0,
            selected: HashSet::new(),
            tree_expanded: None,
            show_hidden,
            sort_key,
            sort_reverse,
            filter: String::new(),
            filter_snapshot: String::new(),
            clipboard: Clipboard::default(),
            recents,
            bookmarks,
            transfer,
            mtp_mount_attempts: HashSet::new(),
            sidebar_focused: false,
            sidebar_cursor: 0,
            previewer: Previewer::new(),
            preview_content: PreviewContent::Loading,
            preview_path: None,
            preview_area: Rect::new(0, 0, 40, 20),
            focused_mime: None,
            focused_job: None,
            job_poll_at: Instant::now() - Duration::from_secs(3),
            mounts_poll_at: Instant::now(),
            mtp_poll_at: Instant::now() - Duration::from_secs(5),
            viewing_recents: false,
            mode: Mode::Normal,
            pending_symlink_target: None,
            pending_delete: Vec::new(),
            pending_open_path: None,
            status: None,
            should_quit: false,
        };
        app.request_preview();
        app
    }

    // -----------------------------------------------------------------------
    // Read-only helpers used by the UI layer
    // -----------------------------------------------------------------------

    pub fn visible_entries(&self) -> Vec<&Entry> {
        crate::core::listing::filter_entries(&self.listing.entries, &self.filter)
    }

    pub fn focused_entry(&self) -> Option<&Entry> {
        self.visible_entries().into_iter().nth(self.cursor)
    }

    /// The raw terminal-graphics payload to blit directly to the terminal
    /// for the currently focused entry, and the path it belongs to (so the
    /// caller can tell a genuinely new image apart from the same one still
    /// showing). `None` when the current preview isn't a graphics image.
    pub fn preview_graphics(&self) -> Option<(&std::path::Path, &[u8])> {
        match (&self.preview_content, &self.preview_path) {
            (PreviewContent::RawGraphics(bytes), Some(path)) => Some((path.as_path(), bytes.as_slice())),
            _ => None,
        }
    }

    pub fn flattened_bookmarks(&self) -> Vec<&Bookmark> {
        self.bookmarks
            .sections
            .iter()
            .chain(self.bookmarks.custom.iter())
            .chain(self.bookmarks.devices.iter())
            .collect()
    }

    // -----------------------------------------------------------------------
    // Background work: preview + transfer job polling
    // -----------------------------------------------------------------------

    /// Drain any pending preview results from the background worker. Cheap;
    /// safe to call every loop iteration.
    pub fn poll_preview(&mut self) {
        while let Ok((path, content)) = self.previewer.receiver.try_recv() {
            if Some(&path) == self.preview_path.as_ref() {
                self.preview_content = content;
            }
        }
    }

    /// Refresh the sidebar's mounted-devices list, at most once every 3
    /// seconds - a plain `/proc/self/mounts` read is cheap, but there's no
    /// reason to redo it every frame.
    pub fn maybe_poll_mounts(&mut self) {
        // MTP auto-mount runs on its own, slightly longer cadence: it's a
        // real subprocess call (`gio mount -li`), not just a /proc read.
        if self.mtp_poll_at.elapsed() >= Duration::from_secs(5) {
            self.mtp_poll_at = Instant::now();
            crate::core::mounts::mount_pending_mtp_devices(&mut self.mtp_mount_attempts);
        }

        if self.mounts_poll_at.elapsed() < Duration::from_secs(3) {
            return;
        }
        self.mounts_poll_at = Instant::now();
        self.bookmarks.devices = crate::core::mounts::detect();

        // A device disappearing (unplugged) while the sidebar cursor was on
        // it, or past it, would otherwise leave the cursor pointing beyond
        // the now-shorter list.
        let len = self.flattened_bookmarks().len();
        if len == 0 {
            self.sidebar_cursor = 0;
        } else if self.sidebar_cursor >= len {
            self.sidebar_cursor = len - 1;
        }
    }

    /// Poll the transfer daemon for a job touching the focused entry, at
    /// most once every 2 seconds. `ftctl list` is a real subprocess call, so
    /// this must never run on every frame.
    pub fn maybe_poll_job_status(&mut self) {
        if !self.transfer.is_available() {
            return;
        }
        if self.job_poll_at.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.job_poll_at = Instant::now();
        match self.focused_entry() {
            Some(e) => {
                let path = e.path.clone();
                self.focused_job = self.transfer.job_for_path(&path);
            }
            None => self.focused_job = None,
        }
    }

    fn request_preview(&mut self) {
        let target = self.focused_entry().map(|e| (e.path.clone(), e.is_dir));
        match target {
            Some((path, is_dir)) => {
                self.focused_mime = if is_dir {
                    None
                } else {
                    Some(crate::fs::mime::detect(&path))
                };
                self.preview_path = Some(path.clone());
                if self.config.preview.enabled {
                    self.preview_content = PreviewContent::Loading;
                    // preview_area is the *outer* rect handed to the "Preview"
                    // Block, border included; content (and, critically, any
                    // graphics image - see main.rs's sync_preview_graphics,
                    // which blits at preview_area.{x,y} + 1, i.e. just inside
                    // the border) is drawn into the 1-cell-smaller-on-each-
                    // side inner area. Requesting the outer dimensions here
                    // sized every image 2 columns/rows larger than the space
                    // it's actually drawn into, so it overflowed past the
                    // pane's own border.
                    let inner_width = self.preview_area.width.saturating_sub(2);
                    let inner_height = self.preview_area.height.saturating_sub(2);
                    self.previewer.request(
                        path,
                        is_dir,
                        inner_width,
                        inner_height,
                        self.config.preview.max_text_lines,
                        self.config.preview.max_binary_bytes,
                        self.config.preview.video_thumbs,
                    );
                } else {
                    self.preview_content =
                        PreviewContent::Unavailable("preview disabled in config".to_string());
                }
            }
            None => {
                self.focused_mime = None;
                self.preview_path = None;
                self.preview_content = PreviewContent::Unavailable("no entry selected".to_string());
            }
        }
    }

    fn refresh_cursor_and_preview(&mut self) {
        self.clamp_cursor();
        self.request_preview();
    }

    fn clamp_cursor(&mut self) {
        let len = self.visible_entries().len();
        self.cursor = if len == 0 { 0 } else { self.cursor.min(len - 1) };
    }

    // -----------------------------------------------------------------------
    // Status helpers
    // -----------------------------------------------------------------------

    fn set_status(&mut self, text: String) {
        self.status = Some(StatusMsg { text, is_error: false });
    }

    fn set_error(&mut self, text: String) {
        self.status = Some(StatusMsg { text, is_error: true });
    }

    fn report_op(&mut self, result: OpResult, ok_msg: String) {
        match result {
            OpResult::Ok(_) => self.set_status(ok_msg),
            OpResult::PermissionDenied => self.set_error("permission denied".to_string()),
            OpResult::NotFound => self.set_error("not found".to_string()),
            OpResult::Err(e) => self.set_error(e),
        }
    }

    // -----------------------------------------------------------------------
    // Navigation
    // -----------------------------------------------------------------------

    fn reload_listing(&mut self) {
        self.listing = crate::core::read_dir(&self.cwd, self.show_hidden, self.sort_key, self.sort_reverse);
        self.cursor = 0;
        self.selected.clear();
        self.tree_expanded = None;
        self.clamp_cursor();
        self.request_preview();
    }

    fn navigate_to(&mut self, path: PathBuf) {
        if !path.is_dir() {
            self.set_error(format!("not a directory: {}", path.display()));
            return;
        }
        self.cwd = path;
        self.viewing_recents = false;
        self.recents.push(self.cwd.clone());
        self.recents.save();
        self.reload_listing();
    }

    fn enter_selected(&mut self) {
        let Some(entry) = self.focused_entry() else { return };
        let path = entry.path.clone();
        let is_dir = entry.is_dir;
        if self.viewing_recents || is_dir {
            self.navigate_to(path);
        } else {
            let name = entry.name.clone();
            self.set_status(format!("'{name}' is not a directory"));
        }
    }

    fn go_parent(&mut self) {
        if self.viewing_recents {
            self.viewing_recents = false;
            self.reload_listing();
            return;
        }
        if let Some(parent) = self.cwd.parent() {
            let p = parent.to_path_buf();
            self.navigate_to(p);
        }
    }

    fn enter_recents_view(&mut self) {
        let entries: Vec<Entry> = self
            .recents
            .paths()
            .iter()
            .map(|p| Entry::from_path(p.clone()))
            .collect();
        self.listing = Listing {
            path: self.cwd.clone(),
            entries,
            error: None,
        };
        self.viewing_recents = true;
        self.cursor = 0;
        self.selected.clear();
        self.request_preview();
    }

    fn jump_to_bookmark(&mut self, index: usize) {
        let target = {
            let flat = self.flattened_bookmarks();
            flat.get(index)
                .map(|bm| (bm.is_recents, bm.exists, bm.name.clone(), bm.path.clone()))
        };
        let Some((is_recents, exists, name, path)) = target else { return };
        if is_recents {
            self.enter_recents_view();
        } else if !exists {
            self.set_error(format!("{name} does not exist"));
        } else {
            self.navigate_to(path);
        }
    }

    /// Move the sidebar's own cursor (used while `sidebar_focused`), clamped
    /// to the flattened list of every section/custom bookmark/device -
    /// this is the only way to reach an entry past the ninth, since `1`-`9`
    /// can only ever address nine slots.
    fn move_sidebar_cursor(&mut self, delta: isize) {
        let len = self.flattened_bookmarks().len();
        if len == 0 {
            return;
        }
        let next = self.sidebar_cursor as isize + delta;
        self.sidebar_cursor = next.clamp(0, len as isize - 1) as usize;
    }

    /// `Enter`/`l` while the sidebar is focused: navigate to the entry under
    /// the sidebar cursor, then return focus to the file list, matching the
    /// "select and go" feel of `1`-`9`.
    fn activate_sidebar_cursor(&mut self) {
        self.jump_to_bookmark(self.sidebar_cursor);
        self.sidebar_focused = false;
    }

    /// Index range within `flattened_bookmarks()` that belongs to
    /// `bookmarks.devices`, since only those are ejectable.
    fn sidebar_cursor_is_device(&self) -> bool {
        let devices_start = self.bookmarks.sections.len() + self.bookmarks.custom.len();
        self.sidebar_cursor >= devices_start
    }

    /// `E` while the sidebar is focused on a device: eject/unmount it.
    fn eject_sidebar_cursor(&mut self) {
        if !self.sidebar_cursor_is_device() {
            self.set_status("only devices can be ejected".to_string());
            return;
        }
        let target = self.flattened_bookmarks().get(self.sidebar_cursor).map(|bm| (bm.name.clone(), bm.path.clone()));
        let Some((name, path)) = target else { return };
        match crate::core::mounts::eject(&path) {
            Ok(()) => self.set_status(format!("ejecting {name}...")),
            Err(e) => self.set_error(e),
        }
    }

    fn refresh(&mut self) {
        self.theme = crate::theme::load(&self.config.theme.source);
        if self.viewing_recents {
            self.enter_recents_view();
        } else {
            self.reload_listing();
        }
        self.set_status("refreshed".to_string());
    }

    // -----------------------------------------------------------------------
    // Sort / filter / hidden
    // -----------------------------------------------------------------------

    fn cycle_sort(&mut self) {
        self.sort_key = match self.sort_key {
            SortKey::Name => SortKey::Size,
            SortKey::Size => SortKey::Mtime,
            SortKey::Mtime => SortKey::Type,
            SortKey::Type => SortKey::Name,
        };
        let label = format!("{:?}", self.sort_key);
        self.reload_listing();
        self.set_status(format!("sort: {label}"));
    }

    fn toggle_sort_reverse(&mut self) {
        self.sort_reverse = !self.sort_reverse;
        self.reload_listing();
    }

    fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.reload_listing();
    }

    fn cycle_layout_mode(&mut self) {
        self.config.ui.layout_mode = self.config.ui.layout_mode.next();
        self.set_status(format!("layout: {}", self.config.ui.layout_mode.label()));
    }

    fn cycle_view_mode(&mut self) {
        self.config.ui.view_mode = self.config.ui.view_mode.next();
        self.tree_expanded = None;
        self.set_status(format!("view: {}", self.config.ui.view_mode.label()));
    }

    /// Tree view only: peek/collapse the focused directory's immediate
    /// children inline, without navigating into it.
    fn toggle_tree_expand(&mut self) {
        let Some(entry) = self.focused_entry() else { return };
        if !entry.is_dir {
            return;
        }
        let path = entry.path.clone();
        self.tree_expanded = if self.tree_expanded.as_ref() == Some(&path) { None } else { Some(path) };
    }

    // -----------------------------------------------------------------------
    // Selection / clipboard
    // -----------------------------------------------------------------------

    fn toggle_selection(&mut self) {
        if let Some(entry) = self.focused_entry() {
            let path = entry.path.clone();
            if !self.selected.remove(&path) {
                self.selected.insert(path);
            }
        }
    }

    fn select_all_visible(&mut self) {
        let paths: Vec<PathBuf> = self.visible_entries().into_iter().map(|e| e.path.clone()).collect();
        self.selected.extend(paths);
    }

    fn clear_selection(&mut self) {
        self.selected.clear();
    }

    fn selection_or_cursor(&self) -> Vec<PathBuf> {
        if !self.selected.is_empty() {
            self.selected.iter().cloned().collect()
        } else {
            self.focused_entry().map(|e| vec![e.path.clone()]).unwrap_or_default()
        }
    }

    fn copy_selection(&mut self) {
        let paths = self.selection_or_cursor();
        if paths.is_empty() {
            return;
        }
        let n = paths.len();
        self.clipboard.set(paths, ClipMode::Copy);
        self.set_status(format!("copied {n} item(s) to clipboard"));
    }

    fn cut_selection(&mut self) {
        let paths = self.selection_or_cursor();
        if paths.is_empty() {
            return;
        }
        let n = paths.len();
        self.clipboard.set(paths, ClipMode::Move);
        self.set_status(format!("cut {n} item(s) to clipboard"));
    }

    fn paste_clipboard(&mut self) {
        if self.clipboard.is_empty() {
            self.set_error("clipboard is empty".to_string());
            return;
        }
        let is_move = matches!(self.clipboard.mode, Some(ClipMode::Move));
        let mode_str = if is_move { "move" } else { "copy" };
        match self.transfer.enqueue(mode_str, &self.clipboard.paths, &self.cwd) {
            Ok(id) => {
                self.set_status(format!("queued {mode_str} job {id}"));
                if is_move {
                    self.clipboard.clear();
                }
            }
            Err(e) => self.set_error(describe_transfer_error(&e)),
        }
    }

    /// `u`: send the selected entries (or the focused one) directly to a
    /// remote host over SSH, via `ftctl`'s `[user@]host:/path` rsync-style
    /// destination spec. fim never validates the spec itself; the daemon
    /// resolves it exactly as its own README documents, and auth/host trust
    /// stay entirely the system's own SSH.
    fn send_to_remote(&mut self, spec: &str) {
        let spec = spec.trim();
        if spec.is_empty() {
            return;
        }
        let paths = self.selection_or_cursor();
        if paths.is_empty() {
            self.set_error("nothing selected to send".to_string());
            return;
        }
        let n = paths.len();
        match self.transfer.enqueue("copy", &paths, Path::new(spec)) {
            Ok(id) => self.set_status(format!("queued send of {n} item(s) to {spec} (job {id})")),
            Err(e) => self.set_error(describe_transfer_error(&e)),
        }
    }

    fn show_clipboard(&mut self) {
        if self.clipboard.is_empty() {
            self.set_status("clipboard is empty".to_string());
        } else {
            let desc = self.clipboard.describe();
            self.set_status(desc);
        }
    }

    // -----------------------------------------------------------------------
    // File operations
    // -----------------------------------------------------------------------

    fn trash_selection(&mut self) {
        let paths = self.selection_or_cursor();
        if paths.is_empty() {
            return;
        }
        let mut errors = Vec::new();
        for p in &paths {
            match ops::trash(p) {
                OpResult::Ok(_) => {}
                OpResult::PermissionDenied => errors.push(format!("permission denied: {}", p.display())),
                OpResult::NotFound => errors.push(format!("not found: {}", p.display())),
                OpResult::Err(e) => errors.push(e),
            }
        }
        let n = paths.len();
        self.reload_listing();
        if errors.is_empty() {
            self.set_status(format!("trashed {n} item(s)"));
        } else {
            self.set_error(errors.join("; "));
        }
    }

    fn begin_delete_confirm(&mut self) {
        let paths = self.selection_or_cursor();
        if paths.is_empty() {
            return;
        }
        self.pending_delete = paths;
        self.mode = Mode::ConfirmDelete(String::new());
    }

    fn confirm_delete(&mut self) {
        let paths = std::mem::take(&mut self.pending_delete);
        let mut errors = Vec::new();
        for p in &paths {
            match ops::delete(p) {
                OpResult::Ok(_) => {}
                OpResult::PermissionDenied => errors.push(format!("permission denied: {}", p.display())),
                OpResult::NotFound => errors.push(format!("not found: {}", p.display())),
                OpResult::Err(e) => errors.push(e),
            }
        }
        let n = paths.len();
        self.reload_listing();
        if errors.is_empty() {
            self.set_status(format!("deleted {n} item(s)"));
        } else {
            self.set_error(errors.join("; "));
        }
    }

    fn submit_prompt(&mut self, kind: PromptKind, input: String) {
        match kind {
            PromptKind::Rename => {
                if let Some(entry) = self.focused_entry() {
                    let path = entry.path.clone();
                    let result = ops::rename(&path, &input);
                    self.report_op(result, format!("renamed to '{input}'"));
                    self.reload_listing();
                }
            }
            PromptKind::Mkdir => {
                let result = ops::mkdir(&self.cwd, &input);
                self.report_op(result, format!("created directory '{input}'"));
                self.reload_listing();
            }
            PromptKind::Touch => {
                let result = ops::touch(&self.cwd, &input);
                self.report_op(result, format!("created '{input}'"));
                self.reload_listing();
            }
            PromptKind::SymlinkTarget => {
                self.pending_symlink_target = Some(input);
                self.mode = Mode::Prompt(PromptKind::SymlinkName, String::new());
                return;
            }
            PromptKind::SymlinkName => {
                let target = self.pending_symlink_target.take().unwrap_or_default();
                let link_path = self.cwd.join(&input);
                match std::os::unix::fs::symlink(&target, &link_path) {
                    Ok(()) => self.set_status(format!("linked '{input}' -> '{target}'")),
                    Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                        self.set_error("permission denied".to_string())
                    }
                    Err(e) => self.set_error(format!("symlink failed: {e}")),
                }
                self.reload_listing();
            }
            PromptKind::OpenWith => self.open_with(&input),
            PromptKind::SendRemote => self.send_to_remote(&input),
        }
        self.mode = Mode::Normal;
    }

    fn submit_goto(&mut self, input: String) {
        let expanded = expand_goto_path(&input, &self.xdg.home, &self.cwd);
        self.navigate_to(expanded);
        self.mode = Mode::Normal;
    }

    // -----------------------------------------------------------------------
    // External programs
    // -----------------------------------------------------------------------

    /// fim's preferred default editor is `nvim`, regardless of `$EDITOR`. If
    /// it isn't installed, `ConfirmInstallEditor` offers to install it before
    /// falling back to `$EDITOR`.
    fn open_editor(&mut self) -> Action {
        let Some(entry) = self.focused_entry() else { return Action::None };
        if entry.is_dir {
            self.set_error("cannot open a directory in an editor".to_string());
            return Action::None;
        }
        let path = entry.path.clone();

        if let Some(bin) = ops::resolve_bin("nvim") {
            return Action::RunForeground(bin, vec![path.to_string_lossy().into_owned()]);
        }

        self.pending_open_path = Some(path);
        self.mode = Mode::ConfirmInstallEditor;
        Action::None
    }

    fn install_nvim_then_retry(&mut self, install: bool) -> Action {
        if !install {
            return self.open_fallback_editor();
        }
        match ops::resolve_bin("sudo") {
            Some(sudo) => Action::RunForeground(
                sudo,
                vec![
                    "pacman".to_string(),
                    "-S".to_string(),
                    "--noconfirm".to_string(),
                    "neovim".to_string(),
                ],
            ),
            None => {
                self.pending_open_path = None;
                self.set_error("sudo not found; install neovim manually: pacman -S neovim".to_string());
                Action::None
            }
        }
    }

    /// Fall back to `$EDITOR` after the user declined to install nvim.
    /// `$EDITOR` commonly carries flags (e.g. Omarchy's
    /// `omarchy-launch-editor --inline`), so it's parsed as a command line,
    /// not a single binary name.
    fn open_fallback_editor(&mut self) -> Action {
        let Some(path) = self.pending_open_path.take() else { return Action::None };
        let editor = match std::env::var("EDITOR") {
            Ok(e) if !e.trim().is_empty() => e,
            _ => {
                self.set_error("$EDITOR is not set and nvim is not installed".to_string());
                return Action::None;
            }
        };
        match resolve_command_line(&editor, &path) {
            Some((bin, args)) => Action::RunForeground(bin, args),
            None => {
                self.set_error(format!("editor '{editor}' not found"));
                Action::None
            }
        }
    }

    fn open_xdg(&mut self) {
        let Some(entry) = self.focused_entry() else { return };
        let path = entry.path.clone();
        match ops::resolve_bin("xdg-open") {
            Some(bin) => {
                let spawned = std::process::Command::new(&bin)
                    .arg(&path)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                match spawned {
                    Ok(_) => self.set_status(format!("opened '{}' via xdg-open", path.display())),
                    Err(e) => self.set_error(format!("xdg-open failed: {e}")),
                }
            }
            None => self.set_error("xdg-open not found".to_string()),
        }
    }

    /// "Open with": run a user-typed command line against the focused
    /// entry, backgrounded (like xdg-open) rather than suspending the TUI,
    /// since this is commonly a GUI application.
    fn open_with(&mut self, cmdline: &str) {
        let Some(entry) = self.focused_entry() else { return };
        let path = entry.path.clone();
        match resolve_command_line(cmdline, &path) {
            Some((bin, args)) => {
                let spawned = std::process::Command::new(&bin)
                    .args(&args)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                match spawned {
                    Ok(_) => self.set_status(format!("opened with '{cmdline}'")),
                    Err(e) => self.set_error(format!("failed to launch '{cmdline}': {e}")),
                }
            }
            None => self.set_error(format!("'{cmdline}' not found")),
        }
    }

    /// `O`: offer a picker of installed applications that declare (via
    /// `.desktop` `MimeType=`) they can open the focused file - this also
    /// surfaces Wine/Proton-wrapped apps automatically, since their
    /// installers (Lutris, Bottles, Heroic, plain Wine) register ordinary
    /// `.desktop` entries in the same places. Falls back straight to the
    /// free-text prompt when the focused entry is a directory (no MIME type)
    /// or nothing matched, so typing a custom command always still works.
    fn open_with_menu(&mut self) {
        let Some(mime) = self.focused_mime.clone() else {
            self.mode = Mode::Prompt(PromptKind::OpenWith, String::new());
            return;
        };
        let apps = desktop_apps::candidates_for_mime(&mime);
        self.mode = if apps.is_empty() {
            Mode::Prompt(PromptKind::OpenWith, String::new())
        } else {
            Mode::OpenWithPicker { apps, selected: 0 }
        };
    }

    fn handle_open_with_picker_key(&mut self, key: KeyEvent, apps: Vec<DesktopApp>, selected: usize) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                let next = (selected + 1).min(apps.len().saturating_sub(1));
                self.mode = Mode::OpenWithPicker { apps, selected: next };
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let prev = selected.saturating_sub(1);
                self.mode = Mode::OpenWithPicker { apps, selected: prev };
            }
            KeyCode::Enter => {
                if let Some(app) = apps.into_iter().nth(selected) {
                    self.open_with_desktop_app(&app);
                }
                self.mode = Mode::Normal;
            }
            KeyCode::Char('/') | KeyCode::Char('c') => {
                self.mode = Mode::Prompt(PromptKind::OpenWith, String::new());
            }
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::Normal,
            _ => self.mode = Mode::OpenWithPicker { apps, selected },
        }
    }

    fn open_with_desktop_app(&mut self, app: &DesktopApp) {
        let Some(entry) = self.focused_entry() else { return };
        let path = entry.path.clone();
        match desktop_apps::resolve_desktop_exec(&app.exec, &path) {
            Some((bin, args)) => {
                let spawned = std::process::Command::new(&bin)
                    .args(&args)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                match spawned {
                    Ok(_) => self.set_status(format!("opened with {}", app.name)),
                    Err(e) => self.set_error(format!("failed to launch '{}': {e}", app.name)),
                }
            }
            None => self.set_error(format!("'{}' binary not found", app.name)),
        }
    }

    // -----------------------------------------------------------------------
    // Key handling
    // -----------------------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent) -> Action {
        if key.kind != KeyEventKind::Press {
            return Action::None;
        }

        let mode = std::mem::replace(&mut self.mode, Mode::Normal);
        match mode {
            Mode::Normal => return self.handle_normal_key(key),
            Mode::Help => self.mode = Mode::Normal,
            Mode::Search => self.handle_search_key(key),
            Mode::GotoPath(buf) => self.handle_goto_key(key, buf),
            Mode::Prompt(kind, buf) => self.handle_prompt_key(key, kind, buf),
            Mode::ConfirmDelete(buf) => self.handle_confirm_delete_key(key, buf),
            Mode::ConfirmInstallEditor => return self.handle_confirm_install_editor_key(key),
            Mode::OpenWithPicker { apps, selected } => self.handle_open_with_picker_key(key, apps, selected),
        }
        Action::None
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return Action::Quit;
        }
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                return Action::Quit;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if self.sidebar_focused { self.move_sidebar_cursor(1) } else { self.move_cursor(1) }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.sidebar_focused { self.move_sidebar_cursor(-1) } else { self.move_cursor(-1) }
            }
            KeyCode::Char('l') | KeyCode::Enter => {
                if self.sidebar_focused { self.activate_sidebar_cursor() } else { self.enter_selected() }
            }
            KeyCode::Char('h') | KeyCode::Backspace => {
                if self.sidebar_focused { self.sidebar_focused = false } else { self.go_parent() }
            }
            KeyCode::Char('b') => {
                self.sidebar_focused = !self.sidebar_focused;
                self.sidebar_cursor = 0;
            }
            KeyCode::Char('E') if self.sidebar_focused => self.eject_sidebar_cursor(),
            KeyCode::Char('~') => {
                let home = self.xdg.home.clone();
                self.navigate_to(home);
            }
            KeyCode::Char('.') => self.toggle_hidden(),
            KeyCode::Tab => self.cycle_layout_mode(),
            KeyCode::Char('v') => self.cycle_view_mode(),
            KeyCode::Char('z') => self.toggle_tree_expand(),
            KeyCode::Char('s') => self.cycle_sort(),
            KeyCode::Char('S') => self.toggle_sort_reverse(),
            KeyCode::Char('/') => {
                self.filter_snapshot = self.filter.clone();
                self.mode = Mode::Search;
            }
            KeyCode::Char('g') => self.mode = Mode::GotoPath(String::new()),
            KeyCode::Char('e') => return self.open_editor(),
            KeyCode::Char('o') => self.open_xdg(),
            KeyCode::Char('R') | KeyCode::F(5) => self.refresh(),
            KeyCode::Char(' ') => self.toggle_selection(),
            KeyCode::Char('a') => self.select_all_visible(),
            KeyCode::Esc => {
                if self.sidebar_focused { self.sidebar_focused = false } else { self.clear_selection() }
            }
            KeyCode::Char('c') => self.copy_selection(),
            KeyCode::Char('x') => self.cut_selection(),
            KeyCode::Char('p') => self.paste_clipboard(),
            KeyCode::Char('u') => self.mode = Mode::Prompt(PromptKind::SendRemote, String::new()),
            KeyCode::Char('P') => self.show_clipboard(),
            KeyCode::Char('d') => self.trash_selection(),
            KeyCode::Char('D') => self.begin_delete_confirm(),
            KeyCode::Char('r') => {
                let name = self.focused_entry().map(|e| e.name.clone());
                if let Some(name) = name {
                    self.mode = Mode::Prompt(PromptKind::Rename, name);
                }
            }
            KeyCode::Char('n') => self.mode = Mode::Prompt(PromptKind::Mkdir, String::new()),
            KeyCode::Char('t') => self.mode = Mode::Prompt(PromptKind::Touch, String::new()),
            KeyCode::Char('L') => self.mode = Mode::Prompt(PromptKind::SymlinkTarget, String::new()),
            KeyCode::Char('O') => self.open_with_menu(),
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                let idx = c.to_digit(10).unwrap() as usize - 1;
                self.jump_to_bookmark(idx);
            }
            _ => {}
        }
        Action::None
    }

    fn move_cursor(&mut self, delta: isize) {
        let len = self.visible_entries().len();
        if len == 0 {
            return;
        }
        let new = (self.cursor as isize + delta).clamp(0, len as isize - 1) as usize;
        if new != self.cursor {
            self.cursor = new;
            self.request_preview();
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        self.mode = Mode::Search;
        match key.code {
            KeyCode::Esc => {
                self.filter = std::mem::take(&mut self.filter_snapshot);
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => self.mode = Mode::Normal,
            KeyCode::Backspace => {
                self.filter.pop();
            }
            KeyCode::Char(c) => self.filter.push(c),
            _ => {}
        }
        self.refresh_cursor_and_preview();
    }

    fn handle_goto_key(&mut self, key: KeyEvent, mut buf: String) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => self.submit_goto(buf),
            KeyCode::Backspace => {
                buf.pop();
                self.mode = Mode::GotoPath(buf);
            }
            KeyCode::Char(c) => {
                buf.push(c);
                self.mode = Mode::GotoPath(buf);
            }
            _ => self.mode = Mode::GotoPath(buf),
        }
    }

    fn handle_prompt_key(&mut self, key: KeyEvent, kind: PromptKind, mut buf: String) {
        match key.code {
            KeyCode::Esc => {
                self.pending_symlink_target = None;
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => {
                if buf.trim().is_empty() {
                    self.set_error("input cannot be empty".to_string());
                    self.mode = Mode::Normal;
                } else {
                    self.submit_prompt(kind, buf);
                }
            }
            KeyCode::Backspace => {
                buf.pop();
                self.mode = Mode::Prompt(kind, buf);
            }
            KeyCode::Char(c) => {
                buf.push(c);
                self.mode = Mode::Prompt(kind, buf);
            }
            _ => self.mode = Mode::Prompt(kind, buf),
        }
    }

    fn handle_confirm_delete_key(&mut self, key: KeyEvent, mut buf: String) {
        match key.code {
            KeyCode::Esc => {
                self.pending_delete.clear();
                self.mode = Mode::Normal;
            }
            KeyCode::Enter => {
                if buf == "yes" {
                    self.confirm_delete();
                } else {
                    self.pending_delete.clear();
                    self.set_error("delete cancelled: type 'yes' exactly to confirm".to_string());
                }
                self.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                buf.pop();
                self.mode = Mode::ConfirmDelete(buf);
            }
            KeyCode::Char(c) => {
                buf.push(c);
                self.mode = Mode::ConfirmDelete(buf);
            }
            _ => self.mode = Mode::ConfirmDelete(buf),
        }
    }

    fn handle_confirm_install_editor_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => self.install_nvim_then_retry(true),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc | KeyCode::Enter => {
                self.install_nvim_then_retry(false)
            }
            _ => {
                self.mode = Mode::ConfirmInstallEditor;
                Action::None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Split a shell-style command line (e.g. `$EDITOR`'s value, or an
/// "open with" prompt entry) into a resolved absolute binary path plus its
/// leading arguments, with `path` appended as the final argument.
///
/// Splits on whitespace only (no quoting support) - sufficient for the
/// common case of a binary name plus flags, e.g. `omarchy-launch-editor
/// --inline` or `mpv --fullscreen`. Never passes a bare, `$PATH`-searched
/// name to `Command::new`.
fn resolve_command_line(cmdline: &str, path: &Path) -> Option<(PathBuf, Vec<String>)> {
    let mut parts = cmdline.split_whitespace();
    let cmd = parts.next()?;
    let bin = ops::resolve_bin(cmd)?;
    let mut args: Vec<String> = parts.map(|s| s.to_string()).collect();
    args.push(path.to_string_lossy().into_owned());
    Some((bin, args))
}

fn describe_transfer_error(e: &TransferError) -> String {
    match e {
        TransferError::NotFound { install_hint } => install_hint.clone(),
        TransferError::DaemonUnreachable => "transfer daemon unreachable".to_string(),
        TransferError::EnqueueFailed(s) => format!("enqueue failed: {s}"),
        TransferError::Timeout => "transfer request timed out".to_string(),
        TransferError::Other(s) => s.clone(),
    }
}

/// Expand a "goto path" prompt input into an absolute path.
///
/// Supports `/` (root), `~` and `~/...` (home), absolute paths as-is, and
/// relative paths resolved against `cwd`.
fn expand_goto_path(input: &str, home: &Path, cwd: &Path) -> PathBuf {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return PathBuf::from("/");
    }
    if trimmed == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        return home.join(rest);
    }
    let p = PathBuf::from(trimmed);
    if p.is_absolute() {
        p
    } else {
        cwd.join(p)
    }
}
