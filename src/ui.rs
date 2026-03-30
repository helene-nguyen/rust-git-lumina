use crate::app::{App, ClickAction, ClickRegion, FilePane, InputMode, Popup, Tab};
use crate::git_ops::FileState;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Tabs, Wrap,
    },
    Frame,
};

/// Colors used throughout the UI
mod colors {
    use ratatui::style::Color;
    pub const ACCENT: Color = Color::Cyan;
    pub const BRANCH: Color = Color::Green;
    pub const HEAD: Color = Color::Yellow;
    pub const REMOTE: Color = Color::Red;
    pub const ADDED: Color = Color::Green;
    pub const MODIFIED: Color = Color::Yellow;
    pub const DELETED: Color = Color::Red;
    pub const CONFLICT: Color = Color::Magenta;
    pub const PUSH: Color = Color::Green;
    pub const PULL: Color = Color::Cyan;
    pub const GRAPH_COLORS: [Color; 6] = [
        Color::Cyan,
        Color::Green,
        Color::Magenta,
        Color::Yellow,
        Color::Blue,
        Color::Red,
    ];
}

/// Main draw function — delegates to tab-specific renderers
pub fn draw(f: &mut Frame, app: &mut App) {
    app.clear_click_regions();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // tab bar
            Constraint::Min(0),   // main content
            Constraint::Length(1), // status bar
        ])
        .split(f.area());

    draw_tabs(f, app, chunks[0]);

    match app.tab {
        Tab::Graph => draw_graph_tab(f, app, chunks[1]),
        Tab::Files => draw_files_tab(f, app, chunks[1]),
        Tab::Branches => draw_branches_tab(f, app, chunks[1]),
    }

    draw_status_bar(f, app, chunks[2]);

    // Draw popups on top
    match &app.popup {
        Popup::NewBranch => {
            draw_input_popup(f, "New Branch", "Enter branch name:", &app.input_buffer)
        }
        Popup::ConfirmDelete(name) => {
            draw_confirm_popup(f, "Delete Branch", &format!("Delete '{}'? (y/n)", name))
        }
        Popup::ConfirmPush => {
            let msg = format!(
                "Push to {}? (y/n)\n↑ {} commit(s) ahead",
                app.remote_status
                    .remote_name
                    .as_deref()
                    .unwrap_or("remote"),
                app.remote_status.ahead
            );
            draw_confirm_popup(f, "Push", &msg)
        }
        Popup::ConfirmPull => {
            let msg = format!(
                "Pull from {}? (y/n)\n↓ {} commit(s) behind",
                app.remote_status
                    .remote_name
                    .as_deref()
                    .unwrap_or("remote"),
                app.remote_status.behind
            );
            draw_confirm_popup(f, "Pull", &msg)
        }
        Popup::Help => draw_help_popup(f),
        Popup::None => {}
    }
}

fn draw_tabs(f: &mut Frame, app: &mut App, area: Rect) {
    let ahead = app.remote_status.ahead;
    let behind = app.remote_status.behind;
    let remote_str = format!(" ↑{} ↓{} │ [P]ush [L]ull [F]etch ", ahead, behind);
    let title = format!(" Git Lumina │{}", remote_str);

    let titles: Vec<Line> = Tab::titles().iter().map(|t| Line::from(*t)).collect();
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title(title))
        .select(app.tab.index())
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(colors::ACCENT)
                .add_modifier(Modifier::BOLD),
        );
    f.render_widget(tabs, area);

    // Register tab click regions
    let tab_width = area.width / 3;
    for (i, tab) in [Tab::Graph, Tab::Files, Tab::Branches].iter().enumerate() {
        let r = Rect::new(area.x + 1 + (i as u16 * tab_width), area.y + 1, tab_width, 1);
        app.register_click_region(r, ClickAction::SelectTab(*tab));
    }
}

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let hint = match app.input_mode {
        InputMode::Editing => " ESC: cancel │ Enter: confirm",
        InputMode::Normal => {
            " ?: help │ q: quit │ Tab/↑↓: nav │ P: push │ L: pull │ F: fetch │ Mouse: click/scroll"
        }
    };
    let status = if app.status_msg.is_empty() {
        hint.to_string()
    } else {
        format!(" {} │{}", app.status_msg, hint)
    };
    let bar = Paragraph::new(status).style(Style::default().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(bar, area);
}

