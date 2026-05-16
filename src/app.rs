use crate::git_ops::{self, BranchInfo, CommitInfo, FileStatus, GraphLane, RemoteStatus};
use anyhow::Result;
use git2::Repository;
use ratatui::layout::Rect;
use std::time::{Duration, Instant};

/// Which tab the user is viewing
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tab {
    Graph,
    Files,
    Branches,
}

impl Tab {
    pub fn index(&self) -> usize {
        match self {
            Tab::Graph => 0,
            Tab::Files => 1,
            Tab::Branches => 2,
        }
    }
}

/// Which pane in the Files tab has focus
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FilePane {
    Unstaged,
    Staged,
    CommitMsg,
}

/// Input mode for text entry
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputMode {
    Normal,
    Editing, // typing into a text field
}

/// Popup dialog types
#[derive(Clone, Debug, PartialEq)]
pub enum Popup {
    None,
    NewBranch,
    ConfirmDelete(String), // branch name
    ConfirmPush,
    ConfirmPull,
    Help,
}

/// The main application state
pub struct App {
    pub repo: Repository,
    pub tab: Tab,
    pub running: bool,

    // Status bar message
    pub status_msg: String,

    // Popup state
    pub popup: Popup,
    pub input_mode: InputMode,
    pub input_buffer: String,

    // Graph tab
    pub commits: Vec<CommitInfo>,
    pub commit_scroll: usize,
    pub commit_selected: usize,
    pub graph_lanes: Vec<GraphLane>,
    pub current_branch: String,

    // Files tab
    pub staged_files: Vec<FileStatus>,
    pub unstaged_files: Vec<FileStatus>,
    pub file_pane: FilePane,
    pub staged_selected: usize,
    pub unstaged_selected: usize,
    pub commit_message: String,
    pub diff_text: String,
    pub diff_scroll: usize,      // scroll offset for diff view
    pub diff_total_lines: usize, // total lines in diff
    pub diff_area: Option<Rect>, // diff panel area for mouse scroll routing

    // Branches tab
    pub branches: Vec<BranchInfo>,
    pub branch_selected: usize,

    // Remote status
    pub remote_status: RemoteStatus,
    pub remote_url: Option<String>,

    // Auto-refresh
    pub auto_refresh: bool,
    pub auto_refresh_interval: Duration,
    pub last_refresh: Instant,

    // Mouse click region tracking — set during rendering
    pub click_regions: Vec<ClickRegion>,

    // When false, mouse capture is released so the terminal can drag-select
    // text for copy. Toggled via `m`.
    pub mouse_capture: bool,
}

/// A clickable region in the terminal, recorded during draw
#[derive(Clone, Debug)]
pub struct ClickRegion {
    pub rect: Rect,
    pub action: ClickAction,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum ClickAction {
    SelectTab(Tab),
    SelectCommit(usize),
    SelectUnstagedFile(usize),
    SelectStagedFile(usize),
    FocusCommitMsg,
    SelectBranch(usize),
    PushButton,
    PullButton,
    RefreshButton,
    StageAllButton,
    UnstageAllButton,
    CommitButton,
    NewBranchButton,
    FetchButton,
}

impl App {
    pub fn new(repo: Repository) -> Result<Self> {
        let mut app = Self {
            repo,
            tab: Tab::Graph,
            running: true,
            status_msg: String::new(),
            popup: Popup::None,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            commits: Vec::new(),
            commit_scroll: 0,
            commit_selected: 0,
            graph_lanes: Vec::new(),
            current_branch: String::new(),
            staged_files: Vec::new(),
            unstaged_files: Vec::new(),
            file_pane: FilePane::Unstaged,
            staged_selected: 0,
            unstaged_selected: 0,
            commit_message: String::new(),
            diff_text: String::new(),
            diff_scroll: 0,
            diff_total_lines: 0,
            diff_area: None,
            branches: Vec::new(),
            branch_selected: 0,
            remote_status: RemoteStatus::default(),
            remote_url: None,
            auto_refresh: false,
            auto_refresh_interval: Duration::from_secs(2),
            last_refresh: Instant::now(),
            click_regions: Vec::new(),
            mouse_capture: true,
        };
        app.refresh_all()?;
        Ok(app)
    }

