use super::super::utils::{download_tgz, fetch_from_git};
use crate::colink_proto::*;
use std::{io::Write, path::Path};
use toml::Value;

pub(super) async fn fetch_protocol(
    protocol_name: &str,
    colink_home: &str,
    source_type: &StartProtocolOperatorSourceType,
    source: &str,
    protocol_inventory: &str,
    dev_mode: bool,
) -> Result<(), String> {
    if !source.is_empty() {
        if !dev_mode {
            return Err(
                "Please enable --pom-allow-external-source to allow loading protocol from arbitrary source.".into(),
            );
        }
        if matches!(source_type, StartProtocolOperatorSourceType::None) {
            return Err(format!(
                "Please specify the source_type of this source {source}.",
            ));
        }
        return fetch_protocol_from_souce(protocol_name, colink_home, source_type, source).await;
    }
    fetch_protocol_from_inventory(protocol_name, colink_home, source_type, protocol_inventory).await
}

async fn fetch_protocol_from_inventory(
    protocol_name: &str,
    colink_home: &str,
    source_type: &StartProtocolOperatorSourceType,
    protocol_inventory: &str,
) -> Result<(), String> {
    let url = &format!("{}/{}.toml", protocol_inventory, protocol_name);
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
    if matches!(
        source_type,
        StartProtocolOperatorSourceType::None | StartProtocolOperatorSourceType::Tgz
    ) && inventory_toml.get("binary").is_some()
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
    if matches!(
        source_type,
        StartProtocolOperatorSourceType::None | StartProtocolOperatorSourceType::Tgz
    ) && inventory_toml.get("source").is_some()
        && inventory_toml["source"].get("archive").is_some()
    {
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
    if matches!(
        source_type,
        StartProtocolOperatorSourceType::None | StartProtocolOperatorSourceType::Git
    ) && inventory_toml.get("source").is_some()
        && inventory_toml["source"].get("git").is_some()
    {
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
    if matches!(
        source_type,
        StartProtocolOperatorSourceType::None | StartProtocolOperatorSourceType::Docker
    ) && inventory_toml.get("docker").is_some()
        && inventory_toml["docker"].get("image").is_some()
    {
        if let Some(source) = inventory_toml["docker"]["image"].as_table() {
            if source.get("name").is_some()
                && source["name"].as_str().is_some()
                && source.get("digest").is_some()
                && source["digest"].as_str().is_some()
            {
                create_toml_for_docker(
                    &format!(
                        "{}@{}",
                        source["name"].as_str().unwrap(),
                        source["digest"].as_str().unwrap()
                    ),
                    protocol_package_dir.to_str().unwrap(),
                )
                .await?;
                return Ok(());
            }
        }
    }
    Err(format!(
        "the inventory file of protocol {} is damaged",
        protocol_name
    ))
}

async fn create_toml_for_docker(image: &str, protocol_package_dir: &str) -> Result<(), String> {
    match std::fs::create_dir_all(protocol_package_dir) {
        Ok(_) => {}
        Err(_) => return Err(format!("fail to create protocol_package_dir for {}", image)),
    }
    let mut file = match std::fs::File::create(Path::new(&protocol_package_dir).join("colink.toml"))
    {
        Ok(file) => file,
        Err(_) => return Err(format!("fail to create colink.toml file for {}", image)),
    };
    match file.write_all(format!("[package]\ndocker_image = \"{}\"\n", image).as_bytes()) {
        Ok(_) => {}
        Err(_) => return Err(format!("fail to write colink.toml file for {}", image)),
    }
    Ok(())
}

async fn fetch_protocol_from_souce(
    protocol_name: &str,
    colink_home: &str,
    source_type: &StartProtocolOperatorSourceType,
    source: &str,
) -> Result<(), String> {
    let protocol_package_dir = Path::new(&colink_home)
        .join("protocols")
        .join(protocol_name);
    match source_type {
        StartProtocolOperatorSourceType::Tgz => {
            download_tgz(source, "", protocol_package_dir.to_str().unwrap()).await?;
        }
        StartProtocolOperatorSourceType::Git => {
            fetch_from_git(source, "", protocol_package_dir.to_str().unwrap()).await?;
        }
        StartProtocolOperatorSourceType::Docker => {
            create_toml_for_docker(source, protocol_package_dir.to_str().unwrap()).await?;
        }
        _ => {}
    }
    Ok(())
}
