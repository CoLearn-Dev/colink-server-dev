#![feature(map_try_insert)]
pub mod mq;
pub mod server;
pub mod service;
pub mod storage;
pub mod subscription;
pub mod colink_proto {
    tonic::include_proto!("colink");
}
