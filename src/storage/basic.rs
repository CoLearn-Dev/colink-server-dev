use crate::storage::common::get_prefix;
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use tokio::sync::RwLock;
use tracing::debug;

#[derive(Default)]
struct StorageMap {
    key_path_to_value: HashMap<String, Vec<u8>>,
    key_name_to_timestamp: HashMap<String, i64>,
    prefix_to_key_paths: HashMap<String, (HashSet<String>, HashSet<String>)>, // 0 for latest, 1 for history
}

#[derive(Default)]
pub struct BasicStorage {
    maps: RwLock<StorageMap>,
}

#[async_trait::async_trait]
impl crate::storage::common::Storage for BasicStorage {
    async fn create(&self, user_id: &str, key_name: &str, value: &[u8]) -> Result<String, String> {
        let mut maps = self.maps.write().await;
        let timestamp = Utc::now().timestamp_nanos();
        let key_path_created = format!("{}::{}@{}", user_id, key_name, timestamp);
        let user_id_key_name = format!("{}::{}", user_id, key_name);
        debug!("{}", user_id_key_name);
        if maps.key_name_to_timestamp.contains_key(&user_id_key_name) {
            if maps.key_path_to_value.contains_key(&format!(
                "{}@{}",
                user_id_key_name,
                maps.key_name_to_timestamp.get(&user_id_key_name).unwrap()
            )) {
                return Err(format!("Key name already exists: {}", user_id_key_name));
            }
        } else {
            _maintain_prefixes_along_path(&mut maps, user_id, key_name);
        }
        maps.key_path_to_value
            .insert(key_path_created.clone(), value.to_vec());
        maps.key_name_to_timestamp
            .insert(user_id_key_name, timestamp);
        let key_name_prefix = get_prefix(key_name);
        let prefix = format!("{}::{}", user_id, key_name_prefix);
        let prefix_set = maps
            .prefix_to_key_paths
            .entry(prefix)
            .or_insert((HashSet::new(), HashSet::new()));
        prefix_set.0.insert(key_path_created.clone());
        prefix_set.1.insert(key_path_created.clone());
        Ok(key_path_created)
    }

    async fn read_from_key_paths(
        &self,
        key_paths: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, String> {
        let maps = self.maps.read().await;
        let mut result = HashMap::new();
        for key_path in key_paths {
            let value = maps.key_path_to_value.get(key_path);
            match value {
                Some(v) => result.insert(key_path.clone(), v.clone()),
                None => return Err(format!("Key path not found: {}", key_path)),
            };
        }
        Ok(result)
    }

    async fn read_from_key_names(
        &self,
        user_id: &str,
        key_names: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, String> {
        let maps = self.maps.read().await;
        let mut result = HashMap::new();
        for key_name in key_names {
            let timestamp = maps
                .key_name_to_timestamp
                .get(&format!("{}::{}", user_id, key_name));
            let timestamp = match timestamp {
                Some(v) => v,
                None => return Err(format!("Key name not found: {}", key_name)),
            };
            let key_path = format!("{}::{}@{}", user_id, key_name, timestamp);
            let value = maps.key_path_to_value.get(&key_path);
            match value {
                Some(v) => result.insert(key_path, v.clone()),
                None => return Err(format!("Entry have been deleted: {}", key_path)),
            };
        }
        Ok(result)
    }

    async fn list_keys(&self, prefix: &str, include_history: bool) -> Result<Vec<String>, String> {
        let maps = self.maps.read().await;
        let res = maps.prefix_to_key_paths.get(&format!("{}:", prefix));
        Ok(match res {
            None => vec![],
            Some(prefix_set) => {
                if include_history {
                    Vec::from_iter(prefix_set.1.clone())
                } else {
                    Vec::from_iter(prefix_set.0.clone())
                }
            }
        })
    }

    async fn update(&self, user_id: &str, key_name: &str, value: &[u8]) -> Result<String, String> {
        let mut maps = self.maps.write().await;
        let timestamp = Utc::now().timestamp_nanos();
        let key_path_created = format!("{}::{}@{}", user_id, key_name, timestamp);
        let user_id_key_name = format!("{}::{}", user_id, key_name);
        maps.key_path_to_value
            .insert(key_path_created.clone(), value.to_vec());
        let old_timestamp = maps
            .key_name_to_timestamp
            .insert(user_id_key_name.clone(), timestamp)
            .unwrap_or(0_i64);
        if old_timestamp == 0 {
            _maintain_prefixes_along_path(&mut maps, user_id, key_name);
        }
        let key_name_prefix = get_prefix(key_name);
        let prefix = format!("{}::{}", user_id, key_name_prefix);
        let prefix_set = maps
            .prefix_to_key_paths
            .entry(prefix)
            .or_insert((HashSet::new(), HashSet::new()));
        if old_timestamp != 0
            && prefix_set
                .0
                .contains(&format!("{}@{}", user_id_key_name, old_timestamp))
        {
            prefix_set
                .0
                .remove(&format!("{}@{}", user_id_key_name, old_timestamp));
        }
        prefix_set.0.insert(key_path_created.clone());
        prefix_set.1.insert(key_path_created.clone());
        Ok(key_path_created)
    }

    async fn delete(&self, user_id: &str, key_name: &str) -> Result<String, String> {
        let mut maps = self.maps.write().await;
        let timestamp = Utc::now().timestamp();
        let user_id_key_name = format!("{}::{}", user_id, key_name);
        if maps.key_name_to_timestamp.contains_key(&user_id_key_name)
            && maps.key_path_to_value.contains_key(&format!(
                "{}@{}",
                user_id_key_name,
                maps.key_name_to_timestamp.get(&user_id_key_name).unwrap()
            ))
        {
            let old_timestamp = maps
                .key_name_to_timestamp
                .insert(user_id_key_name.clone(), timestamp)
                .unwrap_or(0_i64);
            let key_name_prefix = get_prefix(key_name);
            let prefix = format!("{}::{}", user_id, key_name_prefix);
            if maps.prefix_to_key_paths.contains_key(&prefix) {
                let prefix_set = maps.prefix_to_key_paths.get_mut(&prefix).unwrap();
                if old_timestamp != 0
                    && prefix_set
                        .0
                        .contains(&format!("{}@{}", user_id_key_name, old_timestamp))
                {
                    prefix_set
                        .0
                        .remove(&format!("{}@{}", user_id_key_name, old_timestamp));
                }
            }
            Ok(format!("{}::{}@{}", user_id, key_name, timestamp))
        } else {
            Err(format!("Key name not found: {}", key_name))
        }
    }
}

fn _maintain_prefixes_along_path(maps: &mut StorageMap, user_id: &str, key_name: &str) {
    let splits: Vec<&str> = key_name.split(':').collect();
    let mut current_path = user_id.to_string() + "::";
    let mut next_path = user_id.to_string() + "::" + splits[0];
    for i in 0..splits.len() - 1 {
        let current_set = maps
            .prefix_to_key_paths
            .entry(current_path)
            .or_insert((HashSet::new(), HashSet::new()));
        current_set.0.insert(next_path.clone() + "@0");
        current_set.1.insert(next_path.clone() + "@0");
        current_path = next_path.clone() + ":";
        next_path = next_path + ":" + splits[i + 1];
    }
}
