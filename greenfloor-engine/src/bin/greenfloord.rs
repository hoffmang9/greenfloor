#![recursion_limit = "2048"]

use clap::Parser;
use greenfloor_engine::{run_daemon_command, DaemonCliArgs};

#[derive(Debug, Parser)]
#[command(name = "greenfloord", about = "GreenFloor native daemon")]
struct GreenfloordCli {
    #[command(flatten)]
    args: DaemonCliArgs,
}

#[tokio::main]
async fn main() {
    let cli = GreenfloordCli::parse();
    match run_daemon_command(cli.args).await {
        Ok(code) => {
            if code != 0 {
                std::process::exit(code);
            }
        }
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
    }
}
