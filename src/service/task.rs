use super::utils::*;
use crate::colink_proto::*;
use openssl::sha::sha256;
use prost::Message;
use secp256k1::{ecdsa::Signature, PublicKey, Secp256k1};
use tonic::{Request, Response, Status};
use uuid::Uuid;

impl crate::server::MyService {
    pub async fn _create_task(&self, request: Request<Task>) -> Result<Response<Task>, Status> {
        Self::check_user_token(request.metadata())?;
        let user_id = Self::get_user_id(request.metadata());
        let task_id = Uuid::new_v4();
        let mut task = request.into_inner();
        task.decisions
            .resize(task.participants.len(), Default::default());
        task.task_id = task_id.to_string();
        task.status = if task.require_agreement {
            "approved".to_string()
        } else {
            "started".to_string()
        };
        task.decisions[0] = self
            .generate_decision(true, false, "", &user_id, &task)
            .await?;
        let mut payload = vec![];
        task.encode(&mut payload).unwrap();
        self._internal_storage_update(&user_id, &format!("tasks:{}", task_id), &payload)
            .await?;
        for i in 1..task.participants.len() {
            let core_addr = self
                ._internal_storage_read(
                    &user_id,
                    &format!("known_users:{}:core_addr", &task.participants[i].user_id),
                )
                .await?;
            let core_addr = String::from_utf8(core_addr).unwrap();
            let mut client = match self._grpc_connect(&core_addr).await {
                Ok(client) => client,
                Err(e) => return Err(Status::internal(format!("{}", e))),
            };
            let guest_jwt = self
                ._internal_storage_read(
                    &user_id,
                    &format!("known_users:{}:guest_jwt", &task.participants[i].user_id),
                )
                .await?;
            let guest_jwt = String::from_utf8(guest_jwt).unwrap();
            client
                .inter_core_sync_task(generate_request(&guest_jwt, task.clone()))
                .await?;
        }

        let task_storage_mutex = self.task_storage_mutex.lock().await;
        self.add_task_new_status(&user_id, &task).await?;
        drop(task_storage_mutex);

        Ok(Response::new(Task {
            task_id: task_id.to_string(),
            ..Default::default()
        }))
    }

    pub async fn _confirm_task(
        &self,
        request: Request<ConfirmTaskRequest>,
    ) -> Result<Response<Empty>, Status> {
        Self::check_user_token(request.metadata())?;
        let user_id = Self::get_user_id(request.metadata());
        let user_decision = match request.get_ref().decision.clone() {
            Some(user_decision) => user_decision,
            None => {
                return Err(Status::invalid_argument(
                    "The decision does not exist.".to_string(),
                ));
            }
        };
        let user_status = if user_decision.is_approved && !user_decision.is_rejected {
            "approved".to_string()
        } else if !user_decision.is_approved && user_decision.is_rejected {
            "rejected".to_string()
        } else if !user_decision.is_approved && !user_decision.is_rejected {
            "ignored".to_string()
        } else {
            return Err(Status::invalid_argument("Invalid decision.".to_string()));
        };
        let task_storage_mutex = self.task_storage_mutex.lock().await;
        let task = self
            ._internal_storage_read(&user_id, &format!("tasks:{}", request.get_ref().task_id))
            .await?;
        let mut task: Task = Message::decode(&*task).unwrap();
        if task.status != "waiting" {
            return Err(Status::internal(format!(
                "Task {} has already confirmed.",
                task.task_id
            )));
        }
        if chrono::Utc::now().timestamp() > task.expiration_time {
            return Err(Status::internal(format!(
                "Task {} has expired.",
                task.task_id
            )));
        }
        self.remove_task_old_status(&user_id, &task).await?;
        for i in 0..task.participants.len() {
            if task.participants[i].user_id == user_id {
                task.decisions[i] = self
                    .generate_decision(
                        user_decision.is_approved,
                        user_decision.is_rejected,
                        &user_decision.reason,
                        &user_id,
                        &task,
                    )
                    .await?;
            }
        }
        let approved_decisions_num = task
            .decisions
            .iter()
            .filter(|x| x.is_approved && !x.is_rejected)
            .count();
        task.status = if (task.require_agreement
            && approved_decisions_num == task.participants.len())
            || (!task.require_agreement && user_status == "approved")
        {
            "started".to_string()
        } else {
            user_status.clone()
        };
        let mut payload = vec![];
        task.encode(&mut payload).unwrap();
        self._internal_storage_update(&user_id, &format!("tasks:{}", task.task_id), &payload)
            .await?;
        self.add_task_new_status(&user_id, &task).await?;
        drop(task_storage_mutex);

        if task.require_agreement && user_status != "ignored" {
            let core_addr = self
                ._internal_storage_read(
                    &user_id,
                    &format!("known_users:{}:core_addr", &task.participants[0].user_id),
                )
                .await?;
            let core_addr = String::from_utf8(core_addr).unwrap();
            let mut client = match self._grpc_connect(&core_addr).await {
                Ok(client) => client,
                Err(e) => return Err(Status::internal(format!("{}", e))),
            };
            let guest_jwt = self
                ._internal_storage_read(
                    &user_id,
                    &format!("known_users:{}:guest_jwt", &task.participants[0].user_id),
                )
                .await?;
            let guest_jwt = String::from_utf8(guest_jwt).unwrap();
            client
                .inter_core_sync_task(generate_request(
                    &guest_jwt,
                    Task {
                        task_id: task.task_id.clone(),
                        decisions: task.decisions.clone(),
                        ..Default::default()
                    },
                ))
                .await?;
        }

        Ok(Response::new(Empty::default()))
    }

