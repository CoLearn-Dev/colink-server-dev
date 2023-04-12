use crate::colink_proto::co_link_server::{CoLink, CoLinkServer};
use crate::colink_proto::*;
use crate::mq::{common::MQ, rabbitmq::RabbitMQ, redis::RedisStream};
use crate::params::CoLinkServerParams;
use crate::service::auth::{gen_jwt_secret, print_host_token, CheckAuthInterceptor};
use crate::storage::basic::BasicStorage;
use crate::subscription::{common::StorageWithSubscription, mq::StorageWithMQSubscription};
use crate::utils::{start_redis_server, RedisServer};
use secp256k1::Secp256k1;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tokio::sync::{Mutex, RwLock};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{
    transport::{Certificate, Identity, Server, ServerTlsConfig},
    Request, Response, Status,
};
use tracing::error;

#[allow(clippy::type_complexity)]
pub struct MyService {
    pub storage: Box<dyn StorageWithSubscription>,
    pub jwt_secret: [u8; 32],
    pub mq: Arc<dyn MQ>,
    pub imported_users: RwLock<HashSet<String>>,
    // We use this mutex to avoid the TOCTOU race condition in task storage.
    pub task_storage_mutex: Mutex<i32>,
    pub public_key: secp256k1::PublicKey,
    pub secret_key: secp256k1::SecretKey,
    pub inter_core_ca_certificate: Option<Certificate>,
    pub inter_core_identity: Option<Identity>,
    pub params: CoLinkServerParams,
    pub inter_core_reverse_senders: Mutex<HashMap<(String, String), Sender<Result<Task, Status>>>>,
    pub inter_core_reverse_handlers: Mutex<
        HashMap<
            (String, String),
            tokio::task::JoinHandle<Result<(), Box<dyn std::error::Error + Send + Sync>>>,
        >,
    >,
}

pub struct GrpcService {
    pub service: Arc<MyService>,
}

#[tonic::async_trait]
impl CoLink for GrpcService {
    async fn generate_token(
        &self,
        request: Request<GenerateTokenRequest>,
    ) -> Result<Response<Jwt>, Status> {
        self.service._generate_token(request).await
    }

    async fn import_user(&self, request: Request<UserConsent>) -> Result<Response<Jwt>, Status> {
        self.service
            ._import_user(request, self.service.clone())
            .await
    }

    async fn create_entry(
        &self,
        request: Request<StorageEntry>,
    ) -> Result<Response<StorageEntry>, Status> {
        self.service._create_entry(request).await
    }

    async fn read_entries(
        &self,
        request: Request<StorageEntries>,
    ) -> Result<Response<StorageEntries>, Status> {
        self.service._read_entries(request).await
    }

    async fn update_entry(
        &self,
        request: Request<StorageEntry>,
    ) -> Result<Response<StorageEntry>, Status> {
        self.service._update_entry(request).await
    }

    async fn delete_entry(
        &self,
        request: Request<StorageEntry>,
    ) -> Result<Response<StorageEntry>, Status> {
        self.service._delete_entry(request).await
    }

    async fn read_keys(
        &self,
        request: Request<ReadKeysRequest>,
    ) -> Result<Response<StorageEntries>, Status> {
        self.service._read_keys(request).await
    }

    async fn create_task(&self, request: Request<Task>) -> Result<Response<Task>, Status> {
        self.service
            ._create_task(request, self.service.clone())
            .await
    }

    async fn confirm_task(
        &self,
        request: Request<ConfirmTaskRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.service._confirm_task(request).await
    }

    async fn finish_task(&self, request: Request<Task>) -> Result<Response<Empty>, Status> {
        self.service._finish_task(request).await
    }

    async fn request_info(
        &self,
        request: Request<Empty>,
    ) -> Result<Response<RequestInfoResponse>, Status> {
        self.service._request_info(request).await
    }

    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<MqQueueName>, Status> {
        self.service._subscribe(request).await
    }

    async fn unsubscribe(&self, request: Request<MqQueueName>) -> Result<Response<Empty>, Status> {
        self.service._unsubscribe(request).await
    }

    async fn inter_core_sync_task(
        &self,
        request: Request<Task>,
    ) -> Result<Response<Empty>, Status> {
        self.service._inter_core_sync_task(request).await
    }

    type InterCoreSyncTaskWithReverseConnectionStream = ReceiverStream<Result<Task, Status>>;
    async fn inter_core_sync_task_with_reverse_connection(
        &self,
        request: Request<Task>,
    ) -> Result<Response<Self::InterCoreSyncTaskWithReverseConnectionStream>, Status> {
        self.service
            ._inter_core_sync_task_with_reverse_connection(request)
            .await
    }

    async fn start_protocol_operator(
        &self,
        request: Request<StartProtocolOperatorRequest>,
    ) -> Result<Response<ProtocolOperatorInstanceId>, Status> {
        self.service._start_protocol_operator(request).await
    }

    async fn stop_protocol_operator(
        &self,
        request: Request<ProtocolOperatorInstanceId>,
    ) -> Result<Response<Empty>, Status> {
        self.service._stop_protocol_operator(request).await
    }
}

