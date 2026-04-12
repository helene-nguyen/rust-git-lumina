use crate::app::{App, ClickAction, ClickRegion, FilePane, InputMode, Popup, Tab};
use crate::git_ops::FileState;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
    Frame,
};

// ── Dark purple/violet theme ───────────────────────────────

mod colors {
    use ratatui::style::Color;

    // Backgrounds
    pub const BG: Color = Color::Rgb(10, 7, 20);
    pub const BG_PANEL: Color = Color::Rgb(15, 11, 30);
    pub const BG_SURFACE: Color = Color::Rgb(20, 15, 40);
    pub const BG_SELECT: Color = Color::Rgb(45, 30, 75);
    pub const BG_TITLE: Color = Color::Rgb(19, 14, 40);
    pub const BG_TAB: Color = Color::Rgb(12, 8, 25);
    pub const BG_STATUS: Color = Color::Rgb(8, 6, 20);

    // Borders
    pub const BORDER: Color = Color::Rgb(28, 20, 53);
    pub const BORDER_FOCUS: Color = Color::Rgb(124, 58, 237);

    // Text
    pub const TEXT: Color = Color::Rgb(224, 218, 244);
    pub const TEXT_SECONDARY: Color = Color::Rgb(155, 143, 192);
    pub const TEXT_MUTED: Color = Color::Rgb(90, 77, 128);
    pub const TEXT_DIM: Color = Color::Rgb(58, 46, 92);

    // Accent (violet)
    pub const ACCENT: Color = Color::Rgb(167, 139, 250);
    pub const ACCENT_BRIGHT: Color = Color::Rgb(196, 181, 253);
    pub const ACCENT_GLOW: Color = Color::Rgb(139, 92, 246);

    // Semantic
    pub const GREEN: Color = Color::Rgb(134, 239, 172);
    pub const YELLOW: Color = Color::Rgb(253, 230, 138);
    pub const RED: Color = Color::Rgb(252, 165, 165);
    pub const CYAN: Color = Color::Rgb(165, 243, 252);
    pub const PINK: Color = Color::Rgb(249, 168, 212);
    pub const ORANGE: Color = Color::Rgb(253, 186, 116);

    // Badge backgrounds (pre-blended with dark bg)
    pub const GREEN_BG: Color = Color::Rgb(18, 32, 25);
    pub const YELLOW_BG: Color = Color::Rgb(35, 30, 18);
    pub const ACCENT_BG: Color = Color::Rgb(22, 16, 42);

    // Graph lane colors
    pub const GRAPH_COLORS: [Color; 6] = [ACCENT, CYAN, PINK, ORANGE, GREEN, YELLOW];
}

// ── Helpers ────────────────────────────────────────────────

fn relative_time(timestamp: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let diff = now - timestamp;
    if diff < 0 {
        return "just now".to_string();
    }
    let diff = diff as u64;
    match diff {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", diff / 60),
        3600..=86399 => {
            let h = diff / 3600;
            format!("{} hr{} ago", h, if h == 1 { "" } else { "s" })
        }
        86400..=604799 => {
            let d = diff / 86400;
            format!("{} day{} ago", d, if d == 1 { "" } else { "s" })
        }
        _ => {
            let w = diff / 604800;
            format!("{} wk{} ago", w, if w == 1 { "" } else { "s" })
        }
    }
}

fn inner_rect(area: Rect) -> Rect {
    Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}

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

fn styled_block<'a>(title: &str, focused: bool) -> Block<'a> {
    let border_color = if focused {
        colors::BORDER_FOCUS
    } else {
        colors::BORDER
    };
    let header_fg = if focused {
        colors::ACCENT_BRIGHT
    } else {
        colors::TEXT_MUTED
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(header_fg)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(colors::BG_PANEL))
}

// ── Main draw ──────────────────────────────────────────────

