use std::collections::HashMap;

/// CoLink Storage.
/// `key_path` is in the following format: `{user_id} :: {key_name} @ {timestamp}`
#[async_trait::async_trait]
pub trait Storage: Send + Sync {
    /// Create a new entry in the storage. Create inputs a key and a value.
    ///
    /// # Inputs
    /// * `user_id`: The user's public_key, serialized compactly in base64.
    /// * `key_name`: `key_path` = `{user_id} :: {key_name} @ {timestamp}`
    /// * `value`: the value to store in bytes
    /// # Returns
    /// * `Ok(key_path)` if the entry was created successfully.
    /// Notice that this `key_path` is the entire key_path with the `@timestamp` suffix, where the timestamp is the time when the entry is created.
    /// # How it works
    /// Inside storage,
    /// - add entry that maps from `{user_id} :: {key_name}` to current timestamp. If it already exists, returns error string.
    /// - add entry that maps from `{user_id} :: {key_name} @ {current timestamp}` to `value`.
    /// Returns the complete key_path of the new entry, which is `{user_id} :: {key_name} @ {current timestamp}`.
    async fn create(&self, user_id: &str, key_name: &str, value: &[u8]) -> Result<String, String>;

    /// Read entries in the storage from the given `key_path`s.
    ///
    /// # Inputs
    /// * `key_paths`: A list of `key_path`s in the format of `{user_id} :: {key_name} @ {timestamp}` to read from.
    ///
    /// # Returns
    /// * A map from key paths to payloads `Ok(HashMap<String, Vec<u8>>))` if all entries were read successfully.
    ///
    /// If an entry is not found, an error is returned.
    ///
    /// # How it works
    /// Just read the entries in the storage.
    async fn read_from_key_paths(
        &self,
        key_paths: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, String>;

    /// Read entries in the storage from the given `key_path`s.
    ///
    /// # Inputs
    /// * `key_names`: a list of `key_name`s to read from.
    ///
    /// # Returns
    /// * A map from key paths to payloads `Ok(HashMap<String, Vec<u8>>))` if all entries were read successfully.
    ///
    /// # How it works
    /// 1. Find the latest timestamp stored at `{user_id} :: {key_name}`.
    /// 2. Return the entry at `{user_id} :: {key_name} @ {latest timestamp}`.
    ///
    /// If an entry is not found, an error is returned.
    async fn read_from_key_names(
        &self,
        user_id: &str,
        key_names: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, String>;

    /// Returns all keys that starts with `prefix` if `include_history` is true, otherwise the latest key_path for each `key_path@timestamp` (the one with the largest timestamp value) that starts with `prefix`.
    ///
    /// Specifically, this finds all key_paths that starts with `prefix`, followed by a colon (":"), then have no colons in the rest of the key_path.
    /// If list_all = true, then it returns all key_paths that match the above criteria.
    /// Otherwise, it returns the latest one for all key_paths (the one with the largest timestamp after the @) that matches the above criteria.
    ///
    /// **Does not check for permissions**. This is work for the server and the SDK.
    ///
    /// `prefix` has to be none empty, otherwise return error.
    ///
    /// **Only find keys in the current level**. Example:
    /// ```
    /// /*
    /// Key paths:
    /// - A::x @ 1
    /// - A::x:y @ 1
    /// - A::x:y @ 2
    /// - A::x:y:z @ 1
    /// list_key("") -> Err
    /// list_key("A") -> Err
    /// list_key("A:") -> ["A::x@1"]
    /// list_key("A::x") -> ["A::x:y@1", "A::x:y@2"]
    /// list_key("A::x", false) -> ["A::x:y@2"]
    /// list_key("A::x:y") -> ["A::x:y:z@1"]
    /// */
    /// ```
    /// Note that if you want to list all keys possessed by a user, you need to pass in a colon after the user_id.
    ///
    /// # How it works
    /// We return all key_paths if `include_history` is false. If `include_history` is true, we find the one with the largest timestamp for each distinct `key_path_prefix` and return them.
    async fn list_keys(&self, prefix: &str, include_history: bool) -> Result<Vec<String>, String>;

    /// Updates the value of entry corresponding to `key_path_prefix` to `value`.
    ///
    /// # Inputs
    /// * `user_id`: The user's public_key, serialized compactly in base64.
    /// * `key_name`: `key_path` = `{user_id} :: {key_name} @ {timestamp}`
    /// * `value`: the value to store in bytes
    ///
    /// # Returns
    /// * `Ok(key_path)` if the entry was updated successfully.
    ///
    /// Notice that this `key_path` is the entire key_path with the `@timestamp` suffix, where the timestamp is the time when the entry is updated.
    /// # How it works
    /// Inside storage,
    /// - replace entry that maps from `{user_id}::{key_name}` to current timestamp.
    /// - add entry that maps from `{user_id}::{key_name}@{current timestamp}` to `value`.
    async fn update(&self, user_id: &str, key_name: &str, value: &[u8]) -> Result<String, String>;

    /// Updates the value of entry corresponding to `key_path_prefix` to `value`.
    ///
    /// # Inputs
    /// * `user_id`: The user's public_key, serialized compactly in base64.
    /// * `key_name`: `key_path` = `{user_id} :: {key_name} @ {timestamp}`
    /// # Returns
    /// * `Ok(key_path)` if the entry was deleted successfully.
    /// Notice that this `key_path` is the entire key_path with the `@timestamp` suffix, where the timestamp is the time when the entry is deleted.
    /// # How it works
    /// Inside storage, replace entry that maps from `{user_id}::{key_name}` to current timestamp.
    /// Now `{user_id}::{key_name}@{current timestamp}` maps to nothing. So during read, we know that a delete operation has occurred.
    async fn delete(&self, user_id: &str, key_name: &str) -> Result<String, String>;
}

/// Gets the longest prefix of the input string whose final character is a ':'.
/// If no such prefix exists, return the empty string.
pub fn get_prefix(key: &str) -> &str {
    match key.rfind(':') {
        None => "",
        Some(pos) => &key[..pos + 1],
    }
}