    pub async fn _finish_task(&self, request: Request<Task>) -> Result<Response<Empty>, Status> {
        Self::check_user_token(request.metadata())?;
        let user_id = Self::get_user_id(request.metadata());
        let task_storage_mutex = self.task_storage_mutex.lock().await;
        let task = self
            ._internal_storage_read(&user_id, &format!("tasks:{}", request.get_ref().task_id))
            .await?;
        let mut task: Task = Message::decode(&*task).unwrap();
        if task.status != "started" {
            return Err(Status::internal(format!(
                "Task {} has already finished or has not started.",
                task.task_id
            )));
        }
        self.remove_task_old_status(&user_id, &task).await?;
        task.status = "finished".to_string();
        let mut payload = vec![];
        task.encode(&mut payload).unwrap();
        self._internal_storage_update(&user_id, &format!("tasks:{}", task.task_id), &payload)
            .await?;
        self.add_task_new_status(&user_id, &task).await?;
        drop(task_storage_mutex);

        Ok(Response::new(Empty::default()))
    }

    pub async fn _inter_core_sync_task(
        &self,
        request: Request<Task>,
    ) -> Result<Response<Empty>, Status> {
        Self::check_user_token(request.metadata())?;
        let user_id = Self::get_user_id(request.metadata());
        if !self
            ._internal_storage_contains(&user_id, &format!("tasks:{}", request.get_ref().task_id))
            .await?
        {
            // We should create a new task with waiting status if task_id is not found in the storage for the current user.
            let mut task = request.into_inner();
            self.check_decision(&task.decisions[0], &task.participants[0].user_id, &task)?;
            if !task.decisions[0].is_approved {
                return Err(Status::internal(
                    "Initiator's decision is not approved.".to_string(),
                ));
            }
            task.status = "waiting".to_string();
            let mut payload = vec![];
            task.encode(&mut payload).unwrap();
            self._internal_storage_update(&user_id, &format!("tasks:{}", task.task_id), &payload)
                .await?;

            let task_storage_mutex = self.task_storage_mutex.lock().await;
            self.add_task_new_status(&user_id, &task).await?;
            drop(task_storage_mutex);
        } else {
            // We should update the decisions of the task in the storage if task_id is exist in the storage.
            let task_storage_mutex = self.task_storage_mutex.lock().await;
            let task = self
                ._internal_storage_read(&user_id, &format!("tasks:{}", request.get_ref().task_id))
                .await?;
            let mut task: Task = Message::decode(&*task).unwrap();
            if !task.require_agreement {
                return Err(Status::invalid_argument(format!(
                    "Task {} do not need the agreement.",
                    task.task_id
                )));
            }
            if chrono::Utc::now().timestamp() > task.expiration_time {
                return Err(Status::internal(format!(
                    "Task {} has expired.",
                    task.task_id
                )));
            }

            for i in 0..task.participants.len() {
                if task.participants[i].user_id != user_id
                    && request.get_ref().decisions[i] != Default::default()
                {
                    self.check_decision(
                        &request.get_ref().decisions[i],
                        &task.participants[i].user_id,
                        &task,
                    )?;
                    task.decisions[i] = request.get_ref().decisions[i].clone();
                }
            }
            let valid_decisions_num = task
                .decisions
                .iter()
                .filter(|x| x.is_approved ^ x.is_rejected)
                .count();
            let anyone_rejected = task
                .decisions
                .iter()
                .filter(|x| !x.is_approved && x.is_rejected)
                .count()
                > 0;
            let mut payload = vec![];
            task.encode(&mut payload).unwrap();
            self._internal_storage_update(&user_id, &format!("tasks:{}", task.task_id), &payload)
                .await?;
            drop(task_storage_mutex);

            // Change this task's status.
            if task.status == "started" || task.status == "rejected" {
                return Ok(Response::new(Empty::default()));
            }
            if valid_decisions_num == task.participants.len() || anyone_rejected {
                let task_storage_mutex = self.task_storage_mutex.lock().await;
                let task = self
                    ._internal_storage_read(
                        &user_id,
                        &format!("tasks:{}", request.get_ref().task_id),
                    )
                    .await?;
                let mut task: Task = Message::decode(&*task).unwrap();
                self.remove_task_old_status(&user_id, &task).await?;
                if anyone_rejected {
                    task.status = "rejected".to_string();
                } else {
                    task.status = "started".to_string();
                }
                let mut payload = vec![];
                task.encode(&mut payload).unwrap();
                self._internal_storage_update(
                    &user_id,
                    &format!("tasks:{}", task.task_id),
                    &payload,
                )
                .await?;
                self.add_task_new_status(&user_id, &task).await?;
                drop(task_storage_mutex);

                // The initiator should broadcast the status change.
                if task.participants[0].user_id == user_id && task.participants.len() > 2 {
                    for i in 1..task.participants.len() {
                        let core_addr = self
                            ._internal_storage_read(
                                &user_id,
                                &format!("known_users:{}:core_addr", &task.participants[i].user_id),
                            )
                            .await?;
                        let core_addr = String::from_utf8(core_addr).unwrap();
                        let mut client = match self._grpc_connect(&core_addr).await {
                            Ok(client) => client,
                            Err(e) => return Err(Status::internal(format!("{}", e))),
                        };
                        let guest_jwt = self
                            ._internal_storage_read(
                                &user_id,
                                &format!("known_users:{}:guest_jwt", &task.participants[i].user_id),
                            )
                            .await?;
                        let guest_jwt = String::from_utf8(guest_jwt).unwrap();
                        client
                            .inter_core_sync_task(generate_request(&guest_jwt, task.clone()))
                            .await?;
                    }
                }
            }
        }
        Ok(Response::new(Empty::default()))
    }