    /// Toggle mouse capture so users can drag-select terminal text to copy.
    /// The loop in `lib.rs` reconciles this with the actual terminal state.
    pub fn toggle_mouse_capture(&mut self) {
        self.mouse_capture = !self.mouse_capture;
        self.status_msg = if self.mouse_capture {
            "Mouse capture OFF — clicks routed to UI".to_string()
        } else {
            "Mouse capture ON — drag to select/copy text".to_string()
        };
    }

    /// Refresh all data from the repository
    pub fn refresh_all(&mut self) -> Result<()> {
        // Branches first — refresh_commits needs them to assign lanes by branch.
        self.refresh_branches()?;
        self.refresh_commits()?;
        self.refresh_files()?;
        self.refresh_remote_status();
        self.current_branch = git_ops::get_current_branch(&self.repo);
        self.last_refresh = Instant::now();
        Ok(())
    }

    /// Toggle auto-refresh on/off
    pub fn toggle_auto_refresh(&mut self) {
        self.auto_refresh = !self.auto_refresh;
        if self.auto_refresh {
            self.last_refresh = Instant::now();
            self.status_msg = "Auto-refresh ON (2s)".to_string();
        } else {
            self.status_msg = "Auto-refresh OFF".to_string();
        }
    }

    /// Check if an auto-refresh is due, and perform it if so
    pub fn maybe_auto_refresh(&mut self) {
        if self.auto_refresh && self.last_refresh.elapsed() >= self.auto_refresh_interval {
            let _ = self.refresh_all();
        }
    }

    pub fn refresh_commits(&mut self) -> Result<()> {
        self.commits = git_ops::get_commits(&self.repo, 200)?;
        self.graph_lanes = git_ops::compute_branch_lanes(&self.commits, &self.branches);
        if self.commit_selected >= self.commits.len() {
            self.commit_selected = self.commits.len().saturating_sub(1);
        }
        Ok(())
    }

    pub fn refresh_files(&mut self) -> Result<()> {
        let (staged, unstaged) = git_ops::get_status(&self.repo)?;
        self.staged_files = staged;
        self.unstaged_files = unstaged;
        if self.staged_selected >= self.staged_files.len() {
            self.staged_selected = self.staged_files.len().saturating_sub(1);
        }
        if self.unstaged_selected >= self.unstaged_files.len() {
            self.unstaged_selected = self.unstaged_files.len().saturating_sub(1);
        }
        self.update_diff();
        Ok(())
    }

    pub fn refresh_branches(&mut self) -> Result<()> {
        self.branches = git_ops::get_branches(&self.repo)?;
        if self.branch_selected >= self.branches.len() {
            self.branch_selected = self.branches.len().saturating_sub(1);
        }
        Ok(())
    }

    pub fn refresh_remote_status(&mut self) {
        self.remote_status = git_ops::get_remote_status(&self.repo).unwrap_or_default();
        self.remote_url =
            git_ops::get_remote_url(&self.repo, self.remote_status.remote_name.as_deref());
    }

    /// Update the diff preview based on current selection
    fn update_diff(&mut self) {
        let result = match self.file_pane {
            FilePane::Unstaged => {
                if let Some(file) = self.unstaged_files.get(self.unstaged_selected) {
                    git_ops::get_file_diff(&self.repo, &file.path, false)
                } else {
                    Ok(String::new())
                }
            }
            FilePane::Staged => {
                if let Some(file) = self.staged_files.get(self.staged_selected) {
                    git_ops::get_file_diff(&self.repo, &file.path, true)
                } else {
                    Ok(String::new())
                }
            }
            _ => return,
        };
        self.diff_text = result.unwrap_or_else(|e| format!("Error: {}", e));
        self.diff_total_lines = self.diff_text.lines().count();
        self.diff_scroll = 0;
    }

    // ── Diff scroll ─────────────────────────────────────

    pub fn diff_scroll_up(&mut self, amount: usize) {
        self.diff_scroll = self.diff_scroll.saturating_sub(amount);
    }

