use crate::colink_proto::*;
use tonic::{Request, Response, Status};
use tracing::debug;

impl crate::server::MyService {
    pub async fn _create_entry(
        &self,
        request: Request<StorageEntry>,
    ) -> Result<Response<StorageEntry>, Status> {
        Self::check_user_or_host_token(request.metadata())?;
        let user_id = Self::get_user_id(request.metadata());
        let body: StorageEntry = request.into_inner();
        let key_name: String = body.key_name;
        let payload: Vec<u8> = body.payload;
        match self.storage.create(&user_id, &key_name, &payload).await {
            Ok(key_path) => Ok(Response::new(StorageEntry {
                key_name: Default::default(),
                key_path,
                payload: Default::default(),
            })),
            Err(e) => Err(Status::internal(e)),
        }
    }

    pub async fn _read_entries(
        &self,
        request: Request<StorageEntries>,
    ) -> Result<Response<StorageEntries>, Status> {
        Self::check_user_or_host_token(request.metadata())?;
        let user_id = Self::get_user_id(request.metadata());
        let body: StorageEntries = request.into_inner();
        let entries: Vec<StorageEntry> = body.entries;
        let mut key_names_vec: Vec<String> = Vec::new();
        let mut key_paths_vec: Vec<String> = Vec::new();

        for entry in entries {
            let key_path: String = entry.key_path;
            let key_name: String = entry.key_name;
            debug!("key_path is {}\n, key_name is {}\n", key_path, key_name);
            if key_path.is_empty() && key_name.is_empty() {
                return Err(Status::invalid_argument(
                    "both key_path and key_name are empty",
                ));
            } else if !key_path.is_empty() && !key_name.is_empty() {
                return Err(Status::invalid_argument(
                    "both key_path and key_name are not empty",
                ));
            } else if !key_name.is_empty() {
                key_names_vec.push(key_name);
            } else {
                key_paths_vec.push(key_path);
            }
        }

        let mut entries_vec: Vec<StorageEntry> = Vec::new();

        let payload_returned_from_key_paths =
            match self.storage.read_from_key_paths(&key_paths_vec).await {
                Ok(entries) => entries,
                Err(e) => return Err(Status::internal(e)),
            };
        let payload_returned_from_key_names = match self
            .storage
            .read_from_key_names(&user_id, &key_names_vec)
            .await
        {
            Ok(entries) => entries,
            Err(e) => return Err(Status::internal(e)),
        };
        debug!(
            "payload_returned_from_key_paths is {:?}",
            payload_returned_from_key_paths
        );
        debug!(
            "payload_returned_from_key_names is {:?}",
            payload_returned_from_key_names
        );
        for (key_path, payload) in payload_returned_from_key_paths {
            entries_vec.push(StorageEntry {
                key_name: Default::default(),
                key_path,
                payload,
            });
        }
        for (key_path, payload) in payload_returned_from_key_names {
            entries_vec.push(StorageEntry {
                key_name: Default::default(),
                key_path,
                payload,
            });
        }

        Ok(Response::new(StorageEntries {
            entries: entries_vec,
        }))
    }

    pub async fn _update_entry(
        &self,
        request: Request<StorageEntry>,
    ) -> Result<Response<StorageEntry>, Status> {
        Self::check_user_or_host_token(request.metadata())?;
        let user_id = Self::get_user_id(request.metadata());
        let body: StorageEntry = request.into_inner();
        let key_name: String = body.key_name;
        let payload: Vec<u8> = body.payload;
        let key_path = self.storage.update(&user_id, &key_name, &payload).await;

        let key_path = match key_path {
            Ok(key_path) => key_path,
            Err(e) => return Err(Status::internal(e)),
        };

        Ok(Response::new(StorageEntry {
            key_name: Default::default(),
            key_path,
            payload: Default::default(),
        }))
    }

    pub async fn _delete_entry(
        &self,
        request: Request<StorageEntry>,
    ) -> Result<Response<StorageEntry>, Status> {
        Self::check_user_or_host_token(request.metadata())?;
        let user_id = Self::get_user_id(request.metadata());
        let body: StorageEntry = request.into_inner();
        let key_name: String = body.key_name;
        let key_path = self.storage.delete(&user_id, &key_name).await;

        let key_path = match key_path {
            Ok(key_path) => key_path,
            Err(e) => return Err(Status::internal(e)),
        };

        Ok(Response::new(StorageEntry {
            key_name: Default::default(),
            key_path,
            payload: Default::default(),
        }))
    }

    pub async fn _read_keys(
        &self,
        request: Request<ReadKeysRequest>,
    ) -> Result<Response<StorageEntries>, Status> {
        Self::check_user_or_host_token(request.metadata())?;
        let user_id = Self::get_user_id(request.metadata());
        let body: ReadKeysRequest = request.into_inner();
        let prefix: String = body.prefix;
        let include_history: bool = body.include_history;
        if !prefix.starts_with(&user_id) {
            return Err(Status::invalid_argument(
                "prefix must start with the given user_id",
            ));
        }
        let keys = self.storage.list_keys(&prefix, include_history).await;
        match keys {
            Ok(key_paths) => {
                let mut ret: Vec<StorageEntry> = Vec::new();
                for key in key_paths {
                    ret.push(StorageEntry {
                        key_name: Default::default(),
                        key_path: key,
                        payload: Default::default(),
                    });
                }
                Ok(Response::new(StorageEntries { entries: ret }))
            }
            Err(e) => Err(Status::aborted(e)),
        }
    }
}
