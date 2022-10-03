use crate::colink_proto::*;
use tonic::{Request, Response, Status};

impl crate::server::MyService {
    pub async fn _subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<MqQueueName>, Status> {
        Self::check_privilege_in(request.metadata(), &["user", "host"])?;
        let user_id = Self::get_key_from_metadata(request.metadata(), "user_id");
        let queue_name = match self
            .storage
            .subscribe(
                &user_id,
                &request.get_ref().key_name,
                request.get_ref().start_timestamp,
            )
            .await
        {
            Ok(queue_name) => queue_name,
            Err(e) => return Err(Status::internal(e)),
        };
        Ok(Response::new(MqQueueName { queue_name }))
    }

    pub async fn _unsubscribe(
        &self,
        request: Request<MqQueueName>,
    ) -> Result<Response<Empty>, Status> {
        Self::check_privilege_in(request.metadata(), &["user", "host"])?;
        let user_id = Self::get_key_from_metadata(request.metadata(), "user_id");
        match self
            .storage
            .unsubscribe(&user_id, &request.get_ref().queue_name)
            .await
        {
            Ok(_) => {}
            Err(e) => return Err(Status::internal(e)),
        };
        Ok(Response::new(Empty::default()))
    }
}