pub fn draw(f: &mut Frame, app: &mut App) {
    app.clear_click_regions();

    // Fill entire background
    let bg = Block::default().style(Style::default().bg(colors::BG));
    f.render_widget(bg, f.area());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // title bar (1 + padding)
            Constraint::Length(3), // tab bar (1 + padding)
            Constraint::Min(0),   // content
            Constraint::Length(1), // status bar
        ])
        .split(f.area());

    draw_title_bar(f, app, chunks[0]);
    draw_tab_bar(f, app, chunks[1]);

    match app.tab {
        Tab::Graph => draw_graph_tab(f, app, chunks[2]),
        Tab::Files => draw_files_tab(f, app, chunks[2]),
        Tab::Branches => draw_branches_tab(f, app, chunks[2]),
    }

    draw_status_bar(f, app, chunks[3]);

    // Popups on top
    match &app.popup {
        Popup::NewBranch => {
            draw_input_popup(f, "New Branch", "Enter branch name:", &app.input_buffer)
        }
        Popup::ConfirmDelete(name) => {
            draw_confirm_popup(f, "Delete Branch", &format!("Delete '{}'? (y/n)", name))
        }
        Popup::ConfirmPush => {
            let msg = format!(
                "Push to {}?\n\n  ↑ {} commit(s) ahead\n\n  [Y] Confirm    [N] Cancel",
                app.remote_status
                    .remote_name
                    .as_deref()
                    .unwrap_or("remote"),
                app.remote_status.ahead
            );
            draw_confirm_popup(f, "↑ Push", &msg)
        }
        Popup::ConfirmPull => {
            let msg = format!(
                "Pull from {}?\n\n  ↓ {} commit(s) behind\n\n  [Y] Confirm    [N] Cancel",
                app.remote_status
                    .remote_name
                    .as_deref()
                    .unwrap_or("remote"),
                app.remote_status.behind
            );
            draw_confirm_popup(f, "↓ Pull", &msg)
        }
        Popup::Help => draw_help_popup(f),
        Popup::None => {}
    }
}

// ── Title Bar ──────────────────────────────────────────────

