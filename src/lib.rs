pub mod backend;
pub mod config;
pub mod grpc;
pub mod http_backend;
pub mod memory;
pub mod metrics;
pub mod model;
pub mod rest;

pub mod google {
    pub mod pubsub {
        pub mod v1 {
            tonic::include_proto!("google.pubsub.v1");
        }
    }
}
