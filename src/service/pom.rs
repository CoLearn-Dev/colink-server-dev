mod fetch_protocol;
use self::fetch_protocol::fetch_protocol;
use super::utils::get_colink_home;
use crate::colink_proto::*;
use fs4::FileExt;
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
        self.check_privilege_in(request.metadata(), &["user", "host"])
            .await?;
        let privilege = Self::get_key_from_metadata(request.metadata(), "privilege");
        if privilege != "host"
            && Self::get_key_from_metadata(request.metadata(), "user_id")
                != request.get_ref().user_id
        {
            return Err(Status::permission_denied(""));
        }
        // prepare CLI param to start PO instance
        if self.params.core_uri.is_none() {
            return Err(Status::internal("core_uri not found."));
        }
        let core_addr = self.params.core_uri.as_ref().unwrap();
        let user_jwt = self
            ._host_storage_read(&format!("users:{}:user_jwt", request.get_ref().user_id))
            .await?;
        let user_jwt = String::from_utf8(user_jwt).unwrap();
        let instance_id = Uuid::new_v4();
        let protocol_name: &str = &request.get_ref().protocol_name;
        // check protocol_name
        let protocol_name_parsed = Path::new(&protocol_name).file_name();
        if protocol_name_parsed.is_none() || protocol_name_parsed.unwrap() != protocol_name {
            return Err(Status::invalid_argument("protocol_name is invalid."));
        }
        // create protocols directory if not exist
        let colink_home = match get_colink_home() {
            Ok(colink_home) => colink_home,
            Err(e) => return Err(Status::not_found(e)),
        };
        if !Path::new(&colink_home).join("protocols").exists() {
            match std::fs::create_dir_all(Path::new(&colink_home).join("protocols")) {
                Ok(_) => {}
                Err(err) => return Err(Status::internal(err.to_string())),
            }
        }
        let protocol_package_dir = Path::new(&colink_home)
            .join("protocols")
            .join(protocol_name);
        let protocol_package_lock = get_file_lock(&colink_home, protocol_name)?;
        let protocol_package_lock = tokio::task::spawn_blocking(move || {
            protocol_package_lock.lock_exclusive().unwrap();
            protocol_package_lock
        })
        .await
        .unwrap();
        // use a closure to prevent locking forever caused by errors
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
                        "Protocol {} has running instances and cannot be upgraded.",
                        protocol_name
                    )));
                }
            }
            // fetch protocol package if protocol package folder does not exist
            let colink_toml_path = protocol_package_dir.join("colink.toml");
            if std::fs::metadata(&colink_toml_path).is_err() {
                match fetch_protocol(
                    protocol_name,
                    &colink_home,
                    &request.get_ref().source_type,
                    &request.get_ref().source,
                    &self.params.pom_protocol_inventory,
                    self.params.pom_dev_mode,
                )
                .await
                {
                    Ok(_) => {}
                    Err(err) => {
                        return Err(Status::not_found(format!(
                            "fail to fetch protocol {}: {}",
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
            if colink_toml.get("package").is_none() {
                return Err(Status::not_found("package not found."));
            }
            let entrypoint = if let Some(value) = colink_toml["package"].get("entrypoint") {
                value.as_str()
            } else {
                None
            };
            let docker_image = if let Some(value) = colink_toml["package"].get("docker_image") {
                value.as_str()
            } else {
                None
            };
            if entrypoint.is_none() && docker_image.is_none() {
                return Err(Status::not_found("entrypoint or docker_image not found."));
            }
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
            let xid = if let Some(entrypoint) = entrypoint {
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
                process.id().to_string()
            } else if let Some(docker_image) = docker_image {
                let container_id = match Command::new("docker")
                    .args([
                        "run",
                        "-dit",
                        "--rm",
                        "--net=host",
                        "-e",
                        &format!("COLINK_CORE_ADDR={core_addr}"),
                        "-e",
                        &format!("COLINK_JWT={user_jwt}"),
                        docker_image,
                    ])
                    .current_dir(&protocol_package_dir)
                    .output()
                {
                    Ok(output) => String::from_utf8_lossy(&output.stdout).to_string(),
                    Err(err) => return Err(Status::internal(err.to_string())),
                };
                container_id.replace(|c: char| !c.is_ascii_alphanumeric(), "")
            } else {
                unreachable!()
            };
            // update instance information in host storage
            self._host_storage_update(
                &format!("protocol_operator_instances:{}:user_id", instance_id),
                request.get_ref().user_id.as_bytes(),
            )
            .await?;
            self._host_storage_update(
                &format!(
                    "protocol_operator_instances:{}:{}",
                    instance_id,
                    if entrypoint.is_some() {
                        "pid"
                    } else if docker_image.is_some() {
                        "container_id"
                    } else {
                        unreachable!()
                    }
                ),
                xid.as_bytes(),
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
        protocol_package_lock.unlock()?;
        res?;
        Ok(Response::new(ProtocolOperatorInstanceId {
            instance_id: instance_id.to_string(),
        }))
    }

    pub async fn _stop_protocol_operator(
        &self,
        request: Request<ProtocolOperatorInstanceId>,
    ) -> Result<Response<Empty>, Status> {
        self.check_privilege_in(request.metadata(), &["user", "host"])
            .await?;
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
            .await;
        if let Ok(pid) = pid {
            let pid = String::from_utf8(pid).unwrap();
            // kill child process
            let _ = Command::new("pkill").args(["-9", "-P", &pid]).output();
            // kill process
            match Command::new("kill").args(["-9", &pid]).output() {
                Ok(output) => {
                    if !output.status.success() {
                        error!("cannot kill the process: {:?}", output.stderr);
                        return Err(Status::internal("cannot kill the process".to_string()));
                    }
                }
                Err(err) => return Err(Status::internal(err.to_string())),
            };
        } else if let Ok(container_id) = self
            ._host_storage_read(&format!(
                "protocol_operator_instances:{}:container_id",
                request.get_ref().instance_id
            ))
            .await
        {
            let cid = String::from_utf8(container_id).unwrap();
            // kill container
            match Command::new("docker").args(["kill", &cid]).output() {
                Ok(output) => {
                    if !output.status.success() {
                        error!("cannot kill the container: {:?}", output.stderr);
                        return Err(Status::internal("cannot kill the container".to_string()));
                    }
                }
                Err(err) => return Err(Status::internal(err.to_string())),
            };
        } else {
            pid?;
        }
        // update running instances in user storage
        let protocol_name = self
            ._host_storage_read(&format!(
                "protocol_operator_instances:{}:protocol_name",
                request.get_ref().instance_id
            ))
            .await?;
        let protocol_name = String::from_utf8(protocol_name).unwrap();
        let running_instances_key = format!("protocol_operator_groups:{}", protocol_name);
        let colink_home = match get_colink_home() {
            Ok(colink_home) => colink_home,
            Err(e) => return Err(Status::not_found(e)),
        };
        let protocol_package_lock = get_file_lock(&colink_home, &protocol_name)?;
        let protocol_package_lock = tokio::task::spawn_blocking(move || {
            protocol_package_lock.lock_exclusive().unwrap();
            protocol_package_lock
        })
        .await
        .unwrap();
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
        protocol_package_lock.unlock()?;
        res?;
        Ok(Response::new(Empty::default()))
    }
}

fn get_file_lock(colink_home: &str, protocol_name: &str) -> std::io::Result<std::fs::File> {
    let lock_dir = Path::new(&colink_home).join(".lock");
    if !lock_dir.exists() {
        std::fs::create_dir_all(lock_dir.clone())?;
    }
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(lock_dir.join(protocol_name))
}