// ── Graph Tab ───────────────────────────────────────────

fn draw_graph_tab(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(area);

    draw_commit_list(f, app, chunks[0]);
    draw_commit_detail(f, app, chunks[1]);
}

fn draw_commit_list(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .commits
        .iter()
        .enumerate()
        .map(|(i, commit)| {
            let graph_char = if commit.parents.len() > 1 {
                "●─┐"
            } else {
                "● "
            };
            let color_idx = i % colors::GRAPH_COLORS.len();
            let graph_color = colors::GRAPH_COLORS[color_idx];

            let branch_spans: Vec<Span> = commit
                .branches
                .iter()
                .map(|b| {
                    Span::styled(
                        format!(" [{}]", b),
                        Style::default()
                            .fg(if commit.is_head {
                                colors::HEAD
                            } else {
                                colors::BRANCH
                            })
                            .add_modifier(Modifier::BOLD),
                    )
                })
                .collect();

            let mut spans = vec![
                Span::styled(graph_char, Style::default().fg(graph_color)),
                Span::styled(
                    format!(" {} ", commit.id),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                ),
            ];
            spans.extend(branch_spans);
            spans.push(Span::styled(
                format!(" {}", commit.message),
                Style::default().fg(Color::White),
            ));

            let style = if i == app.commit_selected {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Commit Graph "),
    );
    f.render_widget(list, area);

    // Register click regions for visible commit rows
    let inner = inner_rect(area);
    for i in 0..app.commits.len().min(inner.height as usize) {
        let row = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
        let idx = app.commit_scroll + i;
        if idx < app.commits.len() {
            app.register_click_region(row, ClickAction::SelectCommit(idx));
        }
    }
}

fn draw_commit_detail(f: &mut Frame, app: &App, area: Rect) {
    let detail = if let Some(commit) = app.commits.get(app.commit_selected) {
        let time_str = chrono::DateTime::from_timestamp(commit.time, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let branches_str = if commit.branches.is_empty() {
            String::new()
        } else {
            format!("\nBranches: {}", commit.branches.join(", "))
        };

        format!(
            "Commit: {}\nAuthor: {}\nDate:   {}\n{}\n\n{}",
            commit.id_full, commit.author, time_str, branches_str, commit.message
        )
    } else {
        "No commit selected".to_string()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Details ");
    let paragraph = Paragraph::new(detail)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

// ── Files Tab ───────────────────────────────────────────

fn draw_files_tab(f: &mut Frame, app: &mut App, area: Rect) {
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Percentage(40),
            Constraint::Length(5),
        ])
        .split(main_chunks[0]);

    draw_file_list(
        f,
        &mut app.click_regions,
        "Unstaged Changes",
        &app.unstaged_files,
        app.unstaged_selected,
        app.file_pane == FilePane::Unstaged,
        left_chunks[0],
        false,
    );
    draw_file_list(
        f,
        &mut app.click_regions,
        "Staged Changes",
        &app.staged_files,
        app.staged_selected,
        app.file_pane == FilePane::Staged,
        left_chunks[1],
        true,
    );
    draw_commit_input(f, app, left_chunks[2]);
    draw_diff_view(f, app, main_chunks[1]);
}

fn draw_file_list(
    f: &mut Frame,
    click_regions: &mut Vec<ClickRegion>,
    title: &str,
    files: &[crate::git_ops::FileStatus],
    selected: usize,
    focused: bool,
    area: Rect,
    is_staged: bool,
) {
    let items: Vec<ListItem> = files
        .iter()
        .enumerate()
        .map(|(i, file)| {
            let (icon, color) = match file.status {
                FileState::New => ("+", colors::ADDED),
                FileState::Modified => ("~", colors::MODIFIED),
                FileState::Deleted => ("-", colors::DELETED),
                FileState::Renamed => ("→", colors::MODIFIED),
                FileState::Typechange => ("T", colors::MODIFIED),
                FileState::Conflicted => ("!", colors::CONFLICT),
            };

            let style = if i == selected && focused {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", icon), Style::default().fg(color)),
                Span::styled(&file.path, Style::default().fg(Color::White)),
            ]))
            .style(style)
        })
        .collect();

    let border_style = if focused {
        Style::default().fg(colors::ACCENT)
    } else {
        Style::default()
    };

    let action_hint = if is_staged {
        " S-all/u"
    } else {
        " s/S-all"
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(format!(" {} ({}) {} ", title, files.len(), action_hint));

    let list = List::new(items).block(block);
    f.render_widget(list, area);

    // Register click regions
    let inner = inner_rect(area);
    for i in 0..files.len().min(inner.height as usize) {
        let row = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
        let action = if is_staged {
            ClickAction::SelectStagedFile(i)
        } else {
            ClickAction::SelectUnstagedFile(i)
        };
        click_regions.push(ClickRegion { rect: row, action });
    }
}

fn draw_commit_input(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.file_pane == FilePane::CommitMsg;
    let border_style = if focused {
        Style::default().fg(colors::ACCENT)
    } else {
        Style::default()
    };

    let hint = if app.staged_files.is_empty() {
        " (nothing staged)"
    } else if focused && app.input_mode == InputMode::Editing {
        " (Enter: commit, Esc: cancel)"
    } else {
        " ('c' to type)"
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(format!(" Commit Message{} ", hint));

    let display_text = if app.commit_message.is_empty() && !focused {
        "Type a commit message...".to_string()
    } else {
        app.commit_message.clone()
    };

    let style = if app.commit_message.is_empty() && !focused {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };

    let paragraph = Paragraph::new(display_text).style(style).block(block);
    f.render_widget(paragraph, area);

    app.register_click_region(area, ClickAction::FocusCommitMsg);

    if focused && app.input_mode == InputMode::Editing {
        let x = area.x + 1 + app.commit_message.len() as u16;
        let y = area.y + 1;
        f.set_cursor_position((x, y));
    }
}

fn draw_diff_view(f: &mut Frame, app: &mut App, area: Rect) {
    let all_lines: Vec<Line> = app
        .diff_text
        .lines()
        .map(|line| {
            let style = if line.starts_with('+') {
                Style::default().fg(colors::ADDED)
            } else if line.starts_with('-') {
                Style::default().fg(colors::DELETED)
            } else if line.starts_with("@@") {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };
            Line::styled(line, style)
        })
        .collect();

    let total = all_lines.len();
    let visible = area.height.saturating_sub(2) as usize;
    let max_scroll = total.saturating_sub(visible);
    let scroll = app.diff_scroll.min(max_scroll);

    // Scroll percentage in title
    let scroll_info = if total > visible {
        let pct = if max_scroll > 0 {
            (scroll * 100) / max_scroll
        } else {
            0
        };
        format!(" [{}/{}  {}%] ", scroll + 1, total, pct)
    } else {
        String::new()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Diff Preview{} ", scroll_info));

    let paragraph = Paragraph::new(all_lines)
        .block(block)
        .scroll((scroll as u16, 0));
    f.render_widget(paragraph, area);

    // Scrollbar
    if total > visible {
        let mut state = ScrollbarState::new(total)
            .position(scroll)
            .viewport_content_length(visible);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"));
        let sb_area = Rect::new(
            area.x + area.width - 1,
            area.y + 1,
            1,
            area.height.saturating_sub(2),
        );
        f.render_stateful_widget(scrollbar, sb_area, &mut state);
    }

    app.diff_area = Some(area);
}

// ── Branches Tab ────────────────────────────────────────

fn draw_branches_tab(f: &mut Frame, app: &mut App, area: Rect) {
    let items: Vec<ListItem> = app
        .branches
        .iter()
        .enumerate()
        .map(|(i, branch)| {
            let icon = if branch.is_head {
                "→"
            } else if branch.is_remote {
                "☁"
            } else {
                " "
            };

            let name_color = if branch.is_head {
                colors::HEAD
            } else if branch.is_remote {
                colors::REMOTE
            } else {
                colors::BRANCH
            };

            let upstream_str = branch
                .upstream
                .as_ref()
                .map(|u| format!(" ↔ {}", u))
                .unwrap_or_default();

            let style = if i == app.branch_selected {
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", icon),
                    Style::default().fg(name_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(&branch.name, Style::default().fg(name_color)),
                Span::styled(
                    format!("  {}", branch.commit_id),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(upstream_str, Style::default().fg(Color::DarkGray)),
            ]))
            .style(style)
        })
        .collect();

    let block = Block::default().borders(Borders::ALL).title(
        " Branches  (Enter: checkout │ n: new │ d: delete │ P: push │ L: pull) ",
    );
    let list = List::new(items).block(block);
    f.render_widget(list, area);

    let inner = inner_rect(area);
    for i in 0..app.branches.len().min(inner.height as usize) {
        let row = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
        app.register_click_region(row, ClickAction::SelectBranch(i));
    }
}

// ── Popups ──────────────────────────────────────────────

fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length((r.height.saturating_sub(height)) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn draw_input_popup(f: &mut Frame, title: &str, prompt: &str, input: &str) {
    let area = centered_rect(50, 5, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::ACCENT))
        .title(format!(" {} ", title));

    let text = format!("{}\n> {}", prompt, input);
    let paragraph = Paragraph::new(text)
        .block(block)
        .style(Style::default().fg(Color::White));
    f.render_widget(paragraph, area);

    let x = area.x + 3 + input.len() as u16;
    let y = area.y + 2;
    f.set_cursor_position((x, y));
}

fn draw_confirm_popup(f: &mut Frame, title: &str, message: &str) {
    let line_count = message.lines().count() as u16;
    let area = centered_rect(50, line_count + 3, f.area());
    f.render_widget(Clear, area);

    let border_color = match title {
        "Push" => colors::PUSH,
        "Pull" => colors::PULL,
        _ => Color::Red,
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(format!(" {} ", title));

    let paragraph = Paragraph::new(message)
        .block(block)
        .style(Style::default().fg(Color::White));
    f.render_widget(paragraph, area);
}

fn draw_help_popup(f: &mut Frame) {
    let area = centered_rect(60, 24, f.area());
    f.render_widget(Clear, area);

    let help_text = "\
 Navigation
   1/2/3 or Tab     Switch tabs
   ↑/↓ or j/k       Move selection
   Mouse click       Select items, switch tabs
   q                 Quit

 Files Tab
   Tab               Cycle: Unstaged → Staged → Commit
   s / S             Stage selected / Stage all
   u / U             Unstage selected / Unstage all
   c                 Start typing commit message
   Enter             Commit (when in commit message)

 Branches Tab
   Enter             Checkout selected branch
   n                 Create new branch
   d                 Delete selected branch

 Remote Operations
   P                 Push to remote
   L                 Pull from remote (fast-forward)
   F                 Fetch from remote

 Diff View
   Mouse scroll      Scroll diff up/down
   PgUp / PgDn       Scroll diff by page

 General
   r                 Refresh all data
   ?                 Toggle this help";

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::ACCENT))
        .title(" Help ");

    let paragraph = Paragraph::new(help_text)
        .block(block)
        .style(Style::default().fg(Color::White));
    f.render_widget(paragraph, area);
}

// ── Helpers ─────────────────────────────────────────────

fn inner_rect(area: Rect) -> Rect {
    Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}