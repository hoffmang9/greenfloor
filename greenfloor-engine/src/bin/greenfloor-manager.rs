use clap::Parser;
use greenfloor_engine::manager_cli::{run_manager_cli, ManagerCli};

#[tokio::main]
async fn main() {
    let cli = ManagerCli::parse();
    match run_manager_cli(cli).await {
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
