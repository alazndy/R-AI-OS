mod app;
mod cli;
mod discovery;
mod filebrowser;
mod sync;
mod ui;

use std::io;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::app::App;
use crate::cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(command) = cli.command {
        cli::run(command);
        return Ok(());
    }

    run_tui()
}

fn run_tui() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let mut app = App::new();

    loop {
        app.tick();

        terminal.draw(|frame| {
            app.width = frame.area().width;
            app.height = frame.area().height;
            ui::render(frame, &app);
        })?;

        if event::poll(Duration::from_millis(40))? {
            match event::read()? {
                Event::Key(key) => app.handle_key(key)?,
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
