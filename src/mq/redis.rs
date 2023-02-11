use redis::{acl::Rule, aio::Connection, AsyncCommands};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

#[derive(Default)]
pub struct RedisStream {
    redis_uri: String,
    mq_prefix: String,
    routing_table: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl RedisStream {
    pub fn new(redis_uri: &str, mq_prefix: &str) -> Self {
        Self {
            redis_uri: redis_uri.to_string(),
            mq_prefix: mq_prefix.to_string(),
            ..Default::default()
        }
    }
}

#[async_trait::async_trait]
impl crate::mq::common::MQ for RedisStream {
    async fn create_user_account(&self) -> Result<String, String> {
        let ug = passwords::PasswordGenerator::new()
            .length(16)
            .numbers(true)
            .lowercase_letters(true);
        let pg = passwords::PasswordGenerator::new()
            .length(32)
            .numbers(true)
            .lowercase_letters(true)
            .uppercase_letters(true);
        let username = ug.generate_one().unwrap();
        let username = format!("{}--{}", self.mq_prefix, username);
        let password = pg.generate_one().unwrap();
        let admin_uri = match url::Url::parse(&self.redis_uri) {
            Ok(admin_uri) => admin_uri,
            Err(e) => return Err(format!("URI Parse Error: {}", e)),
        };
        let user_uri = match admin_uri.port() {
            Some(port) => format!(
                "{}://{}:{}@{}:{}/",
                admin_uri.scheme(),
                username,
                password,
                admin_uri.host_str().unwrap(),
                port
            ),
            None => format!(
                "{}://{}:{}@{}/",
                admin_uri.scheme(),
                username,
                password,
                admin_uri.host_str().unwrap(),
            ),
        };
        let mut con = self.connect().await?;
        match con
            .acl_setuser_rules::<&str, ()>(
                &username,
                &[
                    Rule::On,
                    Rule::AddPass(password.to_owned()),
                    Rule::Pattern(format!("{}:*", username).to_owned()),
                    Rule::AddCategory("stream".to_owned()),
                ],
            )
            .await
        {
            Ok(_) => {}
            Err(e) => return Err(format!("RedisStream ACL SETUSER Error: {}", e)),
        }
        Ok(user_uri)
    }

    async fn delete_user_account(&self, user_uri: &str) -> Result<(), String> {
        let user_uri = match url::Url::parse(user_uri) {
            Ok(uri) => uri,
            Err(e) => return Err(format!("URI Parse Error: {}", e)),
        };
        let user_name = user_uri.username();
        let mut con = self.connect().await?;
        let key_list: Vec<String> = match con.keys(format!("{}:*", user_name)).await {
            Ok(key_list) => key_list,
            Err(e) => return Err(format!("RedisStream KEYS Error: {}", e)),
        };
        if !key_list.is_empty() {
            match con.del::<&[String], ()>(&key_list).await {
                Ok(_) => {}
                Err(e) => return Err(format!("RedisStream DEL Error: {}", e)),
            };
        }
        match con.acl_deluser::<&str, ()>(&[user_name]).await {
            Ok(_) => {}
            Err(e) => return Err(format!("RedisStream ACL DELUSER Error: {}", e)),
        };
        Ok(())
    }

    async fn delete_all_accounts(&self) -> Result<(), String> {
        let mut con = self.connect().await?;
        let key_list: Vec<String> = match con.keys(format!("{}--*", self.mq_prefix)).await {
            Ok(key_list) => key_list,
            Err(e) => return Err(format!("RedisStream KEYS Error: {}", e)),
        };
        if !key_list.is_empty() {
            match con.del::<&[String], ()>(&key_list).await {
                Ok(_) => {}
                Err(e) => return Err(format!("RedisStream DEL Error: {}", e)),
            };
        }
        let mut users: Vec<String> = match con.acl_users().await {
            Ok(users) => users,
            Err(e) => return Err(format!("RedisStream ACL USERS Error: {}", e)),
        };
        let my_user_name: String = match con.acl_whoami().await {
            Ok(user_name) => user_name,
            Err(e) => return Err(format!("RedisStream ACL WHOAMI Error: {}", e)),
        };
        users.retain(|x| x != &my_user_name && x.starts_with(&format!("{}--", self.mq_prefix)));
        if !users.is_empty() {
            match con.acl_deluser::<String, ()>(&users).await {
                Ok(_) => {}
                Err(e) => return Err(format!("RedisStream ACL DELUSER Error: {}", e)),
            };
        }
        Ok(())
    }

    async fn create_queue(&self, user_uri: &str, queue_name: &str) -> Result<String, String> {
        let mut con = self.connect().await?;
        let queue_name = if queue_name.is_empty() {
            passwords::PasswordGenerator::new()
                .length(24)
                .numbers(true)
                .lowercase_letters(true)
                .generate_one()
                .unwrap()
        } else {
            queue_name.to_string()
        };
        let user_uri = match url::Url::parse(user_uri) {
            Ok(uri) => uri,
            Err(e) => return Err(format!("URI Parse Error: {}", e)),
        };
        let user_name = user_uri.username();
        let queue_name = format!("{}:{}", user_name, queue_name);
        match con
            .xgroup_create_mkstream::<&str, &str, &str, ()>(&queue_name, &queue_name, "$")
            .await
        {
            Ok(_) => {}
            Err(e) => return Err(format!("RedisStream Group Creation Error: {}", e)),
        };
        Ok(queue_name)
    }

    async fn delete_queue(&self, _user_uri: &str, queue_name: &str) -> Result<(), String> {
        let mut con = self.connect().await?;
        match con
            .xgroup_destroy::<&str, &str, ()>(queue_name, queue_name)
            .await
        {
            Ok(_) => {}
            Err(e) => return Err(format!("RedisStream Group Deletion Error: {}", e)),
        };
        match con.del::<&str, ()>(queue_name).await {
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
        _user_uri: &str,
        key: &str,
        payload: &[u8],
    ) -> Result<(), String> {
        let routing_table = self.routing_table.read().await;
        if !routing_table.contains_key(key) {
            return Ok(());
        }
        let mut con = self.connect().await?;
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
    async fn connect(&self) -> Result<Connection, String> {
        let client = match redis::Client::open(&*self.redis_uri) {
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
