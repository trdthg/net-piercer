use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ClientMsg {
    Register { name: String, secret: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ServerMsg {
    Success { uuid: String },
    Failed { reason: String },
}
