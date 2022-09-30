use super::utils::*;
use crate::{colink_proto::*, server::MyService};
use std::{path::Path, sync::Arc};
use toml::Value;

mod colink_policy_module_proto {
    include!(concat!(env!("OUT_DIR"), "/colink_policy_module.rs"));
}

pub async fn user_init(
    service: Arc<MyService>,
    user_id: &str,
    user_jwt: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let colink_home = service.get_colink_home()?;
    let mut path = Path::new(&colink_home).join("user_init_config.toml");
    if std::fs::metadata(&path).is_err() {
        path = Path::new("user_init_config.template.toml").to_path_buf();
    }
    let toml = match std::fs::read_to_string(&path).unwrap().parse::<Value>() {
        Ok(toml) => toml,
        Err(err) => Err(err.to_string())?,
    };
    let mut protocols = toml.as_table().unwrap().clone();
    while !protocols.is_empty() {
        let protocol_num = protocols.len();
        let mut remaining_protocols = protocols.clone();
        for (protocol_name, value) in protocols {
            let mut start = true;
            if value.get("start_after").is_some() && value["start_after"].is_array() {
                let start_after_list = value["start_after"].as_array().unwrap();
                for p in start_after_list {
                    if remaining_protocols.contains_key(&p.as_str().unwrap().to_string()) {
                        start = false;
                    }
                }
            }
            if start {
                if value.get("create_entry").is_some() && value["create_entry"].is_array() {
                    let entries = value["create_entry"].as_array().unwrap();
                    for entry in entries {
                        service
                            ._user_storage_update(
                                user_id,
                                entry["key"].as_str().unwrap(),
                                entry["value"].as_str().unwrap().as_bytes(),
                            )
                            .await?;
                    }
                }
                let is_initialized_key =
                    format!("_internal:protocols:{}:_is_initialized", protocol_name);
                service
                    ._user_storage_update(user_id, &is_initialized_key, &[0])
                    .await?;
                _start_protocol_operator(&service, user_id, user_jwt, &protocol_name).await?;
                loop {
                    let is_initialized = service
                        ._user_storage_read(user_id, &is_initialized_key)
                        .await?[0];
                    if is_initialized == 1 {
                        break;
                    }
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
                remaining_protocols.remove(&protocol_name);
            }
        }
        protocols = remaining_protocols;
        if protocol_num == protocols.len() {
            Err(format!(
                "protocols {:?} cannot start",
                protocols.keys().cloned().collect::<Vec<String>>()
            ))?;
        }
    }
    service
        ._internal_storage_update(user_id, "_is_initialized", &[1])
        .await?;
    Ok(())
}

async fn _start_protocol_operator(
    service: &MyService,
    user_id: &str,
    user_jwt: &str,
    protocol_name: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let mut req = generate_request(
        user_jwt,
        ProtocolOperatorInstance {
            protocol_name: protocol_name.to_string(),
            user_id: user_id.to_string(),
            ..Default::default()
        },
    );
    req.metadata_mut().insert(
        "privilege",
        tonic::metadata::MetadataValue::from_static("user"),
    );
    req.metadata_mut().insert(
        "user_id",
        tonic::metadata::MetadataValue::try_from(user_id).unwrap(),
    );
    service._start_protocol_operator(req).await?;
    Ok(())
}
