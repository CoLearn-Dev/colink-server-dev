use crate::storage::common::Storage;

/// Storage With Subscription.
#[async_trait::async_trait]
pub trait StorageWithSubscription: Storage {
    async fn subscribe(
        &self,
        user_id: &str,
        key_name: &str,
        start_timestamp: i64,
    ) -> Result<String, String>;
    async fn unsubscribe(&self, user_id: &str, queue_name: &str) -> Result<(), String>;
}
