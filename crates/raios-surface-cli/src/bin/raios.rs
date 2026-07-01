use std::io;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use raios_surface_tui::app::App;
use raios_surface_tui::ui;
use raios_surface_cli::cli::{self, Cli};

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.refactor {
        cli::run_refactor_flag(cli.json);
        return Ok(());
    }

    let remote = cli.remote.clone();

    if cli.command.is_some() {
        cli::run(cli);
        return Ok(());
    }

    run_tui(remote)
}

fn run_tui(remote: Option<String>) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, remote);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, remote: Option<String>) -> Result<()> {
    let mut app = match remote {
        Some(host) => App::new_remote(host),
        None => App::new(),
    };

    loop {
        app.tick();

        terminal.draw(|frame| {
            app.width = frame.area().width;
            app.height = frame.area().height;
            ui::render(frame, &app);
        })?;

        if event::poll(Duration::from_millis(40))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => app.handle_key(key)?,
                Event::Resize(w, h) => {
                    app.width = w;
                    app.height = h;
                }
                _ => {}
            }
        }

        while let Ok(msg) = app.rx.try_recv() {
            app.handle_bg_msg(msg);
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
