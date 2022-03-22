use bytes::{Buf, BytesMut};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ClientMsg {
    Register { name: String, secret: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum ServerMsg {
    Success { uuid: Uuid },
    Failed { reason: String },
    CheckPassed,
    CheckFailed { reason: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DataPackage {
    pub uuid: Uuid,
    pub data: BytesMut,
}
impl DataPackage {
    pub fn new(uuid: Uuid, data: BytesMut) -> Self {
        Self { uuid, data }
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn try_from_buf(buf: &mut BytesMut) -> Option<Self> {
        if !buf.starts_with(&0xCAFEBABE_u32.to_ne_bytes()) {
            for i in 0..buf.len() {
                if buf[i].eq(&0xCAu8) {
                    buf.advance(i);
                }
            }
        } else {
            if buf.len() > 8 {
                let size = buf.get(4..8).unwrap();
                let size = size.clone().get_u32();
                if buf.len() >= (size + 8) as usize {
                    let mut ret = buf.split_to(size as usize + 8);
                    ret.advance(8);
                    if let Ok(package) = bincode::deserialize::<DataPackage>(ret.chunk()) {
                        return Some(package);
                    }
                }
            }
        }
        None
    }
}
