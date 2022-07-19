use crate::storage::common::get_prefix;
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use tokio::sync::RwLock;
use tracing::debug;

struct StorageMap {
    data: HashMap<String, Vec<u8>>,
    key_name_timestamp: HashMap<String, i64>,
    key_list: HashMap<String, (HashSet<String>, HashSet<String>)>, // 0 for latest, 1 for history
}

pub struct BasicStorage {
    map: RwLock<StorageMap>,
}

impl BasicStorage {
    pub fn new() -> Self {
        Self {
            map: RwLock::new(StorageMap {
                data: HashMap::new(),
                key_name_timestamp: HashMap::new(),
                key_list: HashMap::new(),
            }),
        }
    }
}

impl Default for BasicStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl crate::storage::common::Storage for BasicStorage {
    async fn create(&self, user_id: &str, key_name: &str, value: &[u8]) -> Result<String, String> {
        let mut map = self.map.write().await;
        let timestamp = Utc::now().timestamp_nanos();
        let key_path_created = format!("{}::{}@{}", user_id, key_name, timestamp);
        let user_id_key_name = format!("{}::{}", user_id, key_name);
        debug!("{}", user_id_key_name);
        if map.key_name_timestamp.contains_key(&user_id_key_name)
            && map.data.contains_key(&format!(
                "{}@{}",
                user_id_key_name,
                map.key_name_timestamp.get(&user_id_key_name).unwrap()
            ))
        {
            return Err(format!("Key name already exists: {}", user_id_key_name));
        }
        map.data.insert(key_path_created.clone(), value.to_vec());
        map.key_name_timestamp.insert(user_id_key_name, timestamp);
        let prefix = get_prefix(key_name);
        let keys_directory = format!("{}::{}", user_id, prefix);
        let contains_key = map.key_list.contains_key(&keys_directory);
        if contains_key {
            let key_list_set = map.key_list.get_mut(&keys_directory).unwrap();
            key_list_set.0.insert(key_path_created.clone());
            key_list_set.1.insert(key_path_created.clone());
        } else {
            let mut key_list_set = (HashSet::new(), HashSet::new());
            key_list_set.0.insert(key_path_created.clone());
            key_list_set.1.insert(key_path_created.clone());
            map.key_list.insert(keys_directory, key_list_set);
        }

        Ok(key_path_created)
    }

    async fn read_from_key_paths(
        &self,
        key_paths: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, String> {
        let map = self.map.read().await;
        let mut result = HashMap::new();
        for key_path in key_paths {
            let value = map.data.get(key_path);
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
        let map = self.map.read().await;
        let mut result = HashMap::new();
        for key_name in key_names {
            let timestamp = map
                .key_name_timestamp
                .get(&format!("{}::{}", user_id, key_name));
            let timestamp = match timestamp {
                Some(v) => v,
                None => return Err(format!("Key name not found: {}", key_name)),
            };
            let key_path = format!("{}::{}@{}", user_id, key_name, timestamp);
            let value = map.data.get(&key_path);
            match value {
                Some(v) => result.insert(key_path, v.clone()),
                None => return Err(format!("Entry have been deleted: {}", key_path)),
            };
        }
        Ok(result)
    }

    async fn list_keys(&self, prefix: &str, include_history: bool) -> Result<Vec<String>, String> {
        let map = self.map.read().await;
        let res = map.key_list.get(&format!("{}:", prefix));
        Ok(match res {
            None => vec![],
            Some(key_list_set) => {
                if include_history {
                    Vec::from_iter(key_list_set.1.clone())
                } else {
                    Vec::from_iter(key_list_set.0.clone())
                }
            }
        })
    }

    async fn update(&self, user_id: &str, key_name: &str, value: &[u8]) -> Result<String, String> {
        let timestamp = Utc::now().timestamp_nanos();
        let mut map = self.map.write().await;
        let key_path_created = format!("{}::{}@{}", user_id, key_name, timestamp);
        let user_id_key_name = format!("{}::{}", user_id, key_name);
        map.data.insert(key_path_created.clone(), value.to_vec());
        let old_timestamp = if map.key_name_timestamp.contains_key(&user_id_key_name) {
            *map.key_name_timestamp.get(&user_id_key_name).unwrap()
        } else {
            0_i64
        };
        map.key_name_timestamp
            .insert(user_id_key_name.clone(), timestamp);
        let prefix = get_prefix(key_name);
        let keys_directory = format!("{}::{}", user_id, prefix);
        let contains_key = map.key_list.contains_key(&keys_directory);
        if contains_key {
            let key_list_set = map.key_list.get_mut(&keys_directory).unwrap();
            if old_timestamp != 0
                && key_list_set
                    .0
                    .contains(&format!("{}@{}", user_id_key_name, old_timestamp))
            {
                key_list_set
                    .0
                    .remove(&format!("{}@{}", user_id_key_name, old_timestamp));
            }

            key_list_set.0.insert(key_path_created.clone());
            key_list_set.1.insert(key_path_created.clone());
        } else {
            let mut key_list_set = (HashSet::new(), HashSet::new());
            key_list_set.0.insert(key_path_created.clone());
            key_list_set.1.insert(key_path_created.clone());
            map.key_list.insert(keys_directory, key_list_set);
        }

        Ok(key_path_created)
    }

    async fn delete(&self, user_id: &str, key_name: &str) -> Result<String, String> {
        let timestamp = Utc::now().timestamp();
        let mut map = self.map.write().await;
        let user_id_key_name = format!("{}::{}", user_id, key_name);
        if map.key_name_timestamp.contains_key(&user_id_key_name)
            && map.data.contains_key(&format!(
                "{}@{}",
                user_id_key_name,
                map.key_name_timestamp.get(&user_id_key_name).unwrap()
            ))
        {
            let old_timestamp = if map.key_name_timestamp.contains_key(&user_id_key_name) {
                *map.key_name_timestamp.get(&user_id_key_name).unwrap()
            } else {
                0_i64
            };
            map.key_name_timestamp
                .insert(user_id_key_name.clone(), timestamp);
            let prefix = get_prefix(key_name);
            let keys_directory = format!("{}::{}", user_id, prefix);
            let contains_key = map.key_list.contains_key(&keys_directory);
            if contains_key {
                let key_list_set = map.key_list.get_mut(&keys_directory).unwrap();
                if old_timestamp != 0
                    && key_list_set
                        .0
                        .contains(&format!("{}@{}", user_id_key_name, old_timestamp))
                {
                    key_list_set
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
