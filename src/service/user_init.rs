use super::utils::*;
use crate::{colink_proto::*, server::MyService};
use std::{path::Path, sync::Arc};
use toml::Value;

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
    let protocols = toml.as_table().unwrap().clone();
    let mut handles = vec![];
    for (protocol_name, init_param) in protocols {
        if init_param.get("operator_num").is_some()
            && init_param["operator_num"].as_integer().unwrap() > 0
        {
            let service = service.clone();
            let user_id = user_id.to_string();
            let user_jwt = user_jwt.to_string();
            handles.push(tokio::spawn(async move {
                init_protocol(&service, &user_id, &user_jwt, &protocol_name, &init_param).await
            }));
        }
    }
    for handle in handles {
        handle.await??;
    }
    service
        ._internal_storage_update(user_id, "_is_initialized", &[1])
        .await?;
    Ok(())
}

async fn init_protocol(
    service: &MyService,
    user_id: &str,
    user_jwt: &str,
    protocol_name: &str,
    init_param: &Value,
) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    if init_param.get("start_after").is_some() {
        let start_after_list = init_param["start_after"].as_array().unwrap();
        for p in start_after_list {
            wait_protocol_initialization(service, user_id, p.as_str().unwrap()).await?;
        }
    }
    if init_param.get("create_entry").is_some() {
        let entries = init_param["create_entry"].as_array().unwrap();
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
    let is_initialized_key = format!("_internal:protocols:{}:_is_initialized", protocol_name);
    service
        ._user_storage_update(user_id, &is_initialized_key, &[0])
        .await?;
    for _ in 0..init_param["operator_num"].as_integer().unwrap() {
        _start_protocol_operator(service, user_id, user_jwt, protocol_name).await?;
    }
    wait_protocol_initialization(service, user_id, protocol_name).await?;
    Ok(())
}

async fn wait_protocol_initialization(
    service: &MyService,
    user_id: &str,
    protocol_name: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let is_initialized_key = format!("_internal:protocols:{}:_is_initialized", protocol_name);
    loop {
        let is_initialized = service
            ._user_storage_read(user_id, &is_initialized_key)
            .await?[0];
        if is_initialized == 1 {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
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