    pub fn diff_scroll_down(&mut self, amount: usize, visible_height: usize) {
        let max_scroll = self.diff_total_lines.saturating_sub(visible_height);
        self.diff_scroll = (self.diff_scroll + amount).min(max_scroll);
    }

    // ── Navigation ──────────────────────────────────────

    pub fn next_tab(&mut self) {
        self.tab = match self.tab {
            Tab::Graph => Tab::Files,
            Tab::Files => Tab::Branches,
            Tab::Branches => Tab::Graph,
        };
    }

    pub fn prev_tab(&mut self) {
        self.tab = match self.tab {
            Tab::Graph => Tab::Branches,
            Tab::Files => Tab::Graph,
            Tab::Branches => Tab::Files,
        };
    }

    pub fn select_up(&mut self) {
        match self.tab {
            Tab::Graph => {
                self.commit_selected = self.commit_selected.saturating_sub(1);
            }
            Tab::Files => match self.file_pane {
                FilePane::Unstaged => {
                    self.unstaged_selected = self.unstaged_selected.saturating_sub(1);
                    self.update_diff();
                }
                FilePane::Staged => {
                    self.staged_selected = self.staged_selected.saturating_sub(1);
                    self.update_diff();
                }
                _ => {}
            },
            Tab::Branches => {
                self.branch_selected = self.branch_selected.saturating_sub(1);
            }
        }
    }

    pub fn select_down(&mut self) {
        match self.tab {
            Tab::Graph => {
                if self.commit_selected + 1 < self.commits.len() {
                    self.commit_selected += 1;
                }
            }
            Tab::Files => match self.file_pane {
                FilePane::Unstaged => {
                    if self.unstaged_selected + 1 < self.unstaged_files.len() {
                        self.unstaged_selected += 1;
                    }
                    self.update_diff();
                }
                FilePane::Staged => {
                    if self.staged_selected + 1 < self.staged_files.len() {
                        self.staged_selected += 1;
                    }
                    self.update_diff();
                }
                _ => {}
            },
            Tab::Branches => {
                if self.branch_selected + 1 < self.branches.len() {
                    self.branch_selected += 1;
                }
            }
        }
    }

    // ── Mouse handling ──────────────────────────────────

    /// Clear click regions before each draw
    pub fn clear_click_regions(&mut self) {
        self.click_regions.clear();
    }

    /// Register a clickable region
    pub fn register_click_region(&mut self, rect: Rect, action: ClickAction) {
        self.click_regions.push(ClickRegion { rect, action });
    }

    /// Handle a mouse click at (col, row), returns true if something was clicked
    pub fn handle_click(&mut self, col: u16, row: u16) -> bool {
        // Search in reverse order so top-most (last drawn) elements get priority
        let action = self
            .click_regions
            .iter()
            .rev()
            .find(|region| {
                col >= region.rect.x
                    && col < region.rect.x + region.rect.width
                    && row >= region.rect.y
                    && row < region.rect.y + region.rect.height
            })
            .map(|region| region.action.clone());

        if let Some(action) = action {
            self.dispatch_click(action);
            true
        } else {
            false
        }
    }

    fn dispatch_click(&mut self, action: ClickAction) {
        match action {
            ClickAction::SelectTab(tab) => self.tab = tab,
            ClickAction::SelectCommit(idx) => self.commit_selected = idx,
            ClickAction::SelectUnstagedFile(idx) => {
                self.unstaged_selected = idx;
                self.file_pane = FilePane::Unstaged;
                self.update_diff();
            }
            ClickAction::SelectStagedFile(idx) => {
                self.staged_selected = idx;
                self.file_pane = FilePane::Staged;
                self.update_diff();
            }
            ClickAction::FocusCommitMsg => {
                self.file_pane = FilePane::CommitMsg;
                self.input_mode = InputMode::Editing;
            }
            ClickAction::SelectBranch(idx) => self.branch_selected = idx,
            ClickAction::PushButton => self.start_push(),
            ClickAction::PullButton => self.start_pull(),
            ClickAction::RefreshButton => {
                let _ = self.refresh_all();
                self.status_msg = "Refreshed".to_string();
            }
            ClickAction::StageAllButton => self.stage_all(),
            ClickAction::UnstageAllButton => self.unstage_all(),
            ClickAction::CommitButton => self.do_commit(),
            ClickAction::NewBranchButton => self.start_new_branch(),
            ClickAction::FetchButton => self.do_fetch(),
        }
    }

