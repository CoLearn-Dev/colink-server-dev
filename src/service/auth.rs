use super::user_init::user_init;
use crate::{colink_proto::*, server::MyService};
use chrono::TimeZone;
use jsonwebtoken::{DecodingKey, Validation};
use prost::Message;
use rand::RngCore;
use secp256k1::PublicKey;
use serde::{Deserialize, Serialize};
use std::{fmt::Debug, sync::Arc};
use tonic::{metadata::MetadataValue, service::Interceptor, Request, Response, Status};
use tracing::{debug, error};

#[derive(Clone)]
pub struct CheckAuthInterceptor {
    pub jwt_secret: [u8; 32],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthContent {
    privilege: String,
    user_id: String,
    exp: i64,
}

impl crate::server::MyService {
    pub async fn _generate_token(
        &self,
        request: Request<GenerateTokenRequest>,
    ) -> Result<Response<Jwt>, Status> {
        debug!("Got a request: {:?}", request);
        let user_id = match self.check_privilege_in(request.metadata(), &["user"]).await {
            Ok(_) => Self::get_key_from_metadata(request.metadata(), "user_id"),
            Err(e) => match &request.get_ref().user_consent {
                Some(user_consent) => {
                    let user_public_key =
                        self.check_user_consent(user_consent, &self.public_key.serialize())?;
                    let user_public_key = PublicKey::from_slice(&user_public_key).unwrap();
                    let user_id = hex::encode(&user_public_key.serialize());
                    if self.imported_users.read().await.contains(&user_id) {
                        let old_user_consent = self
                            ._internal_storage_read(&user_id, "user_consent")
                            .await?;
                        let old_user_consent: UserConsent =
                            Message::decode(&*old_user_consent).unwrap();
                        if user_consent.expiration_timestamp > old_user_consent.expiration_timestamp
                        {
                            let mut user_consent_bytes: Vec<u8> = vec![];
                            user_consent.encode(&mut user_consent_bytes).unwrap();
                            self._internal_storage_update(
                                &user_id,
                                "user_consent",
                                &user_consent_bytes,
                            )
                            .await?;
                        }
                        user_id
                    } else {
                        return Err(Status::permission_denied(format!(
                            "User {} in the consent does not exist.",
                            user_id
                        )));
                    }
                }
                None => {
                    return Err(e);
                }
            },
        };

        let body: GenerateTokenRequest = request.into_inner();
        if !["user", "guest"].contains(&body.privilege.as_str()) {
            return Err(Status::permission_denied(format!(
                "generating token with {} privilege is not allowed.",
                body.privilege
            )));
        }
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &AuthContent {
                privilege: body.privilege,
                user_id,
                exp: body.expiration_time,
            },
            &jsonwebtoken::EncodingKey::from_secret(&self.jwt_secret),
        )
        .unwrap();
        let reply = Jwt { jwt: token };
        Ok(Response::new(reply))
    }

