use redis::{
    aio::Connection,
    streams::{StreamReadOptions, StreamReadReply},
    AsyncCommands, FromRedisValue,
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tracing::debug;

#[derive(Default)]
pub struct RedisStream {
    redis_uri: String,
    routing_table: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl RedisStream {
    pub fn new(redis_uri: &str) -> Self {
        Self {
            redis_uri: redis_uri.to_string(),
            ..Default::default()
        }
    }
}

#[async_trait::async_trait]
impl crate::mq::common::MQ for RedisStream {
    async fn create_user_account(&self) -> Result<String, String> {
        Ok(self.redis_uri.clone())
    }

    async fn delete_user_account(&self, user_uri: &str) -> Result<(), String> {
        Ok(())
    }

    async fn delete_all_accounts(&self) -> Result<(), String> {
        Ok(())
    }

    async fn create_queue(&self, user_uri: &str, queue_name: &str) -> Result<String, String> {
        let mut con = self.connect(user_uri).await?;
        let queue_name = if queue_name.is_empty() {
            passwords::PasswordGenerator::new()
                .length(16)
                .numbers(true)
                .lowercase_letters(true)
                .generate_one()
                .unwrap()
        } else {
            queue_name.to_string()
        };
        match con
            .xgroup_create_mkstream::<&str, &str, &str, ()>(&queue_name, &queue_name, "$")
            .await
        {
            Ok(_) => {}
            Err(e) => return Err(format!("RedisStream Group Creation Error: {}", e)),
        };
        Ok(queue_name)
    }

    async fn delete_queue(&self, user_uri: &str, queue_name: &str) -> Result<(), String> {
        let mut con = self.connect(user_uri).await?;
        match con
            .xgroup_destroy::<&str, &str, ()>(&queue_name, &queue_name)
            .await
        {
            Ok(_) => {}
            Err(e) => return Err(format!("RedisStream Group Deletion Error: {}", e)),
        };
        match con.del::<&str, ()>(&queue_name).await {
            Ok(_) => {}
            Err(e) => return Err(format!("RedisStream Stream Deletion Error: {}", e)),
        };
        Ok(())
    }

    async fn queue_bind(&self, _user_uri: &str, queue_name: &str, key: &str) -> Result<(), String> {
        let mut routing_table = self.routing_table.write().await;
        if !routing_table.contains_key(key) {
            routing_table.insert(key.to_string(), vec![]);
        }
        routing_table
            .get_mut(key)
            .unwrap()
            .push(queue_name.to_string());
        Ok(())
    }

    async fn publish_message(
        &self,
        user_uri: &str,
        key: &str,
        payload: &[u8],
    ) -> Result<(), String> {
        let routing_table = self.routing_table.read().await;
        if !routing_table.contains_key(key) {
            return Ok(());
        }
        let mut con = self.connect(user_uri).await?;
        for queue_name in routing_table.get(key).unwrap() {
            match con
                .xadd::<&str, &str, &str, &[u8], ()>(queue_name, "*", &[("payload", payload)])
                .await
            {
                Ok(_) => {}
                Err(e) => return Err(format!("RedisStream Publish Error: {}", e)),
            }
        }
        Ok(())
    }
}

impl RedisStream {
    async fn connect(&self, user_uri: &str) -> Result<Connection, String> {
        let client = match redis::Client::open("redis://127.0.0.1/") {
            Ok(client) => client,
            Err(e) => return Err(format!("Redis Connection Error: {}", e)),
        };
        let con = match client.get_async_connection().await {
            Ok(con) => con,
            Err(e) => return Err(format!("Redis Connection Error: {}", e)),
        };
        Ok(con)
    }
}
