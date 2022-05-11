use crate::colink_proto::co_link_server::{CoLink, CoLinkServer};
use crate::colink_proto::*;
use crate::mq::{common::MQ, rabbitmq::RabbitMQ};
use crate::service::auth::{gen_jwt, print_admin_token, CheckAuthInterceptor};
use crate::storage::basic::BasicStorage;
use crate::subscription::{common::StorageWithSubscription, mq::StorageWithMQSubscription};
use secp256k1::Secp256k1;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::sync::Mutex;
use tonic::{
    transport::{Certificate, Identity, Server, ServerTlsConfig},
    Request, Response, Status,
};
use tracing::error;

pub struct MyService {
    pub storage: Box<dyn StorageWithSubscription>,
    pub jwt_secret: [u8; 32],
    pub mq: Box<dyn MQ>,
    // We use this mutex to avoid the TOCTOU race condition in task storage.
    pub task_storage_mutex: Mutex<i32>,
    pub public_key: secp256k1::PublicKey,
    pub secret_key: secp256k1::SecretKey,
    pub inter_core_ca_certificate: Option<Certificate>,
    pub inter_core_identity: Option<Identity>,
}

#[tonic::async_trait]
impl CoLink for MyService {
    async fn refresh_token(
        &self,
        request: Request<RefreshTokenRequest>,
    ) -> Result<Response<Jwt>, Status> {
        self._refresh_token(request).await
    }

    async fn import_user(&self, request: Request<UserConsent>) -> Result<Response<Jwt>, Status> {
        self._import_user(request).await
    }

    async fn create_entry(
        &self,
        request: Request<StorageEntry>,
    ) -> Result<Response<StorageEntry>, Status> {
        self._create_entry(request).await
    }

    async fn read_entries(
        &self,
        request: Request<StorageEntries>,
    ) -> Result<Response<StorageEntries>, Status> {
        self._read_entries(request).await
    }

    async fn update_entry(
        &self,
        request: Request<StorageEntry>,
    ) -> Result<Response<StorageEntry>, Status> {
        self._update_entry(request).await
    }

    async fn delete_entry(
        &self,
        request: Request<StorageEntry>,
    ) -> Result<Response<StorageEntry>, Status> {
        self._delete_entry(request).await
    }

    async fn read_keys(
        &self,
        request: Request<ReadKeysRequest>,
    ) -> Result<Response<StorageEntries>, Status> {
        self._read_keys(request).await
    }

    async fn create_task(&self, request: Request<Task>) -> Result<Response<Task>, Status> {
        self._create_task(request).await
    }

    async fn confirm_task(
        &self,
        request: Request<ConfirmTaskRequest>,
    ) -> Result<Response<Empty>, Status> {
        self._confirm_task(request).await
    }

    async fn finish_task(&self, request: Request<Task>) -> Result<Response<Empty>, Status> {
        self._finish_task(request).await
    }

    async fn request_core_info(
        &self,
        request: Request<Empty>,
    ) -> Result<Response<CoreInfo>, Status> {
        self._request_core_info(request).await
    }

    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<MqQueueName>, Status> {
        self._subscribe(request).await
    }

    async fn unsubscribe(&self, request: Request<MqQueueName>) -> Result<Response<Empty>, Status> {
        self._unsubscribe(request).await
    }

    async fn inter_core_sync_task(
        &self,
        request: Request<Task>,
    ) -> Result<Response<Empty>, Status> {
        self._inter_core_sync_task(request).await
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn init_and_run_server(
    address: String,
    port: u16,
    mq_amqp: String,
    mq_api: String,
    mq_prefix: String,
    cert: Option<PathBuf>,
    key: Option<PathBuf>,
    ca: Option<PathBuf>,
    inter_core_ca: Option<PathBuf>,
    inter_core_cert: Option<PathBuf>,
    inter_core_key: Option<PathBuf>,
) {
    let socket_address = format!("{}:{}", address, port).parse().unwrap();
    match run_server(
        socket_address,
        mq_amqp,
        mq_api,
        mq_prefix,
        cert,
        key,
        ca,
        inter_core_ca,
        inter_core_cert,
        inter_core_key,
    )
    .await
    {
        Ok(_) => {}
        Err(e) => {
            error!("{}", e);
            std::process::exit(1);
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_server(
    socket_address: SocketAddr,
    mq_amqp: String,
    mq_api: String,
    mq_prefix: String,
    cert: Option<PathBuf>,
    key: Option<PathBuf>,
    ca: Option<PathBuf>,
    inter_core_ca: Option<PathBuf>,
    inter_core_cert: Option<PathBuf>,
    inter_core_key: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let jwt_secret = gen_jwt();
    tokio::spawn(print_admin_token(jwt_secret));
    let secp = Secp256k1::new();
    let (core_secret_key, core_public_key) =
        secp.generate_keypair(&mut secp256k1::rand::thread_rng());
    let mut service = MyService {
        storage: Box::new(StorageWithMQSubscription::new(
            Box::new(BasicStorage::new()),
            Box::new(RabbitMQ::new(&mq_amqp, &mq_api, &mq_prefix)),
        )),
        jwt_secret,
        mq: Box::new(RabbitMQ::new(&mq_amqp, &mq_api, &mq_prefix)),
        task_storage_mutex: Mutex::new(0),
        secret_key: core_secret_key,
        public_key: core_public_key,
        inter_core_ca_certificate: None,
        inter_core_identity: None,
    };
    if let Some(inter_core_ca) = inter_core_ca {
        service = service.ca_certificate(&inter_core_ca.as_path().display().to_string());
    }
    if let (Some(inter_core_cert), Some(inter_core_key)) = (inter_core_cert, inter_core_key) {
        service = service.identity(
            &inter_core_cert.as_path().display().to_string(),
            &inter_core_key.as_path().display().to_string(),
        );
    }
    service.mq.delete_all_accounts().await?;
    let check_auth_interceptor = CheckAuthInterceptor { jwt_secret };
    let service = CoLinkServer::with_interceptor(service, check_auth_interceptor);

    if cert.is_none() || key.is_none() {
        /* No TLS */
        Server::builder()
            .add_service(service)
            .serve(socket_address)
            .await?;
    } else {
        // reading cert and key of server from disk
        let cert = tokio::fs::read(cert.unwrap()).await?;
        let key = tokio::fs::read(key.unwrap()).await?;
        // creating identity from cert and key
        let server_identity = tonic::transport::Identity::from_pem(cert, key);
        let tls = if let Some(ca) = ca {
            /* MTLS */
            let client_ca_cert = tokio::fs::read(ca).await?;
            let client_ca_cert = tonic::transport::Certificate::from_pem(client_ca_cert);

            ServerTlsConfig::new()
                .identity(server_identity)
                .client_ca_root(client_ca_cert)
        } else {
            /* TLS */
            ServerTlsConfig::new().identity(server_identity)
        };

        Server::builder()
            .tls_config(tls)?
            .add_service(service)
            .serve(socket_address)
            .await?;
    }
    Ok(())
}