    async fn remove_task_from_list(
        &self,
        user_id: &str,
        task: &Task,
        list_key: &str,
    ) -> Result<(), Status> {
        let list = self._internal_storage_read(user_id, list_key).await?;
        let mut list: CoLinkInternalTaskIdList = Message::decode(&*list).unwrap();
        let mut index = list.task_ids_with_key_paths.len();
        for i in 0..list.task_ids_with_key_paths.len() {
            if list.task_ids_with_key_paths[i].task_id == task.task_id {
                index = i;
                break;
            }
        }
        if index == list.task_ids_with_key_paths.len() {
            return Err(Status::internal("Task Not found."));
        }
        list.task_ids_with_key_paths.remove(index);
        let mut payload = vec![];
        list.encode(&mut payload).unwrap();
        self._internal_storage_update(user_id, list_key, &payload)
            .await?;
        Ok(())
    }

    async fn remove_task_old_status(&self, user_id: &str, task: &Task) -> Result<(), Status> {
        let list_key = format!("protocols:{}:{}", task.protocol_name, task.status);
        self.remove_task_from_list(user_id, task, &list_key).await?;
        let list_key = format!("tasks:status:{}", task.status);
        self.remove_task_from_list(user_id, task, &list_key).await?;
        Ok(())
    }

    async fn add_task_to_list(
        &self,
        user_id: &str,
        task: &Task,
        list_key: &str,
    ) -> Result<(), Status> {
        let latest_key = format!("{}:latest", list_key);
        let mut payload = vec![];
        Task {
            task_id: task.task_id.clone(),
            ..Default::default()
        }
        .encode(&mut payload)
        .unwrap();
        let key_path = self
            ._internal_storage_update(user_id, &latest_key, &payload)
            .await?;
        let mut list = if self._internal_storage_contains(user_id, list_key).await? {
            let list = self._internal_storage_read(user_id, list_key).await?;
            Message::decode(&*list).unwrap()
        } else {
            CoLinkInternalTaskIdList {
                task_ids_with_key_paths: vec![],
            }
        };
        list.task_ids_with_key_paths
            .push(CoLinkInternalTaskIdWithKeyPath {
                key_path,
                task_id: task.task_id.clone(),
            });
        payload = vec![];
        list.encode(&mut payload).unwrap();
        self._internal_storage_update(user_id, list_key, &payload)
            .await?;
        Ok(())
    }

    async fn add_task_new_status(&self, user_id: &str, task: &Task) -> Result<(), Status> {
        let list_key = format!("protocols:{}:{}", task.protocol_name, task.status);
        self.add_task_to_list(user_id, task, &list_key).await?;
        let list_key = format!("tasks:status:{}", task.status);
        self.add_task_to_list(user_id, task, &list_key).await?;
        Ok(())
    }