fn draw_title_bar(f: &mut Frame, app: &mut App, area: Rect) {
    // Fill background for entire area
    let bg_block = Block::default().style(Style::default().bg(colors::BG_TITLE));
    f.render_widget(bg_block, area);

    // Center content vertically
    let row = Rect::new(area.x, area.y + area.height / 2, area.width, 1);

    let branch = &app.current_branch;
    let ahead = app.remote_status.ahead;
    let behind = app.remote_status.behind;

    let left: Vec<Span> = vec![
        Span::styled(" ⎇ ", Style::default().fg(colors::ACCENT_BRIGHT).add_modifier(Modifier::BOLD)),
        Span::styled("Git", Style::default().fg(colors::TEXT).add_modifier(Modifier::BOLD)),
        Span::styled("Lumina", Style::default().fg(colors::ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled("  ", Style::default()),
        Span::styled("● ", Style::default().fg(colors::ACCENT_GLOW)),
        Span::styled(branch.as_str(), Style::default().fg(colors::TEXT)),
    ];

    let right: Vec<Span> = vec![
        Span::styled(format!("↑{}", ahead), Style::default().fg(colors::GREEN).add_modifier(Modifier::BOLD)),
        Span::styled(" Push ", Style::default().fg(colors::TEXT_MUTED)),
        Span::styled(format!("↓{}", behind), Style::default().fg(colors::CYAN).add_modifier(Modifier::BOLD)),
        Span::styled(" Pull ", Style::default().fg(colors::TEXT_MUTED)),
        Span::styled("⟳", Style::default().fg(colors::ACCENT)),
        Span::styled(" Fetch ", Style::default().fg(colors::TEXT_MUTED)),
    ];

    let left_width: usize = left.iter().map(|s| s.content.len()).sum();
    let right_width: usize = right.iter().map(|s| s.content.len()).sum();
    let pad = (row.width as usize).saturating_sub(left_width + right_width);

    let mut spans = left;
    spans.push(Span::raw(" ".repeat(pad)));
    spans.extend(right);

    let bar = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(colors::BG_TITLE));
    f.render_widget(bar, row);

    // Click regions for Push, Pull, Fetch buttons
    let push_x = row.x + row.width.saturating_sub(right_width as u16);
    app.register_click_region(
        Rect::new(push_x, row.y, 7, 1),
        ClickAction::PushButton,
    );
    app.register_click_region(
        Rect::new(push_x + 7, row.y, 7, 1),
        ClickAction::PullButton,
    );
    app.register_click_region(
        Rect::new(push_x + 14, row.y, 8, 1),
        ClickAction::FetchButton,
    );
}

// ── Tab Bar ────────────────────────────────────────────────

fn draw_tab_bar(f: &mut Frame, app: &mut App, area: Rect) {
    // Fill background for entire area
    let bg_block = Block::default().style(Style::default().bg(colors::BG_TAB));
    f.render_widget(bg_block, area);

    // Center content vertically
    let row = Rect::new(area.x, area.y + area.height / 2, area.width, 1);

    let tab_names = ["1 Graph", "2 Files", "3 Branches"];
    let mut spans: Vec<Span> = Vec::new();
    spans.push(Span::raw(" "));

    for (i, name) in tab_names.iter().enumerate() {
        let is_active = app.tab.index() == i;
        if is_active {
            spans.push(Span::styled(
                format!(" {} ", name),
                Style::default()
                    .fg(colors::ACCENT_BRIGHT)
                    .bg(colors::BG_SURFACE)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                format!(" {} ", name),
                Style::default().fg(colors::TEXT_MUTED),
            ));
        }
        spans.push(Span::raw("  "));
    }

    // Right side: Refresh button
    let used: usize = spans.iter().map(|s| s.content.len()).sum();
    let refresh_text = "⟳ Refresh ";
    let pad = (row.width as usize).saturating_sub(used + refresh_text.len());
    spans.push(Span::raw(" ".repeat(pad)));
    spans.push(Span::styled(refresh_text, Style::default().fg(colors::TEXT_MUTED)));

    let bar = Paragraph::new(Line::from(spans))
        .style(Style::default().bg(colors::BG_TAB));
    f.render_widget(bar, row);

    // Tab click regions
    let mut x = row.x + 1;
    for (i, name) in tab_names.iter().enumerate() {
        let w = name.len() as u16 + 2; // padding
        let tab = match i {
            0 => Tab::Graph,
            1 => Tab::Files,
            _ => Tab::Branches,
        };
        app.register_click_region(Rect::new(x, row.y, w, 1), ClickAction::SelectTab(tab));
        x += w + 2;
    }

    // Refresh click region
    app.register_click_region(
        Rect::new(row.x + row.width - refresh_text.len() as u16, row.y, refresh_text.len() as u16, 1),
        ClickAction::RefreshButton,
    );
}

// ── Status Bar ─────────────────────────────────────────────

fn draw_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let hints = match app.input_mode {
        InputMode::Editing => "ESC cancel │ Enter confirm",
        InputMode::Normal => "? help │ q quit │ P push │ L pull │ F fetch │ ↑↓ nav",
    };

    let auto_tag = if app.auto_refresh {
        Span::styled(" [AUTO] ", Style::default().fg(colors::GREEN))
    } else {
        Span::raw("")
    };

    let left = if app.status_msg.is_empty() {
        Span::styled(" Ready", Style::default().fg(colors::TEXT_MUTED))
    } else {
        Span::styled(format!(" {}", app.status_msg), Style::default().fg(colors::ACCENT_BRIGHT))
    };

    let right = Span::styled(format!("{} ", hints), Style::default().fg(colors::TEXT_DIM));

    let auto_w = auto_tag.content.len();
    let left_w = left.content.len();
    let right_w = right.content.len();
    let pad = (area.width as usize).saturating_sub(auto_w + left_w + right_w);

    let line = Line::from(vec![left, auto_tag, Span::raw(" ".repeat(pad)), right]);
    let bar = Paragraph::new(line).style(Style::default().bg(colors::BG_STATUS));
    f.render_widget(bar, area);
}

// ── Graph Tab ──────────────────────────────────────────────

fn draw_graph_tab(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(area);

    draw_commit_list(f, app, chunks[0]);
    draw_commit_detail(f, app, chunks[1]);
}

