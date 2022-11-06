use colink_server::server::init_and_run_server;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "CoLink", about = "CoLink")]
struct CommandLineArgs {
    /// Address of CoLink server
    // short and long flags (-d, --debug) will be deduced from the field's name
    #[structopt(short, long, env = "COLINK_SERVER_ADDRESS")]
    address: String,

    /// Port of CoLink server
    #[structopt(short, long, env = "COLINK_SERVER_PORT")]
    port: u16,

    /// AMQP URI of MQ
    #[structopt(long, env = "COLINK_SERVER_MQ_AMQP")]
    mq_amqp: String,

    /// Management API of MQ
    #[structopt(long, env = "COLINK_SERVER_MQ_API")]
    mq_api: String,

    /// Prefix of MQ
    #[structopt(long, env = "COLINK_SERVER_MQ_PREFIX", default_value = "colink")]
    mq_prefix: String,

    /// URI of CoLink server
    #[structopt(long, env = "COLINK_SERVER_CORE_URI")]
    core_uri: Option<String>,

    /// Path to server certificate.
    /// If not supplied, TLS and MTLS will not be enabled
    #[structopt(long, parse(from_os_str), env = "COLINK_SERVER_CERT")]
    cert: Option<PathBuf>,

    /// Path to private key.
    /// If not supplied, TLS and MTLS will not be enabled
    #[structopt(long, parse(from_os_str), env = "COLINK_SERVER_KEY")]
    key: Option<PathBuf>,

    /// Path to client CA certificate.
    /// If not supplied and both cert and key are supplied, then TLS is enabled.
    /// If all of cert, key, and ca are supplied, then MTLS is enabled.
    #[structopt(long, parse(from_os_str), env = "COLINK_SERVER_CA")]
    ca: Option<PathBuf>,

    /// Path to CA certificate you want to trust in inter-core communication.
    #[structopt(long, parse(from_os_str), env = "COLINK_SERVER_INTER_CORE_CA")]
    inter_core_ca: Option<PathBuf>,

    /// Path to client certificate you want to use in inter-core communication.
    #[structopt(long, parse(from_os_str), env = "COLINK_SERVER_INTER_CORE_CERT")]
    inter_core_cert: Option<PathBuf>,

    /// Path to private key you want to use in inter-core communication.
    #[structopt(long, parse(from_os_str), env = "COLINK_SERVER_INTER_CORE_KEY")]
    inter_core_key: Option<PathBuf>,

    /// Enforce the generation of the new jwt secret.
    #[structopt(long, env = "COLINK_SERVER_FORCE_GEN_JWT_SECRET")]
    force_gen_jwt_secret: bool,

    /// Enforce the generation of the core's private key.
    #[structopt(long, env = "COLINK_SERVER_FORCE_GEN_PRIV_KEY")]
    force_gen_priv_key: bool,

    /// Create reverse connections to other servers.
    #[structopt(long, env = "COLINK_SERVER_INTER_CORE_REVERSE_MODE")]
    inter_core_reverse_mode: bool,
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
    } = CommandLineArgs::from_args();

    init_and_run_server(
        address,
        port,
        mq_amqp,
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
