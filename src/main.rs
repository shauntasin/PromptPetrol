mod app;
mod codex_import;
mod models;
mod ui;

use std::path::PathBuf;
use std::time::Duration;
use std::{fs, io::Write};

use color_eyre::eyre::{Result, bail};

use crate::app::{DEFAULT_REFRESH_INTERVAL, bootstrap_app, init_terminal, restore_terminal, run};
use crate::models::provider_summaries;

struct CliArgs {
    data_file: Option<PathBuf>,
    config_file: Option<PathBuf>,
    refresh_interval: Duration,
    export_json: Option<PathBuf>,
    export_csv: Option<PathBuf>,
}

fn parse_cli_args() -> Result<CliArgs> {
    let mut args = std::env::args().skip(1);
    let mut data_file = None;
    let mut config_file = None;
    let mut refresh_interval = DEFAULT_REFRESH_INTERVAL;
    let mut export_json = None;
    let mut export_csv = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-file" => {
                let Some(value) = args.next() else {
                    bail!("missing value for --data-file");
                };
                data_file = Some(PathBuf::from(value));
            }
            "--config-file" => {
                let Some(value) = args.next() else {
                    bail!("missing value for --config-file");
                };
                config_file = Some(PathBuf::from(value));
            }
            "--refresh-interval-seconds" => {
                let Some(value) = args.next() else {
                    bail!("missing value for --refresh-interval-seconds");
                };
                let seconds = value
                    .parse::<u64>()
                    .map_err(|_| color_eyre::eyre::eyre!("invalid refresh interval: {value}"))?;
                if seconds == 0 {
                    bail!("--refresh-interval-seconds must be >= 1");
                }
                refresh_interval = Duration::from_secs(seconds);
            }
            "--export-json" => {
                let Some(value) = args.next() else {
                    bail!("missing value for --export-json");
                };
                export_json = Some(PathBuf::from(value));
            }
            "--export-csv" => {
                let Some(value) = args.next() else {
                    bail!("missing value for --export-csv");
                };
                export_csv = Some(PathBuf::from(value));
            }
            _ => {
                bail!("unknown argument: {arg}");
            }
        }
    }

    Ok(CliArgs {
        data_file,
        config_file,
        refresh_interval,
        export_json,
        export_csv,
    })
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let args = parse_cli_args()?;
    let mut app = bootstrap_app(args.data_file, args.config_file)?;
    if args.export_json.is_some() || args.export_csv.is_some() {
        export_provider_summaries(&app, args.export_json, args.export_csv)?;
        return Ok(());
    }
    let terminal = init_terminal()?;
    let result = run(terminal, &mut app, args.refresh_interval);
    restore_terminal()?;
    result
}

fn export_provider_summaries(
    app: &app::App,
    export_json: Option<PathBuf>,
    export_csv: Option<PathBuf>,
) -> Result<()> {
    let summaries = provider_summaries(&app.data);

    if let Some(path) = export_json {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let payload = serde_json::to_string_pretty(&summaries)?;
        fs::write(path, payload)?;
    }

    if let Some(path) = export_csv {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(path)?;
        writeln!(file, "provider,total_tokens,total_cost_usd")?;
        for summary in &summaries {
            writeln!(
                file,
                "{},{},{}",
                summary.provider, summary.total_tokens, summary.total_cost_usd
            )?;
        }
    }

    Ok(())
}
