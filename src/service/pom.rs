use super::utils::{download_tgz, fetch_from_git};
use crate::colink_proto::*;
use prost::Message;
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
use toml::Value;
use tonic::{Request, Response, Status};
use tracing::error;
use uuid::Uuid;

impl crate::server::MyService {
    pub async fn _start_protocol_operator(
        &self,
        request: Request<StartProtocolOperatorRequest>,
    ) -> Result<Response<ProtocolOperatorInstanceId>, Status> {
        Self::check_privilege_in(request.metadata(), &["user", "host"])?;
        let privilege = Self::get_key_from_metadata(request.metadata(), "privilege");
        if privilege != "host"
            && Self::get_key_from_metadata(request.metadata(), "user_id")
                != request.get_ref().user_id
        {
            return Err(Status::permission_denied(""));
        }
        // prepare information for starting the instance
        if self.core_uri.is_none() {
            return Err(Status::internal("core_uri not found."));
        }
        let core_addr = self.core_uri.as_ref().unwrap();
        let user_jwt = self
            ._host_storage_read(&format!("users:{}:user_jwt", request.get_ref().user_id))
            .await?;
        let user_jwt = String::from_utf8(user_jwt).unwrap();
        let instance_id = Uuid::new_v4();
        let protocol_name: &str = &request.get_ref().protocol_name;
        // check protocol_name
        let file_name = Path::new(&protocol_name).file_name();
        if file_name.is_none() || file_name.unwrap() != protocol_name {
            return Err(Status::invalid_argument("protocol_name is invalid."));
        }
        // create protocols directory if not exist
        let colink_home = self.get_colink_home()?;
        if !Path::new(&colink_home).join("protocols").exists() {
            match std::fs::create_dir_all(Path::new(&colink_home).join("protocols")) {
                Ok(_) => {}
                Err(err) => return Err(Status::internal(err.to_string())),
            }
        }
        let protocol_package_dir = Path::new(&colink_home)
            .join("protocols")
            .join(protocol_name);
        let lock = self.lock(&format!("_pom:{}", protocol_name)).await?;
        // use a closure to catch errors and unlock the lock after this closure
        let res = async {
            // read running instances in user storage
            let running_instances_key = format!("protocol_operator_groups:{}", protocol_name);
            let mut running_instances = if self
                ._internal_storage_contains(&request.get_ref().user_id, &running_instances_key)
                .await?
            {
                let running_instances = self
                    ._internal_storage_read(&request.get_ref().user_id, &running_instances_key)
                    .await?;
                Message::decode(&*running_instances).unwrap()
            } else {
                ListOfString { list: vec![] }
            };
            // upgrade protocol package if requested and there are no running instances
            if request.get_ref().upgrade {
                if running_instances.list.is_empty() {
                    match std::fs::remove_dir_all(&protocol_package_dir) {
                        Ok(_) => {}
                        Err(err) => return Err(Status::internal(err.to_string())),
                    }
                } else {
                    return Err(Status::aborted(format!(
                        "Protocol {} has running instances.",
                        protocol_name
                    )));
                }
            }
            // fetch protocol package from inventory if not exist
            let colink_toml_path = protocol_package_dir.join("colink.toml");
            if std::fs::metadata(&colink_toml_path).is_err() {
                match fetch_protocol_from_inventory(protocol_name, &colink_home).await {
                    Ok(_) => {}
                    Err(err) => {
                        return Err(Status::not_found(&format!(
                            "protocol {} not found: {}",
                            protocol_name, err
                        )));
                    }
                }
            }
            // parse colink.toml
            let colink_toml = match std::fs::read_to_string(&colink_toml_path)
                .unwrap()
                .parse::<Value>()
            {
                Ok(toml) => toml,
                Err(err) => return Err(Status::internal(err.to_string())),
            };
            if colink_toml.get("package").is_none()
                || colink_toml["package"].get("entrypoint").is_none()
            {
                return Err(Status::not_found("entrypoint not found."));
            }
            let entrypoint = colink_toml["package"]["entrypoint"].as_str();
            if entrypoint.is_none() {
                return Err(Status::not_found("entrypoint not found."));
            }
            let entrypoint = entrypoint.unwrap();
            // install dependencies
            if colink_toml["package"].get("install_script").is_some() {
                let install_script = colink_toml["package"]["install_script"].as_str();
                if install_script.is_none() {
                    return Err(Status::internal("invalid install_script"));
                }
                let install_script = install_script.unwrap();
                let install_timestamp = protocol_package_dir.join(".install_timestamp");
                if std::fs::metadata(&install_timestamp).is_err() {
                    match Command::new("bash")
                        .arg("-c")
                        .arg(install_script)
                        .current_dir(&protocol_package_dir)
                        .output()
                    {
                        Ok(output) => {
                            if !output.status.success() {
                                error!("install_script fail: {:?}", output.stderr);
                                return Err(Status::internal("install_script fail".to_string()));
                            }
                        }
                        Err(err) => return Err(Status::internal(err.to_string())),
                    };
                    let mut file = std::fs::File::create(install_timestamp).unwrap();
                    file.write_all(chrono::Utc::now().timestamp().to_string().as_bytes())
                        .unwrap();
                }
            }
            // start instance
            let process = match Command::new("bash")
                .arg("-c")
                .arg(entrypoint)
                .current_dir(&protocol_package_dir)
                .env("COLINK_CORE_ADDR", core_addr)
                .env("COLINK_JWT", user_jwt)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(child) => child,
                Err(err) => return Err(Status::internal(err.to_string())),
            };
            let pid = process.id().to_string();
            // update instance information in host storage
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
            self._host_storage_update(
                &format!("protocol_operator_instances:{}:protocol_name", instance_id),
                protocol_name.as_bytes(),
            )
            .await?;
            // update running instances in user storage
            running_instances.list.push(instance_id.to_string());
            let mut payload = vec![];
            running_instances.encode(&mut payload).unwrap();
            self._internal_storage_update(
                &request.get_ref().user_id,
                &running_instances_key,
                &payload,
            )
            .await?;
            Ok::<(), Status>(())
        }
        .await;
        self.unlock(lock).await?;
        res?;
        Ok(Response::new(ProtocolOperatorInstanceId {
            instance_id: instance_id.to_string(),
        }))
    }

    pub async fn _stop_protocol_operator(
        &self,
        request: Request<ProtocolOperatorInstanceId>,
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
        // kill the instance
        match Command::new("kill").args(["-9", &pid]).output() {
            Ok(output) => {
                if !output.status.success() {
                    error!("cannot kill the instance: {:?}", output.stderr);
                    return Err(Status::internal("cannot kill the instance".to_string()));
                }
            }
            Err(err) => return Err(Status::internal(err.to_string())),
        };
        // update running instances in user storage
        let protocol_name = self
            ._host_storage_read(&format!(
                "protocol_operator_instances:{}:protocol_name",
                request.get_ref().instance_id
            ))
            .await?;
        let protocol_name = String::from_utf8(protocol_name).unwrap();
        let running_instances_key = format!("protocol_operator_groups:{}", protocol_name);
        let lock = self.lock(&format!("_pom:{}", protocol_name)).await?;
        let res = async {
            let mut running_instances: ListOfString = {
                let running_instances = self
                    ._internal_storage_read(&user_id, &running_instances_key)
                    .await?;
                Message::decode(&*running_instances).unwrap()
            };
            running_instances
                .list
                .retain(|x| x != &request.get_ref().instance_id);
            let mut payload = vec![];
            running_instances.encode(&mut payload).unwrap();
            self._internal_storage_update(&user_id, &running_instances_key, &payload)
                .await?;
            Ok::<(), Status>(())
        }
        .await;
        self.unlock(lock).await?;
        res?;
        Ok(Response::new(Empty::default()))
    }
}

