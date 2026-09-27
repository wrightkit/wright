#[path = "../serve.rs"]
mod serve;

use clap::Parser;
use std::process::ExitCode;

#[derive(Debug, Parser)]
struct Cli {
    #[command(flatten)]
    serve: serve::ServeArgs,
}

fn main() -> ExitCode {
    serve::run(Cli::parse().serve)
}
