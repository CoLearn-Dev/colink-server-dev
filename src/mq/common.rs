/// MQ
#[async_trait::async_trait]
pub trait MQ: Send + Sync {
    async fn create_user_account(&self) -> Result<String, String>;
    async fn delete_user_account(&self, user_uri: &str) -> Result<(), String>;
    async fn delete_all_accounts(&self) -> Result<(), String>;
    async fn create_queue(&self, user_uri: &str, queue_name: &str) -> Result<String, String>;
    async fn delete_queue(&self, user_uri: &str, queue_name: &str) -> Result<(), String>;
    async fn queue_bind(&self, user_uri: &str, queue_name: &str, key: &str) -> Result<(), String>;
    async fn publish_message(
        &self,
        user_uri: &str,
        key: &str,
        payload: &[u8],
    ) -> Result<(), String>;
}
