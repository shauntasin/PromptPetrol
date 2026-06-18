mod app;
mod claude_import;
mod codex_import;
mod models;
mod ui;

use std::path::PathBuf;
use std::time::Duration;

use color_eyre::eyre::{Result, bail, eyre};

use crate::app::{DEFAULT_REFRESH_INTERVAL, bootstrap_app, init_terminal, restore_terminal, run};

struct CliArgs {
    config_file: Option<PathBuf>,
    refresh_interval: Duration,
}

fn parse_cli_args() -> Result<CliArgs> {
    let mut args = std::env::args().skip(1);
    let mut config_file = None;
    let mut refresh_interval = DEFAULT_REFRESH_INTERVAL;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config-file" => {
                config_file = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| eyre!("missing value for --config-file"))?,
                ));
            }
            "--refresh-interval-seconds" => {
                let value = args
                    .next()
                    .ok_or_else(|| eyre!("missing value for --refresh-interval-seconds"))?;
                let seconds: f64 = value
                    .parse()
                    .map_err(|_| eyre!("invalid refresh interval: {value}"))?;
                if seconds <= 0.0 {
                    bail!("--refresh-interval-seconds must be > 0");
                }
                refresh_interval = Duration::from_secs_f64(seconds);
            }
            _ => bail!("unknown argument: {arg}"),
        }
    }

    Ok(CliArgs {
        config_file,
        refresh_interval,
    })
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = parse_cli_args()?;
    let mut app = bootstrap_app(args.config_file)?;
    let terminal = init_terminal()?;
    let result = run(terminal, &mut app, args.refresh_interval);
    restore_terminal()?;
    result
}