fn draw_commit_list(f: &mut Frame, app: &mut App, area: Rect) {
    let max_lanes = app
        .graph_lanes
        .iter()
        .map(|g| g.active_lanes.len().max(g.lane + 1))
        .max()
        .unwrap_or(1)
        .min(6); // cap at 6 lanes for display

    let items: Vec<ListItem> = app
        .commits
        .iter()
        .enumerate()
        .map(|(i, commit)| {
            let sel = i == app.commit_selected;
            let lane_info = app.graph_lanes.get(i);

            // Build graph prefix
            let mut graph_spans: Vec<Span> = Vec::new();
            if let Some(info) = lane_info {
                let my_lane = info.lane.min(max_lanes - 1);
                let merge_to = info.merge_from.map(|m| m.min(max_lanes - 1));

                for col in 0..max_lanes {
                    let col_color = colors::GRAPH_COLORS[col % colors::GRAPH_COLORS.len()];
                    let my_color = colors::GRAPH_COLORS[my_lane % colors::GRAPH_COLORS.len()];
                    let is_active = col < info.active_lanes.len() && info.active_lanes[col];

                    if col == my_lane {
                        if let Some(mt) = merge_to {
                            if mt > my_lane {
                                graph_spans.push(Span::styled(
                                    "●─",
                                    Style::default().fg(my_color),
                                ));
                            } else {
                                graph_spans.push(Span::styled(
                                    "● ",
                                    Style::default().fg(my_color),
                                ));
                            }
                        } else {
                            let node = if sel { "◉ " } else { "● " };
                            graph_spans.push(Span::styled(
                                node,
                                Style::default().fg(my_color),
                            ));
                        }
                    } else if let Some(mt) = merge_to {
                        if col > my_lane && col < mt {
                            graph_spans.push(Span::styled(
                                "──",
                                Style::default().fg(colors::GRAPH_COLORS[my_lane % colors::GRAPH_COLORS.len()]),
                            ));
                        } else if col == mt {
                            graph_spans.push(Span::styled(
                                "┐ ",
                                Style::default().fg(col_color),
                            ));
                        } else if is_active {
                            graph_spans.push(Span::styled(
                                "│ ",
                                Style::default().fg(col_color).add_modifier(Modifier::DIM),
                            ));
                        } else {
                            graph_spans.push(Span::raw("  "));
                        }
                    } else if is_active {
                        graph_spans.push(Span::styled(
                            "│ ",
                            Style::default().fg(col_color).add_modifier(Modifier::DIM),
                        ));
                    } else {
                        graph_spans.push(Span::raw("  "));
                    }
                }
            } else {
                graph_spans.push(Span::styled("● ", Style::default().fg(colors::ACCENT)));
                for _ in 1..max_lanes {
                    graph_spans.push(Span::raw("  "));
                }
            }

            // SHA
            let sha_color = if sel { colors::ACCENT } else { colors::TEXT_DIM };
            graph_spans.push(Span::styled(
                format!("{} ", commit.id),
                Style::default().fg(sha_color),
            ));

            // Branch badges
            for b in &commit.branches {
                let is_origin = b.starts_with("origin/");
                let (fg, bg) = if commit.is_head {
                    (colors::YELLOW, colors::YELLOW_BG)
                } else if is_origin {
                    (colors::ACCENT_BRIGHT, colors::ACCENT_BG)
                } else {
                    (colors::GREEN, colors::GREEN_BG)
                };
                let prefix = if commit.is_head { "● " } else { "" };
                graph_spans.push(Span::styled(
                    format!(" {}{} ", prefix, b),
                    Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
                ));
                graph_spans.push(Span::raw(" "));
            }

            // Commit message
            let msg_color = if sel { colors::TEXT } else { colors::TEXT_SECONDARY };
            graph_spans.push(Span::styled(
                &commit.message,
                Style::default().fg(msg_color),
            ));

            // Right side: author · time
            let time_str = relative_time(commit.time);
            let author_short = commit.author.split(' ').next().unwrap_or(&commit.author);
            graph_spans.push(Span::styled(
                format!("  {} · {}", author_short, time_str),
                Style::default().fg(colors::TEXT_DIM),
            ));

            let row_style = if sel {
                Style::default().bg(colors::BG_SELECT)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(graph_spans)).style(row_style)
        })
        .collect();

    let title = format!("Commit Graph ({})", app.commits.len());
    let block = styled_block(&title, true);
    let list = List::new(items).block(block);
    f.render_widget(list, area);

    // Click regions
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
    let block = styled_block("Details", false);

    if let Some(commit) = app.commits.get(app.commit_selected) {
        let time_str = chrono::DateTime::from_timestamp(commit.time, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let rel = relative_time(commit.time);

        let mut lines: Vec<Line> = Vec::new();

        // Commit
        lines.push(Line::from(Span::styled(
            "COMMIT",
            Style::default().fg(colors::TEXT_DIM).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            format!("  {}", commit.id_full),
            Style::default().fg(colors::TEXT),
        )));
        lines.push(Line::raw(""));

        // Author
        lines.push(Line::from(Span::styled(
            "AUTHOR",
            Style::default().fg(colors::TEXT_DIM).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            format!("  {}", commit.author),
            Style::default().fg(colors::TEXT),
        )));
        lines.push(Line::raw(""));

        // Date
        lines.push(Line::from(Span::styled(
            "DATE",
            Style::default().fg(colors::TEXT_DIM).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            format!("  {} ({})", time_str, rel),
            Style::default().fg(colors::TEXT),
        )));
        lines.push(Line::raw(""));

        // Branches
        if !commit.branches.is_empty() {
            lines.push(Line::from(Span::styled(
                "BRANCHES",
                Style::default().fg(colors::TEXT_DIM).add_modifier(Modifier::BOLD),
            )));
            let badge_spans: Vec<Span> = std::iter::once(Span::raw("  "))
                .chain(commit.branches.iter().flat_map(|b| {
                    let is_origin = b.starts_with("origin/");
                    let (fg, bg) = if is_origin {
                        (colors::ACCENT_BRIGHT, colors::ACCENT_BG)
                    } else {
                        (colors::GREEN, colors::GREEN_BG)
                    };
                    vec![
                        Span::styled(
                            format!(" {} ", b),
                            Style::default().fg(fg).bg(bg),
                        ),
                        Span::raw(" "),
                    ]
                }))
                .collect();
            lines.push(Line::from(badge_spans));
            lines.push(Line::raw(""));
        }

        // Message
        lines.push(Line::from(Span::styled(
            "MESSAGE",
            Style::default().fg(colors::TEXT_DIM).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            format!("  {}", commit.message),
            Style::default().fg(colors::TEXT).add_modifier(Modifier::BOLD),
        )));

        let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
        f.render_widget(paragraph, area);
    } else {
        let paragraph = Paragraph::new(Span::styled(
            "  No commit selected",
            Style::default().fg(colors::TEXT_MUTED),
        ))
        .block(block);
        f.render_widget(paragraph, area);
    }
}

// ── Files Tab ──────────────────────────────────────────────

fn draw_files_tab(f: &mut Frame, app: &mut App, area: Rect) {
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(38),
            Constraint::Percentage(38),
            Constraint::Length(6),
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
                FileState::New => ("+", colors::GREEN),
                FileState::Modified => ("~", colors::YELLOW),
                FileState::Deleted => ("-", colors::RED),
                FileState::Renamed => ("→", colors::YELLOW),
                FileState::Typechange => ("T", colors::YELLOW),
                FileState::Conflicted => ("!", colors::PINK),
            };

            let style = if i == selected && focused {
                Style::default().bg(colors::BG_SELECT)
            } else {
                Style::default()
            };

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", icon),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    &file.path,
                    Style::default().fg(if i == selected && focused {
                        colors::TEXT
                    } else {
                        colors::TEXT_SECONDARY
                    }),
                ),
            ]))
            .style(style)
        })
        .collect();

    let btn_label = if is_staged { "Unstage All ↑" } else { "Stage All ↓" };
    let btn_color = if is_staged { colors::YELLOW } else { colors::GREEN };
    let header = format!("{} ({})", title, files.len());

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused {
            colors::BORDER_FOCUS
        } else {
            colors::BORDER
        }))
        .title(Line::from(vec![
            Span::styled(
                format!(" {} ", header),
                Style::default()
                    .fg(if focused { colors::ACCENT_BRIGHT } else { colors::TEXT_MUTED })
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .title_bottom(Line::from(Span::styled(
            format!(" {} ", btn_label),
            Style::default().fg(btn_color),
        )))
        .style(Style::default().bg(colors::BG_PANEL));

    let list = List::new(items).block(block);
    f.render_widget(list, area);

    // Click regions for file items
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

    // Click region for bottom button
    let btn_action = if is_staged {
        ClickAction::UnstageAllButton
    } else {
        ClickAction::StageAllButton
    };
    click_regions.push(ClickRegion {
        rect: Rect::new(area.x, area.y + area.height - 1, area.width, 1),
        action: btn_action,
    });
}

fn draw_commit_input(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.file_pane == FilePane::CommitMsg;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused {
            colors::BORDER_FOCUS
        } else {
            colors::BORDER
        }))
        .title(Span::styled(
            " Commit Message ",
            Style::default()
                .fg(if focused { colors::ACCENT_BRIGHT } else { colors::TEXT_MUTED })
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(colors::BG_PANEL));

    let display_text = if app.commit_message.is_empty() && !focused {
        "Write a commit message..."
    } else {
        &app.commit_message
    };

    let text_style = if app.commit_message.is_empty() && !focused {
        Style::default().fg(colors::TEXT_DIM)
    } else {
        Style::default().fg(colors::TEXT)
    };

    // Build content: message + button
    let n = app.staged_files.len();
    let can_commit = n > 0 && !app.commit_message.is_empty();
    let btn_text = format!(" Commit {} file{} ", n, if n == 1 { "" } else { "s" });
    let btn_style = if can_commit {
        Style::default()
            .fg(colors::TEXT)
            .bg(colors::ACCENT_GLOW)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(colors::TEXT_DIM).bg(colors::BG_SURFACE)
    };

    let lines = vec![
        Line::from(Span::styled(display_text, text_style)),
        Line::raw(""),
        Line::from(Span::styled(btn_text, btn_style)),
    ];

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);

    app.register_click_region(area, ClickAction::FocusCommitMsg);

    // Click region for the commit button (last line of inner area)
    let inner = inner_rect(area);
    if inner.height >= 3 {
        app.register_click_region(
            Rect::new(inner.x, inner.y + 2, inner.width, 1),
            ClickAction::CommitButton,
        );
    }

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
            let (fg, bg) = if line.starts_with('+') {
                (colors::GREEN, Color::Rgb(15, 25, 18))
            } else if line.starts_with('-') {
                (colors::RED, Color::Rgb(30, 15, 15))
            } else if line.starts_with("@@") {
                (colors::ACCENT, Color::Rgb(18, 14, 32))
            } else {
                (colors::TEXT_SECONDARY, colors::BG_PANEL)
            };
            Line::styled(line, Style::default().fg(fg).bg(bg))
        })
        .collect();

    let total = all_lines.len();
    let visible = area.height.saturating_sub(2) as usize;
    let max_scroll = total.saturating_sub(visible);
    let scroll = app.diff_scroll.min(max_scroll);

    let scroll_info = if total > visible {
        let pct = if max_scroll > 0 {
            (scroll * 100) / max_scroll
        } else {
            0
        };
        format!(" [{}/{}  {}%]", scroll + 1, total, pct)
    } else {
        String::new()
    };

    let title = format!("Diff Preview{}", scroll_info);
    let block = styled_block(&title, false);

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
            .end_symbol(Some("▼"))
            .style(Style::default().fg(colors::ACCENT_GLOW));
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

