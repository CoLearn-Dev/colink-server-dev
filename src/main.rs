use colink_server::server::init_and_run_server;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "CoLink", about = "CoLink")]
struct CommandLineArgs {
    /// Address of CoLink server
    // short and long flags (-d, --debug) will be deduced from the field's name
    #[structopt(short, long)]
    address: String,

    /// Port of CoLink server
    #[structopt(short, long)]
    port: u16,

    /// AMQP URI of MQ
    #[structopt(long)]
    mq_amqp: String,

    /// Management API of MQ
    #[structopt(long)]
    mq_api: String,

    /// Prefix of MQ
    #[structopt(long, default_value = "colink")]
    mq_prefix: String,

    /// Path to server certificate.
    /// If not supplied, TLS and MTLS will not be enabled
    #[structopt(long, parse(from_os_str))]
    cert: Option<PathBuf>,

    /// Path to private key.
    /// If not supplied, TLS and MTLS will not be enabled
    #[structopt(long, parse(from_os_str))]
    key: Option<PathBuf>,

    /// Path to client CA certificate.
    /// If not supplied and both cert and key are supplied, then TLS is enabled.
    /// If all of cert, key, and ca are supplied, then MTLS is enabled.
    #[structopt(long, parse(from_os_str))]
    ca: Option<PathBuf>,

    /// Path to CA certificate you want to trust in inter-core communication.
    #[structopt(long, parse(from_os_str))]
    inter_core_ca: Option<PathBuf>,

    /// Path to client certificate you want to use in inter-core communication.
    #[structopt(long, parse(from_os_str))]
    inter_core_cert: Option<PathBuf>,

    /// Path to private key you want to use in inter-core communication.
    #[structopt(long, parse(from_os_str))]
    inter_core_key: Option<PathBuf>,

    /// Enforce the generation of the new initial state.
    #[structopt(long)]
    force_gen_init_state: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // install global collector configured based on RUST_LOG env var.
    tracing_subscriber::fmt::init();
    let CommandLineArgs {
        address,
        port,
        mq_amqp,
        mq_api,
        mq_prefix,
        cert,
        key,
        ca,
        inter_core_ca,
        inter_core_cert,
        inter_core_key,
        force_gen_init_state,
    } = CommandLineArgs::from_args();

    init_and_run_server(
        address,
        port,
        mq_amqp,
        mq_api,
        mq_prefix,
        cert,
        key,
        ca,
        inter_core_ca,
        inter_core_cert,
        inter_core_key,
        force_gen_init_state,
    )
    .await;

    Ok(())
}
