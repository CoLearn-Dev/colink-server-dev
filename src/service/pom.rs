use crate::colink_proto::*;
use std::{
    path::Path,
    process::{Command, Stdio},
};
use toml::Value;
use tonic::{Request, Response, Status};
use uuid::Uuid;

impl crate::server::MyService {
    pub async fn _start_protocol_operator(
        &self,
        request: Request<ProtocolOperatorInstance>,
    ) -> Result<Response<ProtocolOperatorInstance>, Status> {
        Self::check_privilege_in(request.metadata(), &["user", "host"])?;
        let privilege = Self::get_key_from_metadata(request.metadata(), "privilege");
        if privilege != "host"
            && Self::get_key_from_metadata(request.metadata(), "user_id")
                != request.get_ref().user_id
        {
            return Err(Status::permission_denied(""));
        }
        let instance_id = Uuid::new_v4();
        let protocol_name: &str = &request.get_ref().protocol_name;
        let file_name = Path::new(&protocol_name).file_name();
        if file_name.is_none() || file_name.unwrap() != protocol_name {
            return Err(Status::invalid_argument("protocol_name is invalid."));
        }
        let colink_home = if std::env::var("COLINK_HOME").is_ok() {
            std::env::var("COLINK_HOME").unwrap()
        } else if std::env::var("HOME").is_ok() {
            std::env::var("HOME").unwrap() + "/.colink"
        } else {
            return Err(Status::not_found("colink home not found."));
        };
        let path = Path::new(&colink_home)
            .join("protocols")
            .join(protocol_name)
            .join("colink.toml");
        if std::fs::metadata(&path).is_err() {
            return Err(Status::not_found("protocol not found."));
        }
        let toml = std::fs::read_to_string(&path)
            .unwrap()
            .parse::<Value>()
            .unwrap();
        if self.core_uri.is_none() {
            return Err(Status::internal("core_uri not found."));
        }
        let core_addr = self.core_uri.as_ref().unwrap();
        let entrypoint = toml["package"]["entrypoint"].as_str();
        if entrypoint.is_none() {
            return Err(Status::not_found("entrypoint not found."));
        }
        let entrypoint = entrypoint.unwrap();
        let user_jwt = self
            ._host_storage_read(&format!("users:{}:user_jwt", request.get_ref().user_id))
            .await?;
        let user_jwt = String::from_utf8(user_jwt).unwrap();
        let process = Command::new("bash")
            .arg("-c")
            .arg(&*entrypoint)
            .current_dir(
                Path::new(&colink_home)
                    .join("protocols")
                    .join(protocol_name),
            )
            .env("COLINK_CORE_ADDR", core_addr)
            .env("COLINK_JWT", user_jwt)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pid = process.id().to_string();
        self._host_storage_update(
            &format!("protocol_operator_instances:{}:user_id", instance_id),
            request.get_ref().user_id.as_bytes(),
        )
        .await?;
        self._host_storage_update(
            &format!("protocol_operator_instances:{}:pid", instance_id),
            pid.as_bytes(),
        )
        .await?;
        Ok(Response::new(ProtocolOperatorInstance {
            instance_id: instance_id.to_string(),
            ..Default::default()
        }))
    }

    pub async fn _stop_protocol_operator(
        &self,
        request: Request<ProtocolOperatorInstance>,
    ) -> Result<Response<Empty>, Status> {
        Self::check_privilege_in(request.metadata(), &["user", "host"])?;
        let privilege = Self::get_key_from_metadata(request.metadata(), "privilege");
        let user_id = self
            ._host_storage_read(&format!(
                "protocol_operator_instances:{}:user_id",
                request.get_ref().instance_id
            ))
            .await?;
        let user_id = String::from_utf8(user_id).unwrap();
        if privilege != "host"
            && Self::get_key_from_metadata(request.metadata(), "user_id") != user_id
        {
            return Err(Status::permission_denied(""));
        }
        let pid = self
            ._host_storage_read(&format!(
                "protocol_operator_instances:{}:pid",
                request.get_ref().instance_id
            ))
            .await?;
        let pid = String::from_utf8(pid).unwrap();
        Command::new("kill")
            .args(["-9", &pid])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        Ok(Response::new(Empty::default()))
    }
}