// ── Branches Tab ───────────────────────────────────────────

fn draw_branches_tab(f: &mut Frame, app: &mut App, area: Rect) {
    let local_branches: Vec<(usize, &crate::git_ops::BranchInfo)> = app
        .branches
        .iter()
        .enumerate()
        .filter(|(_, b)| !b.is_remote)
        .collect();
    let remote_branches: Vec<(usize, &crate::git_ops::BranchInfo)> = app
        .branches
        .iter()
        .enumerate()
        .filter(|(_, b)| b.is_remote)
        .collect();

    let mut items: Vec<ListItem> = Vec::new();
    let mut row_to_branch: Vec<Option<usize>> = Vec::new();

    // Local header
    items.push(ListItem::new(Line::from(Span::styled(
        "  LOCAL",
        Style::default()
            .fg(colors::TEXT_DIM)
            .add_modifier(Modifier::BOLD),
    ))));
    row_to_branch.push(None);

    for (idx, branch) in &local_branches {
        let sel = *idx == app.branch_selected;
        let icon = if branch.is_head { "→" } else { "⎇" };
        let name_color = if branch.is_head {
            colors::YELLOW
        } else {
            colors::GREEN
        };

        let mut spans = vec![
            Span::styled(
                format!("  {} ", icon),
                Style::default().fg(name_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                &branch.name,
                Style::default().fg(name_color).add_modifier(if branch.is_head {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
            ),
            Span::styled(
                format!("  {}", branch.commit_id),
                Style::default().fg(colors::TEXT_DIM),
            ),
        ];
        if let Some(u) = &branch.upstream {
            spans.push(Span::styled(
                format!("  ↔ {}", u),
                Style::default().fg(colors::TEXT_DIM),
            ));
        }

        let style = if sel {
            Style::default().bg(colors::BG_SELECT)
        } else {
            Style::default()
        };
        items.push(ListItem::new(Line::from(spans)).style(style));
        row_to_branch.push(Some(*idx));
    }

    // Remote header
    items.push(ListItem::new(Line::from(Span::styled(
        "  REMOTE",
        Style::default()
            .fg(colors::TEXT_DIM)
            .add_modifier(Modifier::BOLD),
    ))));
    row_to_branch.push(None);

    for (idx, branch) in &remote_branches {
        let sel = *idx == app.branch_selected;
        let spans = vec![
            Span::styled(
                "  ☁ ",
                Style::default()
                    .fg(colors::CYAN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(&branch.name, Style::default().fg(colors::ACCENT_BRIGHT)),
            Span::styled(
                format!("  {}", branch.commit_id),
                Style::default().fg(colors::TEXT_DIM),
            ),
        ];

        let style = if sel {
            Style::default().bg(colors::BG_SELECT)
        } else {
            Style::default()
        };
        items.push(ListItem::new(Line::from(spans)).style(style));
        row_to_branch.push(Some(*idx));
    }

    let title = format!("Branches ({})", app.branches.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::BORDER_FOCUS))
        .title(Line::from(vec![
            Span::styled(
                format!(" {} ", title),
                Style::default()
                    .fg(colors::ACCENT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .title_bottom(Line::from(Span::styled(
            " Enter: checkout │ n: new │ d: delete ",
            Style::default().fg(colors::TEXT_DIM),
        )))
        .style(Style::default().bg(colors::BG_PANEL));

    // Limit width
    let branch_area = Rect::new(area.x, area.y, area.width.min(80), area.height);
    let list = List::new(items).block(block);
    f.render_widget(list, branch_area);

    // Click regions mapped through row_to_branch
    let inner = inner_rect(branch_area);
    for i in 0..row_to_branch.len().min(inner.height as usize) {
        if let Some(branch_idx) = row_to_branch[i] {
            let row = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
            app.register_click_region(row, ClickAction::SelectBranch(branch_idx));
        }
    }
}

// ── Popups ─────────────────────────────────────────────────

fn draw_input_popup(f: &mut Frame, title: &str, prompt: &str, input: &str) {
    let area = centered_rect(50, 6, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::ACCENT))
        .title(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(colors::ACCENT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(colors::BG_SURFACE));

    let lines = vec![
        Line::from(Span::styled(prompt, Style::default().fg(colors::TEXT_SECONDARY))),
        Line::raw(""),
        Line::from(vec![
            Span::styled("  > ", Style::default().fg(colors::ACCENT)),
            Span::styled(input, Style::default().fg(colors::TEXT)),
        ]),
    ];

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);

    let x = area.x + 5 + input.len() as u16;
    let y = area.y + 3;
    f.set_cursor_position((x, y));
}

fn draw_confirm_popup(f: &mut Frame, title: &str, message: &str) {
    let line_count = message.lines().count() as u16;
    let area = centered_rect(45, line_count + 4, f.area());
    f.render_widget(Clear, area);

    let border_color = if title.contains("Push") {
        colors::GREEN
    } else if title.contains("Pull") {
        colors::CYAN
    } else {
        colors::RED
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(colors::BG_SURFACE));

    let paragraph = Paragraph::new(message)
        .block(block)
        .style(Style::default().fg(colors::TEXT));
    f.render_widget(paragraph, area);
}

fn draw_help_popup(f: &mut Frame) {
    let area = centered_rect(55, 22, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::ACCENT))
        .title(Span::styled(
            " Keyboard Shortcuts ",
            Style::default()
                .fg(colors::ACCENT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(colors::BG_SURFACE));

    let shortcuts = [
        ("1/2/3", "Switch tabs"),
        ("↑↓ j/k", "Navigate"),
        ("Tab", "Cycle panes"),
        ("s / S", "Stage / Stage all"),
        ("u / U", "Unstage / Unstage all"),
        ("c", "Write commit msg"),
        ("Enter", "Commit / Checkout"),
        ("n", "New branch"),
        ("d", "Delete branch"),
        ("P", "Push to remote"),
        ("L", "Pull from remote"),
        ("F", "Fetch from remote"),
        ("PgUp/PgDn", "Scroll diff"),
        ("Mouse", "Click & scroll"),
        ("r", "Refresh"),
        ("q", "Quit"),
    ];

    let mut lines: Vec<Line> = Vec::new();
    for (key, desc) in &shortcuts {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:14}", key),
                Style::default().fg(colors::ACCENT),
            ),
            Span::styled(*desc, Style::default().fg(colors::TEXT_SECONDARY)),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "         Press ? or Esc to close",
        Style::default().fg(colors::TEXT_DIM),
    )));

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);
}
