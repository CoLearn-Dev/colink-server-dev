use crate::colink_proto::*;
use tonic::{Request, Response, Status};

impl crate::server::MyService {
    pub async fn _request_info(
        &self,
        request: Request<Empty>,
    ) -> Result<Response<RequestInfoResponse>, Status> {
        let public_key_vec = self.public_key.serialize().to_vec();
        let requestor_ip = match request.metadata().get("x-forwarded-for") {
            Some(header) => {
                let splits: Vec<&str> = header.to_str().unwrap().split(',').collect();
                splits[0].to_string()
            }
            None => match request.metadata().get("x-real-ip") {
                Some(header) => header.to_str().unwrap().to_string(),
                None => request.remote_addr().unwrap().ip().to_string(),
            },
        };
        Ok(Response::new(RequestInfoResponse {
            mq_uri: match Self::check_privilege_in(request.metadata(), &["user", "host"]) {
                Ok(_) => {
                    let user_id = Self::get_key_from_metadata(request.metadata(), "user_id");
                    match self._internal_storage_read(&user_id, "mq_uri").await {
                        Ok(mq_uri) => String::from_utf8(mq_uri).unwrap(),
                        Err(_) => Default::default(),
                    }
                }
                Err(_) => Default::default(),
            },
            core_public_key: public_key_vec,
            requestor_ip,
        }))
    }
}