    /// Handle mouse scroll in the diff pane
    pub fn handle_scroll(&mut self, col: u16, row: u16, up: bool, diff_area: Option<Rect>) {
        // If scroll is within the diff pane area, scroll the diff
        if let Some(area) = diff_area
            && col >= area.x
            && col < area.x + area.width
            && row >= area.y
            && row < area.y + area.height
        {
            let visible = area.height.saturating_sub(2) as usize; // minus borders
            if up {
                self.diff_scroll_up(3);
            } else {
                self.diff_scroll_down(3, visible);
            }
            return;
        }

        // Otherwise scroll the main list
        if up {
            self.select_up();
        } else {
            self.select_down();
        }
    }

    // ── File operations ─────────────────────────────────

    pub fn toggle_file_pane(&mut self) {
        self.file_pane = match self.file_pane {
            FilePane::Unstaged => FilePane::Staged,
            FilePane::Staged => FilePane::CommitMsg,
            FilePane::CommitMsg => FilePane::Unstaged,
        };
        self.update_diff();
    }

    pub fn stage_selected(&mut self) {
        if self.file_pane != FilePane::Unstaged {
            return;
        }
        if let Some(file) = self.unstaged_files.get(self.unstaged_selected) {
            let path = file.path.clone();
            match git_ops::stage_file(&self.repo, &path) {
                Ok(()) => {
                    self.status_msg = format!("Staged: {}", path);
                    let _ = self.refresh_files();
                }
                Err(e) => self.status_msg = format!("Error staging: {}", e),
            }
        }
    }

    pub fn unstage_selected(&mut self) {
        if self.file_pane != FilePane::Staged {
            return;
        }
        if let Some(file) = self.staged_files.get(self.staged_selected) {
            let path = file.path.clone();
            match git_ops::unstage_file(&self.repo, &path) {
                Ok(()) => {
                    self.status_msg = format!("Unstaged: {}", path);
                    let _ = self.refresh_files();
                }
                Err(e) => self.status_msg = format!("Error unstaging: {}", e),
            }
        }
    }

    pub fn stage_all(&mut self) {
        let paths: Vec<String> = self.unstaged_files.iter().map(|f| f.path.clone()).collect();
        self.batch_file_op(paths, git_ops::stage_file, "Staged");
    }

    pub fn unstage_all(&mut self) {
        let paths: Vec<String> = self.staged_files.iter().map(|f| f.path.clone()).collect();
        self.batch_file_op(paths, git_ops::unstage_file, "Unstaged");
    }

    fn batch_file_op(
        &mut self,
        paths: Vec<String>,
        op: fn(&Repository, &str) -> Result<()>,
        verb: &str,
    ) {
        let count = paths.iter().filter(|p| op(&self.repo, p).is_ok()).count();
        self.status_msg = format!("{} {} files", verb, count);
        let _ = self.refresh_files();
    }

    pub fn do_commit(&mut self) {
        if self.commit_message.trim().is_empty() {
            self.status_msg = "Commit message cannot be empty".to_string();
            return;
        }
        if self.staged_files.is_empty() {
            self.status_msg = "Nothing staged to commit".to_string();
            return;
        }
        match git_ops::create_commit(&self.repo, &self.commit_message) {
            Ok(sha) => {
                self.status_msg = format!("Committed: {}", sha);
                self.commit_message.clear();
                let _ = self.refresh_all();
            }
            Err(e) => self.status_msg = format!("Commit failed: {}", e),
        }
    }

    pub fn start_commit_input(&mut self) {
        self.file_pane = FilePane::CommitMsg;
        self.input_mode = InputMode::Editing;
    }

    // ── Remote operations ───────────────────────────────