const PROTOCOL_INVENTORY: &str =
    "https://raw.githubusercontent.com/CoLearn-Dev/colink-protocol-inventory/main/protocols";
async fn fetch_protocol_from_inventory(
    protocol_name: &str,
    colink_home: &str,
) -> Result<(), String> {
    let url = &format!("{}/{}.toml", PROTOCOL_INVENTORY, protocol_name);
    let http_client = reqwest::Client::new();
    let resp = http_client.get(url).send().await;
    if resp.is_err() || resp.as_ref().unwrap().status() != reqwest::StatusCode::OK {
        return Err(format!(
            "fail to find protocol {} in inventory",
            protocol_name
        ));
    }
    let inventory_toml = match resp.unwrap().text().await {
        Ok(toml) => match toml.parse::<Value>() {
            Ok(toml) => toml,
            Err(err) => return Err(err.to_string()),
        },
        Err(err) => {
            return Err(err.to_string());
        }
    };
    let protocol_package_dir = Path::new(&colink_home)
        .join("protocols")
        .join(protocol_name);
    if inventory_toml.get("binary").is_some()
        && inventory_toml["binary"]
            .get(&format!(
                "{}-{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            ))
            .is_some()
    {
        if let Some(binary) = inventory_toml["binary"]
            [&format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)]
            .as_table()
        {
            if binary.get("url").is_some()
                && binary["url"].as_str().is_some()
                && binary.get("sha256").is_some()
                && binary["sha256"].as_str().is_some()
            {
                download_tgz(
                    binary["url"].as_str().unwrap(),
                    binary["sha256"].as_str().unwrap(),
                    protocol_package_dir.to_str().unwrap(),
                )
                .await?;
                return Ok(());
            }
        }
    }
    if inventory_toml.get("source").is_some() {
        if inventory_toml["source"].get("archive").is_some() {
            if let Some(source) = inventory_toml["source"]["archive"].as_table() {
                if source.get("url").is_some()
                    && source["url"].as_str().is_some()
                    && source.get("sha256").is_some()
                    && source["sha256"].as_str().is_some()
                {
                    download_tgz(
                        source["url"].as_str().unwrap(),
                        source["sha256"].as_str().unwrap(),
                        protocol_package_dir.to_str().unwrap(),
                    )
                    .await?;
                    return Ok(());
                }
            }
        }
        if inventory_toml["source"].get("git").is_some() {
            if let Some(source) = inventory_toml["source"]["git"].as_table() {
                if source.get("url").is_some()
                    && source["url"].as_str().is_some()
                    && source.get("commit").is_some()
                    && source["commit"].as_str().is_some()
                {
                    fetch_from_git(
                        source["url"].as_str().unwrap(),
                        source["commit"].as_str().unwrap(),
                        protocol_package_dir.to_str().unwrap(),
                    )
                    .await?;
                    return Ok(());
                }
            }
        }
    }
    Err(format!(
        "the inventory file of protocol {} is damaged",
        protocol_name
    ))
}
