use crate::colink_proto::*;
use chrono::TimeZone;
use jsonwebtoken::{DecodingKey, Validation};
use prost::Message;
use rand::RngCore;
use secp256k1::PublicKey;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use tonic::{metadata::MetadataValue, service::Interceptor, Request, Response, Status};
use tracing::debug;

#[derive(Clone)]
pub struct CheckAuthInterceptor {
    pub jwt_secret: [u8; 32],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthContent {
    role: String,
    user_id: String,
    exp: i64,
}

impl crate::server::MyService {
    pub async fn _refresh_token(
        &self,
        request: Request<RefreshTokenRequest>,
    ) -> Result<Response<Jwt>, Status> {
        debug!("Got a request: {:?}", request);
        Self::check_user_token(request.metadata())?;
        let token = request.metadata().get("authorization").unwrap().clone();
        let token = token.to_str().unwrap();
        let body: RefreshTokenRequest = request.into_inner();
        let token = jsonwebtoken::decode::<AuthContent>(
            token,
            &jsonwebtoken::DecodingKey::from_secret(&self.jwt_secret),
            &jsonwebtoken::Validation::default(),
        )
        .unwrap();
        let token = token.claims;
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &AuthContent {
                role: token.role,
                user_id: token.user_id,
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
    ) -> Result<Response<Jwt>, Status> {
        Self::check_admin_token(request.metadata())?;
        let body: UserConsent = request.into_inner();
        let user_consent_to_be_stored: UserConsent = body.clone();
        let user_consent_to_be_checked: UserConsent = body.clone();
        let signature_timestamp: i64 = body.signature_timestamp;
        let expiration_timestamp: i64 = body.expiration_timestamp;
        let user_public_key_vec: Vec<u8> = body.public_key;
        let user_public_key: PublicKey = match PublicKey::from_slice(&user_public_key_vec) {
            Ok(pk) => pk,
            Err(e) => {
                return Err(Status::invalid_argument(format!(
                    "The public key could not be decoded in compressed serialized format: {:?}",
                    e
                )))
            }
        };
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
        self.check_user_consent(&user_consent_to_be_checked, &self.public_key.serialize())?;
        let user_id = base64::encode(&user_public_key.serialize());
        let mq_uri = match self.mq.create_user_account().await {
            Ok(mq_uri) => mq_uri,
            Err(e) => return Err(Status::internal(e)),
        };
        let mut user_consent_bytes: Vec<u8> = vec![];
        user_consent_to_be_stored
            .encode(&mut user_consent_bytes)
            .unwrap();
        self._internal_storage_update(&user_id, "user_consent", &user_consent_bytes)
            .await?;
        self._internal_storage_update(&user_id, "mq_uri", mq_uri.as_bytes())
            .await?;

        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &AuthContent {
                role: "user".to_string(),
                user_id,
                exp: expiration_timestamp,
            },
            &jsonwebtoken::EncodingKey::from_secret(&self.jwt_secret),
        )
        .unwrap();
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
                .insert("role", MetadataValue::from_static("anonymous"));
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
            .insert("role", token.claims.role.parse().unwrap());
        req.metadata_mut()
            .insert("user_id", token.claims.user_id.parse().unwrap());

        Ok(req)
    }
}

pub fn gen_jwt() -> [u8; 32] {
    let mut jwt_secret: [u8; 32] = [0; 32];
    let mut rng = rand::thread_rng();
    rng.fill_bytes(&mut jwt_secret);
    debug!("JWT secret: {:?}", jwt_secret);
    jwt_secret
}

pub fn get_admin_token(jwt_secret: [u8; 32]) -> String {
    let exp = chrono::Utc::now() + chrono::Duration::days(31);
    let auth_content = AuthContent {
        role: "admin".to_string(),
        user_id: "_admin".to_string(),
        exp: exp.timestamp(),
    };
    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &auth_content,
        &jsonwebtoken::EncodingKey::from_secret(&jwt_secret),
    )
    .unwrap()
}

pub async fn print_admin_token(jwt_secret: [u8; 32]) {
    // This should update every 24 hours in production code, but now we're just writing it to a file.
    let token = get_admin_token(jwt_secret);
    std::fs::write("admin_token.txt", token.clone()).unwrap();
    debug!("{}", token);
}