    pub async fn _import_user(
        &self,
        request: Request<UserConsent>,
        service: Arc<MyService>,
    ) -> Result<Response<Jwt>, Status> {
        self.check_privilege_in(request.metadata(), &["host"])
            .await?;
        let body: UserConsent = request.into_inner();
        let user_consent_to_be_stored: UserConsent = body.clone();
        let user_consent_to_be_checked: UserConsent = body.clone();
        let signature_timestamp: i64 = body.signature_timestamp;
        let expiration_timestamp: i64 = body.expiration_timestamp;
        if chrono::Utc
            .timestamp(signature_timestamp, 0)
            .signed_duration_since(chrono::Utc::now())
            .num_seconds()
            .abs()
            > 10 * 60
        {
            return Err(Status::unauthenticated(
                "the timestamp is more than 10 minutes before the current time",
            ));
        }
        let user_public_key =
            self.check_user_consent(&user_consent_to_be_checked, &self.public_key.serialize())?;
        let user_public_key: PublicKey = match PublicKey::from_slice(&user_public_key) {
            Ok(pk) => pk,
            Err(e) => {
                return Err(Status::invalid_argument(format!(
                    "The public key could not be decoded in compressed serialized format: {:?}",
                    e
                )))
            }
        };
        let user_id = hex::encode(&user_public_key.serialize());
        let mut user_consent_bytes: Vec<u8> = vec![];
        user_consent_to_be_stored
            .encode(&mut user_consent_bytes)
            .unwrap();
        self._internal_storage_update(&user_id, "user_consent", &user_consent_bytes)
            .await?;

        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &AuthContent {
                privilege: "user".to_string(),
                user_id: user_id.clone(),
                exp: expiration_timestamp,
            },
            &jsonwebtoken::EncodingKey::from_secret(&self.jwt_secret),
        )
        .unwrap();
        self._host_storage_update(&format!("users:{}:user_jwt", user_id), token.as_bytes())
            .await?;
        self._internal_storage_update(&user_id, "_is_initialized", &[0])
            .await?;
        let init_user_id = user_id.clone();
        let init_user_jwt = token.clone();
        tokio::spawn(async move {
            match user_init(service, &init_user_id, &init_user_jwt).await {
                Ok(_) => {}
                Err(err) => error!("user_init: {}", err.to_string()),
            }
            Ok::<(), Box<dyn std::error::Error + Send + Sync + 'static>>(())
        });
        self.imported_users.write().await.insert(user_id);
        let reply = Jwt { jwt: token };
        Ok(Response::new(reply))
    }
}

impl Interceptor for CheckAuthInterceptor {
    fn call(&mut self, req: Request<()>) -> Result<Request<()>, Status> {
        debug!("Intercepting request: {:?}", req);

        let token = match req.metadata().get("authorization") {
            Some(t) => {
                debug!("The authorization header is: {}", t.to_str().unwrap());
                t.to_str().unwrap()
            }
            None => {
                debug!("Debug: No valid auth token");
                return Err(Status::unauthenticated("No valid auth token"));
            }
        };

        if token.is_empty() {
            let mut req = req;
            req.metadata_mut()
                .insert("privilege", MetadataValue::from_static("anonymous"));
            return Ok(req);
        }
        let token = match jsonwebtoken::decode::<AuthContent>(
            token,
            &DecodingKey::from_secret(&self.jwt_secret),
            &Validation::default(),
        ) {
            Ok(token_data) => token_data,
            Err(e) => {
                return Err(Status::unauthenticated(format!(
                    "Debug: wrong secret or token has expired. {}",
                    e
                )));
            }
        };

        let mut req = req;
        req.metadata_mut()
            .insert("privilege", token.claims.privilege.parse().unwrap());
        req.metadata_mut()
            .insert("user_id", token.claims.user_id.parse().unwrap());

        Ok(req)
    }
}

pub fn gen_jwt_secret() -> [u8; 32] {
    let mut jwt_secret: [u8; 32] = [0; 32];
    let mut rng = rand::thread_rng();
    rng.fill_bytes(&mut jwt_secret);
    debug!("JWT secret: {:?}", jwt_secret);
    jwt_secret
}

pub fn get_host_token(jwt_secret: [u8; 32], host_id: &str) -> String {
    let exp = chrono::Utc::now() + chrono::Duration::days(31);
    let auth_content = AuthContent {
        privilege: "host".to_string(),
        user_id: host_id.to_string(),
        exp: exp.timestamp(),
    };
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &auth_content,
        &jsonwebtoken::EncodingKey::from_secret(&jwt_secret),
    )
    .unwrap()
}

pub async fn print_host_token(jwt_secret: [u8; 32], host_id: String) {
    // This should update every 24 hours in production code, but now we're just writing it to a file.
    let token = get_host_token(jwt_secret, &host_id);
    std::fs::write("host_token.txt", token.clone()).unwrap();
    debug!("{}", token);
}