    /**
     * This function only checks the validity of the signature, user's decision will not be checked.
     */
    fn check_decision(
        &self,
        decision: &Decision,
        user_id: &str,
        task: &Task,
    ) -> Result<(), Status> {
        if decision.signature.is_empty() {
            return Err(Status::invalid_argument("The signature is empty"));
        }
        let core_public_key_vec: &Vec<u8> = &decision.core_public_key;
        let core_public_key: PublicKey = match PublicKey::from_slice(core_public_key_vec) {
            Ok(pk) => pk,
            Err(e) => {
                return Err(Status::invalid_argument(format!(
                "The core public key could not be decoded in compressed serialized format: {:?}",
                e
                )))
            }
        };
        let signature: &Vec<u8> = &decision.signature;
        let signature = match Signature::from_compact(signature) {
            Ok(sig) => sig,
            Err(e) => {
                return Err(Status::invalid_argument(format!(
                    "The signature could not be decoded in ECDSA: {}",
                    e
                )))
            }
        };
        // We define verify_decision, which is a duplicate for decision with only is_approved/is_rejected(skipping reason for now), to help verify the signature.
        let verify_decision: Decision = Decision {
            is_approved: decision.is_approved,
            is_rejected: decision.is_rejected,
            reason: decision.clone().reason,
            ..Default::default()
        };
        // Check the core's signature first
        let mut task_for_check = task.clone();
        task_for_check.decisions = Default::default();
        task_for_check.status = Default::default();
        let mut msg: Vec<u8> = vec![];
        task_for_check.encode(&mut msg).unwrap();
        let mut verify_decision_bytes: Vec<u8> = vec![];
        verify_decision.encode(&mut verify_decision_bytes).unwrap();
        let mut user_consent_bytes: Vec<u8> = vec![];
        decision
            .user_consent
            .as_ref()
            .unwrap()
            .encode(&mut user_consent_bytes)
            .unwrap();
        msg.extend_from_slice(&verify_decision_bytes);
        msg.extend_from_slice(&user_consent_bytes);
        let verify_signature = secp256k1::Message::from_slice(&sha256(&msg)).unwrap();
        let secp = Secp256k1::new();
        match secp.verify_ecdsa(&verify_signature, &signature, &core_public_key) {
            Ok(_) => {}
            Err(e) => {
                return Err(Status::invalid_argument(format!(
                    "Invalid Signature: {}",
                    e
                )))
            }
        }
        self.check_user_consent(decision.user_consent.as_ref().unwrap(), core_public_key_vec)?;
        // After checking user consent, we need to verify that the user_id match up with the UserConsent's user public key.
        let user_pubilc_key_vec_from_userid: &Vec<u8> = &base64::decode(user_id).unwrap();
        let user_public_key_vec_from_userconsent: &Vec<u8> =
            &decision.user_consent.as_ref().unwrap().public_key;
        if user_pubilc_key_vec_from_userid != user_public_key_vec_from_userconsent {
            return Err(Status::invalid_argument(
                "UserConsent is not for the current participant",
            ));
        }
        if decision.is_approved && decision.is_rejected {
            Err(Status::invalid_argument(
                "The decision's is_approved and is_rejected are true at the same time.",
            ))
        } else {
            Ok(())
        }
    }

    async fn generate_decision(
        &self,
        is_approved: bool,
        is_rejected: bool,
        reason: &str,
        user_id: &str,
        task: &Task,
    ) -> Result<Decision, Status> {
        let user_consent_bytes = self._internal_storage_read(user_id, "user_consent").await?;
        let mut decision: Decision = Decision {
            is_approved,
            is_rejected,
            reason: reason.to_string(),
            ..Default::default()
        };
        let mut task_for_sign = task.clone();
        task_for_sign.decisions = Default::default();
        task_for_sign.status = Default::default();
        let mut msg: Vec<u8> = vec![];
        task_for_sign.encode(&mut msg).unwrap();
        let mut decision_bytes: Vec<u8> = vec![];
        decision.encode(&mut decision_bytes).unwrap();
        msg.extend_from_slice(&decision_bytes);
        msg.extend_from_slice(&user_consent_bytes);
        let secp = Secp256k1::new();
        let signature = secp.sign_ecdsa(
            &secp256k1::Message::from_slice(&sha256(&msg)).unwrap(),
            &self.secret_key,
        );
        decision.user_consent = Some(Message::decode(&*user_consent_bytes).unwrap());
        decision.core_public_key = self.public_key.serialize().to_vec();
        decision.signature = signature.serialize_compact().to_vec();
        Ok(decision)
    }
}
