use crossterm::{terminal, execute};
use ratatui::{Terminal, backend::CrosstermBackend, widgets::{Block, Borders}};
use std::io::stdout;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    terminal::enable_raw_mode()?;
    execute!(stdout(), terminal::EnterAlternateScreen)?;
    let mut term = Terminal::new(CrosstermBackend::new(stdout()))?;

    term.draw(|f| {
        f.render_widget(Block::default().title("Trail — env check OK").borders(Borders::ALL), f.area());
    })?;

    std::thread::sleep(std::time::Duration::from_secs(2));

    execute!(stdout(), terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    Ok(())
}

