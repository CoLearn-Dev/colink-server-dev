use crate::colink_proto::SubscriptionMessage;
use crate::mq::common::MQ;
use crate::storage::common::Storage;
use prost::Message;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use tokio::sync::{Mutex, RwLock};

pub struct StorageWithMQSubscription {
    storage: Box<dyn Storage>,
    mq: Box<dyn MQ>,
    key_subscription_count: Mutex<HashMap<String, i32>>,
    queue_name_to_user_id_key_name: Mutex<HashMap<String, String>>,
    lock: RwLock<i32>,
}

#[async_trait::async_trait]
impl crate::storage::common::Storage for StorageWithMQSubscription {
    async fn create(&self, user_id: &str, key_name: &str, value: &[u8]) -> Result<String, String> {
        let _lock = self.lock.read().await;
        let key_path = self.storage.create(user_id, key_name, value).await?;
        self.publish_change(user_id, key_name, "create", &key_path, value)
            .await?;
        Ok(key_path)
    }

    async fn read_from_key_paths(
        &self,
        key_paths: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, String> {
        self.storage.read_from_key_paths(key_paths).await
    }

    async fn read_from_key_names(
        &self,
        user_id: &str,
        key_names: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, String> {
        self.storage.read_from_key_names(user_id, key_names).await
    }

    async fn list_keys(&self, prefix: &str, include_history: bool) -> Result<Vec<String>, String> {
        self.storage.list_keys(prefix, include_history).await
    }

    async fn update(&self, user_id: &str, key_name: &str, value: &[u8]) -> Result<String, String> {
        let _lock = self.lock.read().await;
        let key_path = self.storage.update(user_id, key_name, value).await?;
        self.publish_change(user_id, key_name, "update", &key_path, value)
            .await?;
        Ok(key_path)
    }

    async fn delete(&self, user_id: &str, key_name: &str) -> Result<String, String> {
        let _lock = self.lock.read().await;
        let key_path = self.storage.delete(user_id, key_name).await?;
        self.publish_change(user_id, key_name, "delete", &key_path, Default::default())
            .await?;
        Ok(key_path)
    }
}

#[async_trait::async_trait]
impl crate::subscription::common::StorageWithSubscription for StorageWithMQSubscription {
    async fn subscribe(
        &self,
        user_id: &str,
        key_name: &str,
        start_timestamp: i64,
    ) -> Result<String, String> {
        let user_id_key_name = format!("{}::{}", user_id, key_name);
        let mq_uri = self.get_mq_uri(user_id).await?;
        let queue_name = self.mq.create_queue(&mq_uri, "").await?;
        self.mq
            .queue_bind(&mq_uri, &queue_name, &queue_name)
            .await?;
        let lock = self.lock.write().await;
        let mut key_subscription_count = self.key_subscription_count.lock().await;
        let num = if key_subscription_count.contains_key(&user_id_key_name) {
            key_subscription_count.get(&user_id_key_name).unwrap() + 1
        } else {
            1
        };
        key_subscription_count.insert(user_id_key_name.clone(), num);
        drop(key_subscription_count);
        self.queue_name_to_user_id_key_name
            .lock()
            .await
            .insert(queue_name.clone(), user_id_key_name.clone());
        let routing_key = if user_id_key_name.len() > 200 {
            let mut hasher = Sha256::new();
            hasher.update(user_id_key_name.as_bytes());
            let sha256 = hasher.finalize();
            format!("sha256:{}", hex::encode(sha256))
        } else {
            user_id_key_name.clone()
        };
        self.mq
            .queue_bind(&mq_uri, &queue_name, &routing_key)
            .await?;
        let key_list = self
            .storage
            .list_keys(&get_prefix(user_id, key_name), true)
            .await?;
        let mut valid_key_paths = vec![];
        for key_path in key_list {
            if get_user_id_key_name(&key_path) == user_id_key_name
                && get_timestamp(&key_path) >= start_timestamp
            {
                valid_key_paths.push(key_path);
            }
        }
        let res = self.storage.read_from_key_paths(&valid_key_paths).await?;
        for (key_path, payload) in res {
            let message = SubscriptionMessage {
                change_type: "in-storage".to_string(),
                key_path,
                payload,
            };
            let mut payload = vec![];
            message.encode(&mut payload).unwrap();
            self.mq
                .publish_message(&mq_uri, &queue_name, &payload)
                .await?;
        }
        drop(lock);
        Ok(queue_name)
    }

    async fn unsubscribe(&self, user_id: &str, queue_name: &str) -> Result<(), String> {
        let mut queue_name_to_user_id_key_name = self.queue_name_to_user_id_key_name.lock().await;
        let user_id_key_name = match queue_name_to_user_id_key_name.get(queue_name) {
            Some(user_id_key_name) => user_id_key_name.clone(),
            None => return Err("Ubsubscribe Error: subscription not found.".to_string()),
        };
        queue_name_to_user_id_key_name.remove(queue_name);
        drop(queue_name_to_user_id_key_name);
        if user_id != get_user_id(&user_id_key_name) {
            return Err("Ubsubscribe Error: permission denied.".to_string());
        }
        let mut key_subscription_count = self.key_subscription_count.lock().await;
        if key_subscription_count.contains_key(&user_id_key_name) {
            let num = *key_subscription_count.get(&user_id_key_name).unwrap();
            if num == 1 {
                key_subscription_count.remove(&user_id_key_name);
            } else {
                key_subscription_count.insert(user_id_key_name, num - 1);
            }
        } else {
            return Err("Ubsubscribe Error: key_subscription_count not found.".to_string());
        }
        drop(key_subscription_count);
        let mq_uri = self.get_mq_uri(user_id).await?;
        self.mq.delete_queue(&mq_uri, queue_name).await?;
        Ok(())
    }
}

impl StorageWithMQSubscription {
    pub fn new(storage: Box<dyn Storage>, mq: Box<dyn MQ>) -> StorageWithMQSubscription {
        Self {
            storage,
            mq,
            key_subscription_count: Mutex::new(HashMap::new()),
            queue_name_to_user_id_key_name: Mutex::new(HashMap::new()),
            lock: RwLock::new(0),
        }
    }

    async fn get_mq_uri(&self, user_id: &str) -> Result<String, String> {
        let entries = self
            .storage
            .read_from_key_names(user_id, &["_internal:mq_uri".to_string()])
            .await?;
        let payload = entries.values().next().unwrap();
        Ok(String::from_utf8(payload.to_vec()).unwrap())
    }

    async fn publish_change(
        &self,
        user_id: &str,
        key_name: &str,
        change_type: &str,
        key_path: &str,
        payload: &[u8],
    ) -> Result<(), String> {
        let user_id_key_name = format!("{}::{}", user_id, key_name);
        if self
            .key_subscription_count
            .lock()
            .await
            .contains_key(&user_id_key_name)
        {
            let message = SubscriptionMessage {
                change_type: change_type.to_string(),
                key_path: key_path.to_string(),
                payload: payload.to_vec(),
            };
            let mut payload = vec![];
            message.encode(&mut payload).unwrap();
            let mq_uri = self.get_mq_uri(user_id).await?;
            let routing_key = if user_id_key_name.len() > 200 {
                let mut hasher = Sha256::new();
                hasher.update(user_id_key_name.as_bytes());
                let sha256 = hasher.finalize();
                format!("sha256:{}", hex::encode(sha256))
            } else {
                user_id_key_name.clone()
            };
            self.mq
                .publish_message(&mq_uri, &routing_key, &payload)
                .await?;
        }
        Ok(())
    }
}

fn get_prefix(user_id: &str, key_name: &str) -> String {
    match key_name.rfind(':') {
        Some(pos) => format!("{}::{}", user_id, &key_name[..pos]),
        None => format!("{}:", user_id),
    }
}

fn get_user_id_key_name(key_path: &str) -> &str {
    let pos = key_path.rfind('@').unwrap();
    &key_path[..pos]
}

fn get_timestamp(key_path: &str) -> i64 {
    let pos = key_path.rfind('@').unwrap();
    key_path[pos + 1..].parse().unwrap()
}

fn get_user_id(user_id_key_name: &str) -> &str {
    match user_id_key_name.find(':') {
        Some(pos) => &user_id_key_name[..pos],
        None => "",
    }
}
