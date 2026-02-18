mod app;
mod codex_import;
mod models;
mod ui;

use color_eyre::Result;

use crate::app::{bootstrap_app, init_terminal, restore_terminal, run};

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut app = bootstrap_app()?;
    let terminal = init_terminal()?;
    let result = run(terminal, &mut app);
    restore_terminal()?;
    result
}
