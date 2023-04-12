use clap::Parser;
use colink_server::params::CoLinkServerParams;
use colink_server::server::init_and_run_server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // install global collector configured based on RUST_LOG env var.
    tracing_subscriber::fmt::init();
    let colink_server_params = CoLinkServerParams::parse();

    init_and_run_server(colink_server_params).await;

    Ok(())
}
