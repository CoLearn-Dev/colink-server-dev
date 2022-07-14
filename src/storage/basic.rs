use crate::storage::common::{ends_with_reserved_tokens, get_prefix};
use chrono::Utc;
use std::collections::HashMap;
use tokio::sync::Mutex;
use tracing::debug;

pub struct BasicStorage {
    map: Mutex<HashMap<String, Vec<u8>>>,
}

impl BasicStorage {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
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
        ends_with_reserved_tokens(key_name)?;
        let timestamp = Utc::now().timestamp_nanos();
        let mut map = self.map.lock().await;
        let key_path_created = format!("{}::{}@{}", user_id, key_name, timestamp);
        let user_id_key_name = format!("{}::{}", user_id, key_name);
        debug!("{}", user_id_key_name);
        if map.contains_key(&user_id_key_name)
            && map.contains_key(&format!(
                "{}@{}",
                user_id_key_name,
                String::from_utf8(map.get(&user_id_key_name).unwrap().to_vec()).unwrap()
            ))
        {
            return Err(format!("Key name already exists: {}", user_id_key_name));
        }
        map.insert(key_path_created.clone(), value.to_vec());
        map.insert(user_id_key_name, timestamp.to_string().into_bytes());
        let prefix = get_prefix(key_name);
        let keys_directory = format!("{}::{}__keys", user_id, prefix);
        let contains_key = map.contains_key(&keys_directory);
        if contains_key {
            let v = map.get(&keys_directory).unwrap();
            let mut v: Vec<String> = serde_json::from_slice(v).unwrap();
            v.push(key_path_created.clone());
            map.insert(keys_directory, serde_json::to_vec(&v).unwrap());
        } else {
            map.insert(
                keys_directory,
                serde_json::to_vec(&vec![key_path_created.clone()]).unwrap(),
            );
        }

        Ok(key_path_created)
    }

    async fn read_from_key_paths(
        &self,
        key_paths: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, String> {
        let map = self.map.lock().await;
        let mut result = HashMap::new();
        for key_path in key_paths {
            ends_with_reserved_tokens(key_path)?;
            let value = map.get(key_path);
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
        let map = self.map.lock().await;
        let mut result = HashMap::new();
        for key_name in key_names {
            ends_with_reserved_tokens(key_name)?;
            let timestamp = map.get(&format!("{}::{}", user_id, key_name));
            let timestamp = match timestamp {
                Some(v) => v,
                None => return Err(format!("Key name not found: {}", key_name)),
            };
            let timestamp = String::from_utf8(timestamp.to_vec()).unwrap();
            let key_path = format!("{}::{}@{}", user_id, key_name, timestamp);
            let value = map.get(&key_path);
            match value {
                Some(v) => result.insert(key_path, v.clone()),
                None => return Err(format!("Entry have been deleted: {}", key_path)),
            };
        }
        Ok(result)
    }

    async fn list_keys(&self, prefix: &str, include_history: bool) -> Result<Vec<String>, String> {
        let map = self.map.lock().await;
        let res = map.get(&format!("{}:__keys", prefix));
        Ok(match res {
            None => vec![],
            Some(s) => {
                let keys: Vec<String> = serde_json::from_slice(s).unwrap();
                if keys.is_empty() || include_history {
                    keys
                } else {
                    vec![keys
                        .iter()
                        .max_by_key(|k| {
                            let key = k.as_str();
                            let mut parts = key.rsplitn(2, '@');
                            let timestamp = parts.next().unwrap();
                            timestamp.parse::<i64>().unwrap()
                        })
                        .cloned()
                        .unwrap()]
                }
            }
        })
    }

    async fn update(&self, user_id: &str, key_name: &str, value: &[u8]) -> Result<String, String> {
        ends_with_reserved_tokens(key_name)?;
        let timestamp = Utc::now().timestamp_nanos();
        let mut map = self.map.lock().await;
        let key_path_created = format!("{}::{}@{}", user_id, key_name, timestamp);
        let user_id_key_name = format!("{}::{}", user_id, key_name);
        map.insert(key_path_created.clone(), value.to_vec());
        map.insert(user_id_key_name, timestamp.to_string().into_bytes());
        let prefix = get_prefix(key_name);
        let keys_directory = format!("{}::{}__keys", user_id, prefix);
        let contains_key = map.contains_key(&keys_directory);
        if contains_key {
            let v = map.get(&keys_directory).unwrap();
            let mut v: Vec<String> = serde_json::from_slice(v).unwrap();
            v.push(key_path_created.clone());
            map.insert(keys_directory, serde_json::to_vec(&v).unwrap());
        } else {
            map.insert(
                keys_directory,
                serde_json::to_vec(&vec![key_path_created.clone()]).unwrap(),
            );
        }

        Ok(key_path_created)
    }

    async fn delete(&self, user_id: &str, key_name: &str) -> Result<String, String> {
        ends_with_reserved_tokens(key_name)?;
        let timestamp = Utc::now().timestamp();
        let mut map = self.map.lock().await;
        let user_id_key_name = format!("{}::{}", user_id, key_name);
        if map.contains_key(&user_id_key_name)
            && map.contains_key(&format!(
                "{}@{}",
                user_id_key_name,
                String::from_utf8(map.get(&user_id_key_name).unwrap().to_vec()).unwrap()
            ))
        {
            map.insert(user_id_key_name, timestamp.to_string().into_bytes());
            Ok(format!("{}::{}@{}", user_id, key_name, timestamp))
        } else {
            Err(format!("Key name not found: {}", key_name))
        }
    }
}
