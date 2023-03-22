use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Clone, Parser)]
#[command(name = "CoLink", about = "CoLink", version)]
pub struct CoLinkServerParams {
    /// Address of CoLink server
    // short and long flags (-d, --debug) will be deduced from the field's name
    #[arg(
        short,
        long,
        env = "COLINK_SERVER_ADDRESS",
        default_value = "127.0.0.1"
    )]
    pub address: String,

    /// Port of CoLink server
    #[arg(short, long, env = "COLINK_SERVER_PORT", default_value = "2021")]
    pub port: u16,

    /// URI of MQ (AMQP or Redis)
    #[arg(long, env = "COLINK_SERVER_MQ_URI")]
    pub mq_uri: Option<String>,

    /// Management API of MQ, only required by RabbitMQ
    #[arg(long, env = "COLINK_SERVER_MQ_API")]
    pub mq_api: Option<String>,

    /// Prefix of MQ
    #[arg(long, env = "COLINK_SERVER_MQ_PREFIX", default_value = "colink")]
    pub mq_prefix: String,

    /// URI of CoLink server
    #[arg(
        long,
        env = "COLINK_SERVER_CORE_URI",
        default_value = "http://127.0.0.1:2021"
    )]
    pub core_uri: Option<String>,

    /// Path to server certificate.
    /// If not supplied, TLS and MTLS will not be enabled
    #[arg(long, env = "COLINK_SERVER_CERT")]
    pub cert: Option<PathBuf>,

    /// Path to private key.
    /// If not supplied, TLS and MTLS will not be enabled
    #[arg(long, env = "COLINK_SERVER_KEY")]
    pub key: Option<PathBuf>,

    /// Path to client CA certificate.
    /// If not supplied and both cert and key are supplied, then TLS is enabled.
    /// If all of cert, key, and ca are supplied, then MTLS is enabled.
    #[arg(long, env = "COLINK_SERVER_CA")]
    pub ca: Option<PathBuf>,

    /// Path to CA certificate you want to trust in inter-core communication.
    #[arg(long, env = "COLINK_SERVER_INTER_CORE_CA")]
    pub inter_core_ca: Option<PathBuf>,

    /// Path to client certificate you want to use in inter-core communication.
    #[arg(long, env = "COLINK_SERVER_INTER_CORE_CERT")]
    pub inter_core_cert: Option<PathBuf>,

    /// Path to private key you want to use in inter-core communication.
    #[arg(long, env = "COLINK_SERVER_INTER_CORE_KEY")]
    pub inter_core_key: Option<PathBuf>,

    /// Enforce the generation of the new jwt secret.
    #[arg(long, env = "COLINK_SERVER_FORCE_GEN_JWT_SECRET")]
    pub force_gen_jwt_secret: bool,

    /// Enforce the generation of the core's private key.
    #[arg(long, env = "COLINK_SERVER_FORCE_GEN_PRIV_KEY")]
    pub force_gen_priv_key: bool,

    /// Create reverse connections to other servers.
    #[arg(long, env = "COLINK_SERVER_INTER_CORE_REVERSE_MODE")]
    pub inter_core_reverse_mode: bool,

    /// Set protocol inventory for POM.
    #[arg(
        long,
        env = "COLINK_POM_PROTOCOL_INVENTORY",
        default_value = "https://raw.githubusercontent.com/CoLearn-Dev/colink-protocol-inventory/main/protocols"
    )]
    pub pom_protocol_inventory: String,

    /// Allow POM to launch protocol from any source.
    #[arg(long, env = "COLINK_POM_DEV_MODE")]
    pub pom_dev_mode: bool,
}
