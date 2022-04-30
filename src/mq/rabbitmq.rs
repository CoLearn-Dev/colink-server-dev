use lapin::{
    options::{
        BasicPublishOptions, ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions,
        QueueDeleteOptions,
    },
    types::FieldTable,
    BasicProperties, Channel, Connection, ConnectionProperties, ExchangeKind,
};
use tracing::debug;
pub struct RabbitMQ {
    mq_amqp: String,
    mq_api: String,
    mq_prefix: String,
}

impl RabbitMQ {
    pub fn new(mq_amqp: &str, mq_api: &str, mq_prefix: &str) -> Self {
        Self {
            mq_amqp: mq_amqp.to_string(),
            mq_api: mq_api.to_string(),
            mq_prefix: mq_prefix.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl crate::mq::common::MQ for RabbitMQ {
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
        let password = pg.generate_one().unwrap();
        let username = format!("{}--{}", self.mq_prefix, username);
        let vhost = username.clone();
        let admin_uri = match url::Url::parse(&self.mq_amqp) {
            Ok(admin_uri) => admin_uri,
            Err(e) => return Err(format!("MQ URI Parse Error: {}", e)),
        };
        let user_uri = match admin_uri.port() {
            Some(port) => format!(
                "{}://{}:{}@{}:{}/{}",
                admin_uri.scheme(),
                username,
                password,
                admin_uri.host_str().unwrap(),
                port,
                vhost
            ),
            None => format!(
                "{}://{}:{}@{}/{}",
                admin_uri.scheme(),
                username,
                password,
                admin_uri.host_str().unwrap(),
                vhost
            ),
        };
        debug!("MQ user_uri: {}", user_uri);
        let http_client = reqwest::Client::new();
        let resp = http_client
            .put(self.mq_api.clone() + "/users/" + &username)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(format!(r#"{{"password":"{}","tags":""}}"#, password))
            .send()
            .await;
        check_mq_api_resp(resp)?;
        let resp = http_client
            .put(self.mq_api.clone() + "/vhosts/" + &vhost)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .send()
            .await;
        check_mq_api_resp(resp)?;
        let resp = http_client
            .put(self.mq_api.clone() + "/permissions/" + &vhost + "/" + &username)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(r#"{"configure":".*","write":".*","read":".*"}"#)
            .send()
            .await;
        check_mq_api_resp(resp)?;
        let channel = self.connect(&user_uri).await?;
        match channel
            .exchange_declare(
                "CoLink",
                ExchangeKind::Direct,
                ExchangeDeclareOptions::default(),
                FieldTable::default(),
            )
            .await
        {
            Ok(x) => x,
            Err(e) => return Err(format!("MQ Exchange Creation Error: {}", e)),
        };
        Ok(user_uri)
    }

    async fn delete_user_account(&self, user_uri: &str) -> Result<(), String> {
        let user_uri = match url::Url::parse(user_uri) {
            Ok(uri) => uri,
            Err(e) => return Err(format!("MQ URI Parse Error: {}", e)),
        };
        let username = user_uri.username().to_string();
        let vhost = username.clone();
        let http_client = reqwest::Client::new();
        let resp = http_client
            .delete(self.mq_api.clone() + "/users/" + &username)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .send()
            .await;
        check_mq_api_resp(resp)?;
        let resp = http_client
            .delete(self.mq_api.clone() + "/vhosts/" + &vhost)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .send()
            .await;
        check_mq_api_resp(resp)?;
        Ok(())
    }

    async fn delete_all_accounts(&self) -> Result<(), String> {
        let http_client = reqwest::Client::new();
        let resp = http_client
            .get(self.mq_api.clone() + "/users/")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .send()
            .await;
        let resp = check_mq_api_resp(resp)?;
        let user_list = match resp.json::<serde_json::Value>().await {
            Ok(x) => x,
            Err(e) => return Err(format!("MQ API Error: {}", e)),
        };
        let user_list = match user_list.as_array() {
            Some(user_list) => user_list,
            None => return Err("MQ API Error: expected a list".to_string()),
        };
        for user in user_list {
            match user["name"].as_str() {
                Some(username) => {
                    let vhost = username;
                    let prefix = format!("{}--", self.mq_prefix);
                    if username.starts_with(&prefix) && username.len() - prefix.len() == 16 {
                        let resp = http_client
                            .delete(self.mq_api.clone() + "/users/" + username)
                            .header(reqwest::header::CONTENT_TYPE, "application/json")
                            .send()
                            .await;
                        check_mq_api_resp(resp)?;
                        let resp = http_client
                            .delete(self.mq_api.clone() + "/vhosts/" + vhost)
                            .header(reqwest::header::CONTENT_TYPE, "application/json")
                            .send()
                            .await;
                        check_mq_api_resp(resp)?;
                    }
                }
                None => {}
            }
        }
        Ok(())
    }

    async fn create_queue(&self, user_uri: &str, queue_name: &str) -> Result<String, String> {
        let channel = self.connect(user_uri).await?;
        let queue = match channel
            .queue_declare(
                queue_name,
                QueueDeclareOptions::default(),
                FieldTable::default(),
            )
            .await
        {
            Ok(queue) => queue,
            Err(e) => return Err(format!("MQ Queue Creation Error: {}", e)),
        };
        Ok(queue.name().to_string())
    }

    async fn delete_queue(&self, user_uri: &str, queue_name: &str) -> Result<(), String> {
        let channel = self.connect(user_uri).await?;
        match channel
            .queue_delete(queue_name, QueueDeleteOptions::default())
            .await
        {
            Ok(_) => {}
            Err(e) => return Err(format!("MQ Queue Deletion Error: {}", e)),
        };
        Ok(())
    }

    async fn queue_bind(&self, user_uri: &str, queue_name: &str, key: &str) -> Result<(), String> {
        let channel = self.connect(user_uri).await?;
        match channel
            .queue_bind(
                queue_name,
                "CoLink",
                key,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await
        {
            Ok(x) => x,
            Err(e) => return Err(format!("MQ Bind Error: {}", e)),
        };
        Ok(())
    }

    async fn publish_message(
        &self,
        user_uri: &str,
        key: &str,
        payload: &[u8],
    ) -> Result<(), String> {
        let channel = self.connect(user_uri).await?;
        match match channel
            .basic_publish(
                "CoLink",
                key,
                BasicPublishOptions::default(),
                payload,
                BasicProperties::default(),
            )
            .await
        {
            Ok(x) => x,
            Err(e) => return Err(format!("MQ Publish Error: {}", e)),
        }
        .await
        {
            Ok(x) => x,
            Err(e) => return Err(format!("MQ Publish Error: {}", e)),
        };
        Ok(())
    }
}

impl RabbitMQ {
    async fn connect(&self, user_uri: &str) -> Result<Channel, String> {
        let user_uri = match url::Url::parse(user_uri) {
            Ok(uri) => uri,
            Err(e) => return Err(format!("MQ URI Parse Error: {}", e)),
        };
        let vhost_path = user_uri.path();
        let uri = self.mq_amqp.clone() + vhost_path;
        let mq = match Connection::connect(&uri, ConnectionProperties::default()).await {
            Ok(mq) => mq,
            Err(e) => return Err(format!("MQ Connection Error: {}", e)),
        };
        let channel = match mq.create_channel().await {
            Ok(channel) => channel,
            Err(e) => return Err(format!("MQ Connection Error: {}", e)),
        };
        Ok(channel)
    }
}

fn check_mq_api_resp(
    resp: Result<reqwest::Response, reqwest::Error>,
) -> Result<reqwest::Response, String> {
    let resp = match resp {
        Ok(resp) => resp,
        Err(e) => return Err(format!("MQ API Error: {}", e)),
    };
    if resp.status() != reqwest::StatusCode::OK
        && resp.status() != reqwest::StatusCode::CREATED
        && resp.status() != reqwest::StatusCode::NO_CONTENT
    {
        return Err(format!("MQ API Error: {}", resp.status()));
    }
    Ok(resp)
}