    pub fn start_push(&mut self) {
        self.popup = Popup::ConfirmPush;
    }

    pub fn confirm_push(&mut self) {
        self.popup = Popup::None;
        self.status_msg = "Pushing...".to_string();
        match git_ops::push(&self.repo) {
            Ok(msg) => {
                self.status_msg = msg;
                self.refresh_remote_status();
                let _ = self.refresh_commits();
            }
            Err(e) => self.status_msg = format!("Push failed: {}", e),
        }
    }

    pub fn start_pull(&mut self) {
        self.popup = Popup::ConfirmPull;
    }

    pub fn confirm_pull(&mut self) {
        self.popup = Popup::None;
        self.status_msg = "Pulling...".to_string();
        match git_ops::pull(&self.repo) {
            Ok(msg) => {
                self.status_msg = msg;
                let _ = self.refresh_all();
            }
            Err(e) => self.status_msg = format!("Pull failed: {}", e),
        }
    }

    pub fn do_fetch(&mut self) {
        self.status_msg = "Fetching...".to_string();
        match git_ops::fetch(&self.repo) {
            Ok(remote) => {
                self.status_msg = format!("Fetched from {}", remote);
                self.refresh_remote_status();
                let _ = self.refresh_commits();
            }
            Err(e) => self.status_msg = format!("Fetch failed: {}", e),
        }
    }

    // ── Branch operations ───────────────────────────────

    pub fn checkout_selected_branch(&mut self) {
        if let Some(branch) = self.branches.get(self.branch_selected) {
            if branch.is_head {
                self.status_msg = "Already on this branch".to_string();
                return;
            }
            if branch.is_remote {
                self.status_msg = "Cannot checkout remote branch directly".to_string();
                return;
            }
            let name = branch.name.clone();
            match git_ops::checkout_branch(&self.repo, &name) {
                Ok(()) => {
                    self.status_msg = format!("Switched to branch: {}", name);
                    let _ = self.refresh_all();
                }
                Err(e) => self.status_msg = format!("Checkout failed: {}", e),
            }
        }
    }

    pub fn start_new_branch(&mut self) {
        self.popup = Popup::NewBranch;
        self.input_mode = InputMode::Editing;
        self.input_buffer.clear();
    }

    pub fn confirm_new_branch(&mut self) {
        let name = self.input_buffer.trim().to_string();
        if name.is_empty() {
            self.status_msg = "Branch name cannot be empty".to_string();
        } else {
            match git_ops::create_branch(&self.repo, &name) {
                Ok(()) => {
                    self.status_msg = format!("Created branch: {}", name);
                    let _ = self.refresh_branches();
                }
                Err(e) => self.status_msg = format!("Failed to create branch: {}", e),
            }
        }
        self.popup = Popup::None;
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
    }

    pub fn start_delete_branch(&mut self) {
        if let Some(branch) = self.branches.get(self.branch_selected) {
            if branch.is_head {
                self.status_msg = "Cannot delete the current branch".to_string();
                return;
            }
            if branch.is_remote {
                self.status_msg = "Cannot delete remote branches from here".to_string();
                return;
            }
            self.popup = Popup::ConfirmDelete(branch.name.clone());
        }
    }

    pub fn confirm_delete_branch(&mut self) {
        if let Popup::ConfirmDelete(ref name) = self.popup {
            let name = name.clone();
            match git_ops::delete_branch(&self.repo, &name) {
                Ok(()) => {
                    self.status_msg = format!("Deleted branch: {}", name);
                    let _ = self.refresh_branches();
                }
                Err(e) => self.status_msg = format!("Delete failed: {}", e),
            }
        }
        self.popup = Popup::None;
    }

    pub fn confirm_popup(&mut self) {
        match &self.popup {
            Popup::ConfirmDelete(_) => self.confirm_delete_branch(),
            Popup::ConfirmPush => self.confirm_push(),
            Popup::ConfirmPull => self.confirm_pull(),
            _ => {}
        }
    }

    pub fn cancel_popup(&mut self) {
        self.popup = Popup::None;
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
    }

    pub fn show_help(&mut self) {
        self.popup = Popup::Help;
    }
}
