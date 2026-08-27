//! `mca2lod` executable entry point.

mod config;
mod pipeline;

use clap::Parser;
use config::CliConfig;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cfg = CliConfig::parse();

    // Initialize tracing subscriber for diagnostic filtering
    if cfg.verbose {
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
    }

    match pipeline::run_pipeline(cfg) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("mca2lod: fatal error: {:#}", err);
            ExitCode::FAILURE
        }
    }
}