pub async fn init_and_run_server(mut params: CoLinkServerParams) {
    let _redis_server = if params.mq_uri.is_none() {
        let (redis_server, uri) = match start_redis_server().await {
            Ok(res) => res,
            Err(e) => {
                error!("{}", e);
                std::process::exit(1);
            }
        };
        params.mq_uri = Some(uri);
        redis_server
    } else {
        RedisServer { process: None }
    };
    match run_server(params).await {
        Ok(_) => {}
        Err(e) => {
            error!("{}", e);
            std::process::exit(1);
        }
    }
}

async fn run_server(params: CoLinkServerParams) -> Result<(), Box<dyn std::error::Error>> {
    let socket_address = format!("{}:{}", params.address, params.port)
        .parse()
        .unwrap();
    std::fs::create_dir_all("init_state")?;
    if params.force_gen_jwt_secret || std::fs::metadata("init_state/jwt_secret.txt").is_err() {
        let jwt_secret = gen_jwt_secret();
        let mut file = std::fs::File::create("init_state/jwt_secret.txt")?;
        file.write_all(hex::encode(jwt_secret).as_bytes())?;
    }
    if params.force_gen_priv_key || std::fs::metadata("init_state/priv_key.txt").is_err() {
        let secp = Secp256k1::new();
        let (core_secret_key, _core_public_key) =
            secp.generate_keypair(&mut secp256k1::rand::thread_rng());
        let mut file = std::fs::File::create("init_state/priv_key.txt")?;
        file.write_all(hex::encode(core_secret_key.secret_bytes()).as_bytes())?;
    }
    let mut file = std::fs::File::open("init_state/jwt_secret.txt")?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    let jwt_secret = <[u8; 32]>::try_from(hex::decode(&buffer)?).unwrap();
    file = std::fs::File::open("init_state/priv_key.txt")?;
    buffer.clear();
    file.read_to_end(&mut buffer)?;
    let core_secret_key = secp256k1::SecretKey::from_slice(&hex::decode(&buffer)?)?;
    let core_public_key =
        secp256k1::PublicKey::from_secret_key(&Secp256k1::new(), &core_secret_key);
    let host_id = hex::encode(core_public_key.serialize());
    tokio::spawn(print_host_token(jwt_secret, host_id.clone()));
    let uri_parsed = url::Url::parse(params.mq_uri.as_ref().unwrap())?;
    let mq: Arc<dyn MQ> = if uri_parsed.scheme().starts_with("redis") {
        Arc::new(RedisStream::new(
            params.mq_uri.as_ref().unwrap(),
            &params.mq_prefix,
        ))
    } else {
        if params.mq_api.is_none() {
            Err("--mq-api <MQ_API> must be provided.")?;
        }
        Arc::new(RabbitMQ::new(
            params.mq_uri.as_ref().unwrap(),
            params.mq_api.as_ref().unwrap(),
            &params.mq_prefix,
        ))
    };
    let mut service = MyService {
        storage: Box::new(StorageWithMQSubscription::new(
            Box::<BasicStorage>::default(),
            mq.clone(),
        )),
        jwt_secret,
        mq: mq.clone(),
        imported_users: RwLock::new(HashSet::new()),
        task_storage_mutex: Mutex::new(0),
        secret_key: core_secret_key,
        public_key: core_public_key,
        inter_core_ca_certificate: None,
        inter_core_identity: None,
        params: params.clone(),
        inter_core_reverse_senders: Mutex::new(HashMap::new()),
        inter_core_reverse_handlers: Mutex::new(HashMap::new()),
    };
    if let Some(inter_core_ca) = params.inter_core_ca {
        service = service.ca_certificate(&inter_core_ca.as_path().display().to_string());
    }
    if let (Some(inter_core_cert), Some(inter_core_key)) =
        (params.inter_core_cert, params.inter_core_key)
    {
        service = service.identity(
            &inter_core_cert.as_path().display().to_string(),
            &inter_core_key.as_path().display().to_string(),
        );
    }
    mq.delete_all_accounts().await?;
    let grpc_service = GrpcService {
        service: Arc::new(service),
    };
    let check_auth_interceptor = CheckAuthInterceptor { jwt_secret };
    let grpc_service = CoLinkServer::with_interceptor(grpc_service, check_auth_interceptor);
    let grpc_service = tonic_web::config().enable(grpc_service);

    if params.cert.is_none() || params.key.is_none() {
        /* No TLS */
        Server::builder()
            .layer(tower_http::cors::CorsLayer::permissive())
            .accept_http1(true)
            .add_service(grpc_service)
            .serve(socket_address)
            .await?;
    } else {
        // reading cert and key of server from disk
        let cert = tokio::fs::read(params.cert.unwrap()).await?;
        let key = tokio::fs::read(params.key.unwrap()).await?;
        // creating identity from cert and key
        let server_identity = tonic::transport::Identity::from_pem(cert, key);
        let tls = if let Some(ca) = params.ca {
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
            .layer(tower_http::cors::CorsLayer::permissive())
            .accept_http1(true)
            .tls_config(tls)?
            .add_service(grpc_service)
            .serve(socket_address)
            .await?;
    }
    Ok(())
}
