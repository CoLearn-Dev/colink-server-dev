use crate::storage::common::get_prefix;
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use tokio::sync::RwLock;
use tracing::debug;

#[derive(Default)]
struct StorageMap {
    key_path_to_value: HashMap<String, Vec<u8>>,
    key_name_to_timestamp: HashMap<String, i64>,
    path_to_key_paths: HashMap<String, (HashSet<String>, HashSet<String>)>, // 0 for latest, 1 for history
    path_to_directories: HashMap<String, HashSet<String>>,
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
            _update_dir(&mut maps, user_id, key_name);
        }
        maps.key_path_to_value
            .insert(key_path_created.clone(), value.to_vec());
        maps.key_name_to_timestamp
            .insert(user_id_key_name, timestamp);
        let prefix = get_prefix(key_name);
        let keys_directory = format!("{}::{}", user_id, prefix);
        let contains_key = maps.path_to_key_paths.contains_key(&keys_directory);
        if contains_key {
            let key_list_set = maps.path_to_key_paths.get_mut(&keys_directory).unwrap();
            key_list_set.0.insert(key_path_created.clone());
            key_list_set.1.insert(key_path_created.clone());
        } else {
            let mut key_list_set = (HashSet::new(), HashSet::new());
            key_list_set.0.insert(key_path_created.clone());
            key_list_set.1.insert(key_path_created.clone());
            maps.path_to_key_paths.insert(keys_directory, key_list_set);
        }
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
        let res = maps.path_to_key_paths.get(&format!("{}:", prefix));
        let dir = maps.path_to_directories.get(&format!("{}:", prefix));
        Ok(match res {
            None => match dir {
                None => vec![],
                Some(dir) => Vec::from_iter(dir.clone()),
            },
            Some(key_list_set) => {
                if include_history {
                    match dir {
                        None => Vec::from_iter(key_list_set.1.clone()),
                        Some(dir) => _merge_key_list_directories(&key_list_set.1, dir),
                    }
                } else {
                    match dir {
                        None => Vec::from_iter(key_list_set.0.clone()),
                        Some(dir) => _merge_key_list_directories(&key_list_set.0, dir),
                    }
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
            _update_dir(&mut maps, user_id, key_name);
        }
        let prefix = get_prefix(key_name);
        let keys_directory = format!("{}::{}", user_id, prefix);
        let contains_key = maps.path_to_key_paths.contains_key(&keys_directory);
        if contains_key {
            let key_list_set = maps.path_to_key_paths.get_mut(&keys_directory).unwrap();
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
            maps.path_to_key_paths.insert(keys_directory, key_list_set);
        }
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
            let prefix = get_prefix(key_name);
            let keys_directory = format!("{}::{}", user_id, prefix);
            let contains_key = maps.path_to_key_paths.contains_key(&keys_directory);
            if contains_key {
                let key_list_set = maps.path_to_key_paths.get_mut(&keys_directory).unwrap();
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

fn _update_dir(maps: &mut StorageMap, user_id: &str, key_name: &str) {
    let split: Vec<&str> = key_name.split(':').collect();
    let mut path1 = user_id.to_string() + "::";
    let mut path2 = user_id.to_string() + "::" + split[0];
    for i in 0..split.len() - 1 {
        let contains_key = maps.path_to_directories.contains_key(&path1);
        let value = path2.clone() + "@0";
        if contains_key {
            let directories = maps.path_to_directories.get_mut(&path1).unwrap();
            directories.insert(value);
        } else {
            let mut directories = HashSet::new();
            directories.insert(value);
            maps.path_to_directories.insert(path1, directories);
        }
        path1 = path2.clone() + ":";
        path2 = path2 + ":" + split[i + 1];
    }
}

fn _merge_key_list_directories(
    key_list: &HashSet<String>,
    directories: &HashSet<String>,
) -> Vec<String> {
    let mut ans = vec![];
    let key_list = key_list.clone();
    let mut directories = directories.clone();
    for key in key_list {
        let dir = key[..key.rfind('@').unwrap()].to_string() + "@0";
        directories.remove(&dir);
        ans.push(key);
    }
    ans.append(&mut Vec::from_iter(directories));
    ans
}
