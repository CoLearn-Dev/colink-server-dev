use crate::colink_proto::{co_link_client::CoLinkClient, UserConsent};
use openssl::sha::sha256;
use secp256k1::{ecdsa::Signature, PublicKey, Secp256k1};
use tonic::{
    metadata::{MetadataMap, MetadataValue},
    transport::{Certificate, Channel, ClientTlsConfig, Identity},
    Status,
};

impl crate::server::MyService {
    pub fn ca_certificate(self, ca_certificate: &str) -> Self {
        let ca_certificate = std::fs::read(ca_certificate).unwrap();
        let ca_certificate = Certificate::from_pem(ca_certificate);
        Self {
            inter_core_ca_certificate: Some(ca_certificate),
            ..self
        }
    }

    pub fn identity(self, client_cert: &str, client_key: &str) -> Self {
        let client_cert = std::fs::read(client_cert).unwrap();
        let client_key = std::fs::read(client_key).unwrap();
        let identity = Identity::from_pem(client_cert, client_key);
        Self {
            inter_core_identity: Some(identity),
            ..self
        }
    }

    pub async fn _grpc_connect(
        &self,
        address: &str,
    ) -> Result<CoLinkClient<Channel>, Box<dyn std::error::Error>> {
        let channel =
            if self.inter_core_ca_certificate.is_none() && self.inter_core_identity.is_none() {
                Channel::builder(address.parse()?).connect().await?
            } else {
                let mut tls = ClientTlsConfig::new();
                if self.inter_core_ca_certificate.is_some() {
                    tls = tls.ca_certificate(self.inter_core_ca_certificate.clone().unwrap());
                }
                if self.inter_core_identity.is_some() {
                    tls = tls.identity(self.inter_core_identity.clone().unwrap());
                }
                Channel::builder(address.parse()?)
                    .tls_config(tls)?
                    .connect()
                    .await?
            };
        let client = CoLinkClient::new(channel);
        Ok(client)
    }

    pub fn check_admin_token(request_metadata: &MetadataMap) -> Result<(), Status> {
        let role = request_metadata.get("role").unwrap().to_str().unwrap();
        if role != "admin" {
            Err(Status::permission_denied(
                "This procedure requires an admin token, which you did not provide.",
            ))
        } else {
            Ok(())
        }
    }

    pub fn check_user_or_admin_token(request_metadata: &MetadataMap) -> Result<(), Status> {
        let role = request_metadata.get("role").unwrap().to_str().unwrap();
        if role != "admin" && role != "user" {
            Err(Status::permission_denied(
                "This procedure needs an admin or user token, which you did not provide.",
            ))
        } else {
            Ok(())
        }
    }

    pub fn check_user_token(request_metadata: &MetadataMap) -> Result<(), Status> {
        let role = request_metadata.get("role").unwrap().to_str().unwrap();
        if role != "user" {
            Err(Status::permission_denied(
                "This procedure needs an admin or user token, which you did not provide.",
            ))
        } else {
            Ok(())
        }
    }

    pub fn get_user_id(request_metadata: &MetadataMap) -> String {
        request_metadata
            .get("user_id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }

    pub async fn _internal_storage_update(
        &self,
        user_id: &str,
        key_name: &str,
        payload: &[u8],
    ) -> Result<String, Status> {
        let key_name = format!("_internal:{}", key_name);
        match self.storage.update(user_id, &key_name, payload).await {
            Ok(key_path) => Ok(key_path),
            Err(e) => Err(Status::internal(e)),
        }
    }

    pub async fn _internal_storage_read(
        &self,
        user_id: &str,
        key_name: &str,
    ) -> Result<Vec<u8>, Status> {
        let real_key_name = format!("_internal:{}", key_name);
        let entries = match self
            .storage
            .read_from_key_names(user_id, &[real_key_name])
            .await
        {
            Ok(entries) => entries,
            Err(e) => return Err(Status::internal(e)),
        };
        let payload = entries.values().next().unwrap();
        Ok(payload.to_vec())
    }

    pub async fn _internal_storage_contains(
        &self,
        user_id: &str,
        key_name: &str,
    ) -> Result<bool, Status> {
        let key_name = format!("_internal:{}", key_name);
        Ok(self
            .storage
            .read_from_key_names(user_id, &[key_name])
            .await
            .is_ok())
    }

    pub fn check_user_consent(
        &self,
        user_consent: &UserConsent,
        core_public_key_vec: &[u8],
    ) -> Result<(), Status> {
        let user_consent_signature_timestamp: i64 = user_consent.signature_timestamp;
        let user_consent_expiration_timestamp: i64 = user_consent.expiration_timestamp;
        let user_consent_signature: &Vec<u8> = &user_consent.signature;
        let user_consent_signature = match Signature::from_compact(user_consent_signature) {
            Ok(sig) => sig,
            Err(e) => {
                return Err(Status::invalid_argument(format!(
                    "The user consent signature could not be decoded in ECDSA: {}",
                    e
                )))
            }
        };
        let mut user_public_key_vec: Vec<u8> = user_consent.clone().public_key;
        let user_public_key: PublicKey = match PublicKey::from_slice(&user_public_key_vec) {
            Ok(pk) => pk,
            Err(e) => {
                return Err(Status::invalid_argument(format!(
                    "The public key could not be decoded in compressed serialized format: {:?}",
                    e
                )))
            }
        };
        user_public_key_vec.extend_from_slice(&user_consent_signature_timestamp.to_le_bytes());
        user_public_key_vec.extend_from_slice(&user_consent_expiration_timestamp.to_le_bytes());
        user_public_key_vec.extend_from_slice(core_public_key_vec);
        let verify_consent_signature =
            secp256k1::Message::from_slice(&sha256(&user_public_key_vec)).unwrap();
        let secp = Secp256k1::new();
        match secp.verify_ecdsa(
            &verify_consent_signature,
            &user_consent_signature,
            &user_public_key,
        ) {
            Ok(_) => {}
            Err(e) => {
                return Err(Status::invalid_argument(format!(
                    "Invalid User Consent Signature: {}",
                    e
                )))
            }
        }
        Ok(())
    }
}

pub fn generate_request<T>(jwt: &str, data: T) -> tonic::Request<T> {
    let mut request = tonic::Request::new(data);
    let user_token = MetadataValue::try_from(jwt).unwrap();
    request.metadata_mut().insert("authorization", user_token);
    request
}
