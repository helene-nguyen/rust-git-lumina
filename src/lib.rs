pub mod app;
pub mod git_ops;
pub mod ui;

use anyhow::Result;
use app::{App, FilePane, InputMode, Popup, Tab};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::time::Duration;

pub fn run() -> Result<()> {
    let repo = git_ops::open_repo(None)?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(repo)?;

    let result = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {}", e);
    }

    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    // Mirrors `app.mouse_capture` so we only toggle the terminal when it changes.
    let mut capture_on = app.mouse_capture;
    loop {
        if app.mouse_capture != capture_on {
            if app.mouse_capture {
                execute!(io::stdout(), EnableMouseCapture)?;
            } else {
                execute!(io::stdout(), DisableMouseCapture)?;
            }
            capture_on = app.mouse_capture;
        }

        terminal.draw(|f| ui::draw(f, app))?;

        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        app.running = false;
                    }

                    match app.input_mode {
                        InputMode::Editing => handle_editing_input(app, key.code),
                        InputMode::Normal => match &app.popup {
                            Popup::ConfirmDelete(_) | Popup::ConfirmPush | Popup::ConfirmPull => {
                                match key.code {
                                    KeyCode::Char('y') => app.confirm_popup(),
                                    KeyCode::Char('n') | KeyCode::Esc => app.cancel_popup(),
                                    _ => {}
                                }
                            }
                            Popup::Help => match key.code {
                                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
                                    app.popup = Popup::None
                                }
                                _ => {}
                            },
                            Popup::None => handle_normal_input(app, key.code),
                            _ => {}
                        },
                    }
                }

                Event::Mouse(mouse) => {
                    handle_mouse_event(app, mouse);
                }

                _ => {}
            }
        }

        app.maybe_auto_refresh();

        if !app.running {
            return Ok(());
        }
    }
}

fn handle_mouse_event(app: &mut App, mouse: MouseEvent) {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            app.handle_click(mouse.column, mouse.row);
        }

        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            let up = matches!(mouse.kind, MouseEventKind::ScrollUp);
            app.handle_scroll(mouse.column, mouse.row, up, app.diff_area);
        }

        _ => {}
    }
}

fn handle_normal_input(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('q') => app.running = false,

        KeyCode::Char('1') => app.tab = Tab::Graph,
        KeyCode::Char('2') => app.tab = Tab::Files,
        KeyCode::Char('3') => app.tab = Tab::Branches,
        KeyCode::Tab => app.next_tab(),
        KeyCode::BackTab => app.prev_tab(),

        KeyCode::Up | KeyCode::Char('k') => app.select_up(),
        KeyCode::Down | KeyCode::Char('j') => app.select_down(),

        KeyCode::PageUp => {
            if app.tab == Tab::Files {
                app.diff_scroll_up(10);
            }
        }
        KeyCode::PageDown => {
            if app.tab == Tab::Files {
                app.diff_scroll_down(10, 20);
            }
        }

        KeyCode::Char('r') => {
            let _ = app.refresh_all();
            app.status_msg = "Refreshed".to_string();
        }

        KeyCode::Char('a') => app.toggle_auto_refresh(),

        KeyCode::Char('m') => app.toggle_mouse_capture(),

        KeyCode::Char('?') => app.show_help(),

        KeyCode::Char('P') => app.start_push(),
        KeyCode::Char('L') => app.start_pull(),
        KeyCode::Char('F') => app.do_fetch(),

        _ => match app.tab {
            Tab::Files => handle_files_input(app, key),
            Tab::Branches => handle_branches_input(app, key),
            Tab::Graph => {}
        },
    }
}

fn handle_files_input(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Left | KeyCode::Right => app.toggle_file_pane(),

        KeyCode::Char('s') => app.stage_selected(),
        KeyCode::Char('u') => app.unstage_selected(),

        KeyCode::Char('S') => app.stage_all(),
        KeyCode::Char('U') => app.unstage_all(),

        KeyCode::Char('c') => app.start_commit_input(),

        KeyCode::Enter if app.file_pane == FilePane::CommitMsg => {
            app.do_commit();
        }

        _ => {}
    }
}

fn handle_branches_input(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Enter => app.checkout_selected_branch(),
        KeyCode::Char('n') => app.start_new_branch(),
        KeyCode::Char('d') => app.start_delete_branch(),
        _ => {}
    }
}

fn handle_editing_input(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc => {
            if app.popup != Popup::None {
                app.cancel_popup();
            } else {
                app.input_mode = InputMode::Normal;
            }
        }
        KeyCode::Enter => {
            if app.popup == Popup::NewBranch {
                app.confirm_new_branch();
            } else if app.file_pane == FilePane::CommitMsg {
                app.input_mode = InputMode::Normal;
                app.do_commit();
            }
        }
        KeyCode::Backspace => {
            if app.popup != Popup::None {
                app.input_buffer.pop();
            } else if app.file_pane == FilePane::CommitMsg {
                app.commit_message.pop();
            }
        }
        KeyCode::Char(c) => {
            if app.popup != Popup::None {
                app.input_buffer.push(c);
            } else if app.file_pane == FilePane::CommitMsg {
                app.commit_message.push(c);
            }
        }
        _ => {}
    }
}
