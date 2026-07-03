mod app;
mod config;
mod db;
mod models;
mod queries;
mod ui;

use std::{io, time::Duration};

use anyhow::Result;
use app::App;
use config::Config;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    let pool = db::connect(&config.database_url).await?;
    let mut app = App::new(
        config.cli.tenant,
        config.cli.limit,
        config.cli.refresh_seconds,
    );
    app.reload(&pool).await;

    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal, &pool, &mut app).await;
    restore_terminal(&mut terminal)?;
    result
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    pool: &sqlx::PgPool,
    app: &mut App,
) -> Result<()> {
    while !app.should_quit {
        if app.needs_refresh() {
            app.reload(pool).await;
        }

        terminal.draw(|frame| ui::render(frame, app))?;

        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                let reload = app.handle_key(key);
                if reload {
                    app.reload(pool).await;
                }
            }
        }
    }

    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
