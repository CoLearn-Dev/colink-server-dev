use crate::colink_proto::{co_link_client::CoLinkClient, UserConsent};
use futures_lite::StreamExt;
use secp256k1::{
    ecdsa::{RecoverableSignature, RecoveryId, Signature},
    PublicKey, Secp256k1,
};
use sha2::{Digest, Sha256};
use sha3::Keccak256;
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
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

    pub async fn check_privilege_in(
        &self,
        request_metadata: &MetadataMap,
        privileges: &[&str],
    ) -> Result<(), Status> {
        let privilege = request_metadata.get("privilege").unwrap().to_str().unwrap();
        if privileges.contains(&privilege) {
            let user_id = request_metadata.get("user_id").unwrap().to_str().unwrap();
            if ["user", "guest"].contains(&privilege)
                && !self.imported_users.read().await.contains(user_id)
            {
                Err(Status::permission_denied(format!(
                    "User {} in the JWT is created before the latest server start.",
                    user_id
                )))
            } else {
                Ok(())
            }
        } else {
            Err(Status::permission_denied(format!(
                "This procedure requires specific privileges[{:?}], while you provide[{}].",
                privileges, privilege
            )))
        }
    }

    pub fn get_key_from_metadata(request_metadata: &MetadataMap, key: &str) -> String {
        request_metadata
            .get(key)
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

    pub async fn _host_storage_create(
        &self,
        key_name: &str,
        payload: &[u8],
    ) -> Result<String, Status> {
        match self
            .storage
            .create(&self.get_host_id(), key_name, payload)
            .await
        {
            Ok(key_path) => Ok(key_path),
            Err(e) => Err(Status::internal(e)),
        }
    }

    pub async fn _host_storage_update(
        &self,
        key_name: &str,
        payload: &[u8],
    ) -> Result<String, Status> {
        match self
            .storage
            .update(&self.get_host_id(), key_name, payload)
            .await
        {
            Ok(key_path) => Ok(key_path),
            Err(e) => Err(Status::internal(e)),
        }
    }

    pub async fn _host_storage_read(&self, key_name: &str) -> Result<Vec<u8>, Status> {
        let entries = match self
            .storage
            .read_from_key_names(&self.get_host_id(), &[key_name.to_owned()])
            .await
        {
            Ok(entries) => entries,
            Err(e) => return Err(Status::internal(e)),
        };
        let payload = entries.values().next().unwrap();
        Ok(payload.to_vec())
    }

    pub async fn _host_storage_delete(&self, key_name: &str) -> Result<String, Status> {
        match self.storage.delete(&self.get_host_id(), key_name).await {
            Ok(key_path) => Ok(key_path),
            Err(e) => Err(Status::internal(e)),
        }
    }

    pub async fn _user_storage_update(
        &self,
        user_id: &str,
        key_name: &str,
        payload: &[u8],
    ) -> Result<String, Status> {
        match self.storage.update(user_id, key_name, payload).await {
            Ok(key_path) => Ok(key_path),
            Err(e) => Err(Status::internal(e)),
        }
    }

    pub async fn _user_storage_read(
        &self,
        user_id: &str,
        key_name: &str,
    ) -> Result<Vec<u8>, Status> {
        let entries = match self
            .storage
            .read_from_key_names(user_id, &[key_name.to_owned()])
            .await
        {
            Ok(entries) => entries,
            Err(e) => return Err(Status::internal(e)),
        };
        let payload = entries.values().next().unwrap();
        Ok(payload.to_vec())
    }

    pub fn check_user_consent(
        &self,
        user_consent: &UserConsent,
        core_public_key_vec: &[u8],
    ) -> Result<Vec<u8>, Status> {
        let user_consent_signature_timestamp: i64 = user_consent.signature_timestamp;
        let user_consent_expiration_timestamp: i64 = user_consent.expiration_timestamp;
        let user_consent_signature: &Vec<u8> = &user_consent.signature;
        if !user_consent.public_key.is_empty() {
            // Non-MetaMask
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
                secp256k1::Message::from_slice(&Sha256::digest(&user_public_key_vec)).unwrap();
            let secp = Secp256k1::new();
            match secp.verify_ecdsa(
                &verify_consent_signature,
                &user_consent_signature,
                &user_public_key,
            ) {
                Ok(_) => Ok(user_consent.clone().public_key),
                Err(e) => Err(Status::invalid_argument(format!(
                    "Invalid User Consent Signature: {}",
                    e
                ))),
            }
        } else {
            // MetaMask
            let msg = format!(
                "sigTime: {}\nexpTime: {}\ncorePubKey: {}\n",
                user_consent_signature_timestamp,
                user_consent_expiration_timestamp,
                hex::encode(core_public_key_vec)
            );
            let msg = secp256k1::Message::from_slice(&Keccak256::digest(
                format!("{}{}{}", "\x19Ethereum Signed Message:\n", msg.len(), msg).as_bytes(),
            ))
            .unwrap();
            let signature = match RecoverableSignature::from_compact(
                &user_consent_signature[..64],
                RecoveryId::from_i32(user_consent_signature[64] as i32 - 27).unwrap(),
            ) {
                Ok(sig) => sig,
                Err(e) => {
                    return Err(Status::invalid_argument(format!(
                        "The user consent signature could not be decoded in ECDSA: {}",
                        e
                    )))
                }
            };
            let secp = Secp256k1::new();
            match secp.recover_ecdsa(&msg, &signature) {
                Ok(public_key) => Ok(public_key.serialize().to_vec()),
                Err(e) => Err(Status::invalid_argument(format!(
                    "Invalid User Consent Signature: {}",
                    e
                ))),
            }
        }
    }

    pub fn get_host_id(&self) -> String {
        hex::encode(self.public_key.serialize())
    }

    pub fn find_resource_file(&self, file_name: &str) -> Result<PathBuf, Status> {
        let colink_home = match get_colink_home() {
            Ok(colink_home) => colink_home,
            Err(e) => return Err(Status::not_found(e)),
        };
        let mut path = Path::new(file_name).to_path_buf();
        if std::fs::metadata(&path).is_err() {
            path = Path::new(&colink_home).join(file_name);
        }
        if std::fs::metadata(&path).is_ok() {
            Ok(path)
        } else {
            Err(Status::not_found(file_name.to_string()))
        }
    }
}

