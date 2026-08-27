mod app;
mod claude_import;
mod codex_import;
mod models;
mod ui;

use std::path::PathBuf;
use std::time::Duration;

use color_eyre::eyre::{Result, bail, eyre};

use crate::app::{DEFAULT_REFRESH_INTERVAL, bootstrap_app, init_terminal, restore_terminal, run};

const HELP: &str = "PromptPetrol - monitor AI subscription usage in your terminal

Usage: promptpetrol [OPTIONS]

Options:
  --config-file <PATH>               Use a specific configuration file
  --refresh-interval-seconds <SECS>  Refresh data at this interval [default: 10]
  -h, --help                         Print help
  -V, --version                      Print version";

struct CliArgs {
    config_file: Option<PathBuf>,
    refresh_interval: Duration,
}

enum CliAction {
    Run(CliArgs),
    Help,
    Version,
}

fn parse_cli_args_from(args: impl IntoIterator<Item = String>) -> Result<CliAction> {
    let mut args = args.into_iter();
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
                    bail!("--refresh-interval-seconds must be finite and > 0");
                }
                refresh_interval = Duration::try_from_secs_f64(seconds)
                    .map_err(|_| eyre!("--refresh-interval-seconds must be finite and > 0"))?;
            }
            "-h" | "--help" => return Ok(CliAction::Help),
            "-V" | "--version" => return Ok(CliAction::Version),
            _ => bail!("unknown argument: {arg}"),
        }
    }

    Ok(CliAction::Run(CliArgs {
        config_file,
        refresh_interval,
    }))
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = match parse_cli_args_from(std::env::args().skip(1))? {
        CliAction::Run(args) => args,
        CliAction::Help => {
            println!("{HELP}");
            return Ok(());
        }
        CliAction::Version => {
            println!("PromptPetrol {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
    };
    let mut app = bootstrap_app(args.config_file)?;
    let terminal = init_terminal()?;
    let result = run(terminal, &mut app, args.refresh_interval);
    let restore_result = restore_terminal();
    result.and(restore_result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_custom_config_and_fractional_refresh_interval() {
        let action = parse_cli_args_from([
            "--config-file".into(),
            "/tmp/promptpetrol.json".into(),
            "--refresh-interval-seconds".into(),
            "0.5".into(),
        ])
        .expect("valid arguments");
        let CliAction::Run(args) = action else {
            panic!("expected run action");
        };

        assert_eq!(
            args.config_file,
            Some(PathBuf::from("/tmp/promptpetrol.json"))
        );
        assert_eq!(args.refresh_interval, Duration::from_millis(500));
    }

    #[test]
    fn rejects_non_finite_refresh_intervals_without_panicking() {
        for value in ["NaN", "inf", "-inf", "0", "-1"] {
            let result = parse_cli_args_from(["--refresh-interval-seconds".into(), value.into()]);
            assert!(result.is_err(), "accepted {value}");
        }
    }

    #[test]
    fn recognizes_help_and_version() {
        assert!(matches!(
            parse_cli_args_from(["--help".into()]),
            Ok(CliAction::Help)
        ));
        assert!(matches!(
            parse_cli_args_from(["--version".into()]),
            Ok(CliAction::Version)
        ));
    }
}
