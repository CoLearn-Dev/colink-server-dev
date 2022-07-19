use crate::colink_proto::*;
use tonic::{Request, Response, Status};

impl crate::server::MyService {
    pub async fn _request_core_info(
        &self,
        request: Request<Empty>,
    ) -> Result<Response<CoreInfo>, Status> {
        let public_key_vec = self.public_key.serialize().to_vec();
        Ok(Response::new(CoreInfo {
            mq_uri: match Self::check_privilege(request.metadata(), &["user"]) {
                Ok(_i) => {
                    let user_id = Self::get_user_id(request.metadata());
                    let mq_uri_bytes = self._internal_storage_read(&user_id, "mq_uri").await?;
                    String::from_utf8(mq_uri_bytes).unwrap()
                }
                Err(_e) => Default::default(),
            },
            core_public_key: public_key_vec,
        }))
    }
}
