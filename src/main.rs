use clap::Parser;
use colink_server::server::init_and_run_server;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "CoLink", about = "CoLink", version)]
struct CommandLineArgs {
    /// Address of CoLink server
    // short and long flags (-d, --debug) will be deduced from the field's name
    #[arg(short, long, env = "COLINK_SERVER_ADDRESS")]
    address: String,

    /// Port of CoLink server
    #[arg(short, long, env = "COLINK_SERVER_PORT")]
    port: u16,

    /// URI of MQ (AMQP or Redis)
    #[arg(long, env = "COLINK_SERVER_MQ_URI")]
    mq_uri: String,

    /// Management API of MQ, only required by RabbitMQ
    #[arg(long, env = "COLINK_SERVER_MQ_API")]
    mq_api: Option<String>,

    /// Prefix of MQ, for RabbitMQ only
    #[arg(long, env = "COLINK_SERVER_MQ_PREFIX", default_value = "colink")]
    mq_prefix: String,

    /// URI of CoLink server
    #[arg(long, env = "COLINK_SERVER_CORE_URI")]
    core_uri: Option<String>,

    /// Path to server certificate.
    /// If not supplied, TLS and MTLS will not be enabled
    #[arg(long, env = "COLINK_SERVER_CERT")]
    cert: Option<PathBuf>,

    /// Path to private key.
    /// If not supplied, TLS and MTLS will not be enabled
    #[arg(long, env = "COLINK_SERVER_KEY")]
    key: Option<PathBuf>,

    /// Path to client CA certificate.
    /// If not supplied and both cert and key are supplied, then TLS is enabled.
    /// If all of cert, key, and ca are supplied, then MTLS is enabled.
    #[arg(long, env = "COLINK_SERVER_CA")]
    ca: Option<PathBuf>,

    /// Path to CA certificate you want to trust in inter-core communication.
    #[arg(long, env = "COLINK_SERVER_INTER_CORE_CA")]
    inter_core_ca: Option<PathBuf>,

    /// Path to client certificate you want to use in inter-core communication.
    #[arg(long, env = "COLINK_SERVER_INTER_CORE_CERT")]
    inter_core_cert: Option<PathBuf>,

    /// Path to private key you want to use in inter-core communication.
    #[arg(long, env = "COLINK_SERVER_INTER_CORE_KEY")]
    inter_core_key: Option<PathBuf>,

    /// Enforce the generation of the new jwt secret.
    #[arg(long, env = "COLINK_SERVER_FORCE_GEN_JWT_SECRET")]
    force_gen_jwt_secret: bool,

    /// Enforce the generation of the core's private key.
    #[arg(long, env = "COLINK_SERVER_FORCE_GEN_PRIV_KEY")]
    force_gen_priv_key: bool,

    /// Create reverse connections to other servers.
    #[arg(long, env = "COLINK_SERVER_INTER_CORE_REVERSE_MODE")]
    inter_core_reverse_mode: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // install global collector configured based on RUST_LOG env var.
    tracing_subscriber::fmt::init();
    let CommandLineArgs {
        address,
        port,
        mq_uri,
        mq_api,
        mq_prefix,
        core_uri,
        cert,
        key,
        ca,
        inter_core_ca,
        inter_core_cert,
        inter_core_key,
        force_gen_jwt_secret,
        force_gen_priv_key,
        inter_core_reverse_mode,
    } = CommandLineArgs::parse();

    init_and_run_server(
        address,
        port,
        mq_uri,
        mq_api,
        mq_prefix,
        core_uri,
        cert,
        key,
        ca,
        inter_core_ca,
        inter_core_cert,
        inter_core_key,
        force_gen_jwt_secret,
        force_gen_priv_key,
        inter_core_reverse_mode,
    )
    .await;

    Ok(())
}
