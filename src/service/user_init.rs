use super::utils::*;
use crate::{colink_proto::*, server::MyService};
use colink_policy_module_proto::*;
use prost::Message;
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
    if toml.get("policy_module").is_some() && toml["policy_module"]["enable"].as_bool().unwrap() {
        _start_protocol_operator(&service, user_id, user_jwt, "policy_module").await?;
        let mut settings = Settings {
            enable: true,
            ..Default::default()
        };
        if toml["policy_module"]["accept_all_tasks"].as_bool().unwrap() {
            let rule_id = uuid::Uuid::new_v4().to_string();
            let rule = Rule {
                rule_id,
                task_filter: Some(TaskFilter::default()),
                action: "approve".to_string(),
                priority: 1,
            };
            settings.rules.push(rule);
        }
        let mut payload = vec![];
        settings.encode(&mut payload).unwrap();
        service
            ._user_storage_update(user_id, "_policy_module:settings", &payload)
            .await?;
        let participants = vec![Participant {
            user_id: user_id.to_string(),
            role: "local".to_string(),
        }];
        _run_local_task(
            &service,
            user_id,
            "policy_module",
            Default::default(),
            &participants,
        )
        .await?;
        loop {
            if service
                ._user_storage_read(user_id, "_policy_module:applied_settings_timestamp")
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }
    if toml.get("remote_storage").is_some() && toml["remote_storage"]["enable"].as_bool().unwrap() {
        _start_protocol_operator(&service, user_id, user_jwt, "remote_storage").await?;
    }
    if toml.get("remote_command").is_some() && toml["remote_command"]["enable"].as_bool().unwrap() {
        _start_protocol_operator(&service, user_id, user_jwt, "remote_command").await?;
    }
    if toml.get("registry").is_some() && toml["registry"]["enable"].as_bool().unwrap() {
        _start_protocol_operator(&service, user_id, user_jwt, "registry").await?;
    }
    if toml.get("telegram_bot").is_some() && toml["telegram_bot"]["enable"].as_bool().unwrap() {
        _start_protocol_operator(&service, user_id, user_jwt, "telegram_bot").await?;
    }
    service
        ._internal_storage_update(user_id, "is_initialized", &[1])
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

async fn _run_local_task(
    service: &MyService,
    user_id: &str,
    protocol_name: &str,
    protocol_param: &[u8],
    participants: &[Participant],
) -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let mut local_task = Task {
        task_id: uuid::Uuid::new_v4().to_string(),
        protocol_name: protocol_name.to_string(),
        protocol_param: protocol_param.to_vec(),
        participants: participants.to_vec(),
        require_agreement: false,
        status: "started".to_string(),
        expiration_time: chrono::Utc::now().timestamp() + 86400,
        ..Default::default()
    };
    local_task
        .decisions
        .resize(local_task.participants.len(), Default::default());
    local_task.decisions[0] = service
        .generate_decision(true, false, "", user_id, &local_task)
        .await?;
    let mut payload = vec![];
    local_task.encode(&mut payload).unwrap();
    service
        ._internal_storage_update(user_id, &format!("tasks:{}", local_task.task_id), &payload)
        .await?;
    let task_storage_mutex = service.task_storage_mutex.lock().await;
    service.add_task_new_status(user_id, &local_task).await?;
    drop(task_storage_mutex);
    Ok(())
}
