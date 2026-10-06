#[path = "../mcp.rs"]
mod mcp;
#[path = "../serve.rs"]
mod serve;
#[path = "../tooldefs.rs"]
mod tooldefs;

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