pub fn get_colink_home() -> Result<String, String> {
    let colink_home = if std::env::var("COLINK_HOME").is_ok() {
        std::env::var("COLINK_HOME").unwrap()
    } else if std::env::var("HOME").is_ok() {
        std::env::var("HOME").unwrap() + "/.colink"
    } else {
        return Err("colink home not found.".to_string());
    };
    Ok(colink_home)
}

pub fn generate_request<T>(jwt: &str, data: T) -> tonic::Request<T> {
    let mut request = tonic::Request::new(data);
    let user_token = MetadataValue::try_from(jwt).unwrap();
    request.metadata_mut().insert("authorization", user_token);
    request
}

pub async fn fetch_from_git(url: &str, commit: &str, path: &str) -> Result<(), String> {
    let git_clone = match Command::new("git")
        .args(["clone", "--recursive", url, path])
        .output()
    {
        Ok(git_clone) => git_clone,
        Err(err) => return Err(err.to_string()),
    };
    if !git_clone.status.success() {
        return Err(format!("fail to fetch from {}", url));
    }
    if !commit.is_empty() {
        let git_checkout = match Command::new("git")
            .args(["checkout", commit])
            .current_dir(path)
            .output()
        {
            Ok(git_checkout) => git_checkout,
            Err(err) => return Err(err.to_string()),
        };
        if !git_checkout.status.success() {
            return Err(format!("checkout error: commit {}", commit));
        }
    }
    Ok(())
}

pub async fn download_tgz(url: &str, sha256: &str, path: &str) -> Result<(), String> {
    let mut file = match tempfile::tempfile() {
        Ok(file) => file,
        Err(err) => return Err(err.to_string()),
    };
    let http_client = reqwest::Client::new();
    let res = match http_client.get(url).send().await {
        Ok(res) => res,
        Err(err) => return Err(err.to_string()),
    };
    let mut stream = res.bytes_stream();
    while let Some(item) = stream.next().await {
        let chunk = match item {
            Ok(chunk) => chunk,
            Err(err) => return Err(err.to_string()),
        };
        match file.write_all(&chunk) {
            Ok(_) => {}
            Err(err) => return Err(err.to_string()),
        };
    }
    if !sha256.is_empty() {
        match file.seek(SeekFrom::Start(0)) {
            Ok(_) => {}
            Err(err) => return Err(err.to_string()),
        }
        let mut hasher = Sha256::new();
        match io::copy(&mut file, &mut hasher) {
            Ok(_) => {}
            Err(err) => return Err(err.to_string()),
        };
        let hash = hasher.finalize();
        let file_sha256 = hex::encode(hash);
        if file_sha256 != sha256 {
            return Err(format!(
                "file checksum mismatch: expected {}, actual {}",
                sha256, file_sha256
            ));
        }
    }
    match file.seek(SeekFrom::Start(0)) {
        Ok(_) => {}
        Err(err) => return Err(err.to_string()),
    }
    let tar = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(tar);
    match archive.unpack(path) {
        Ok(_) => {}
        Err(err) => return Err(err.to_string()),
    };
    Ok(())
}
