use bytes::{Buf, BytesMut};
use log::{info, warn, debug};
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
        // 如果[0..4]不是规定字符，就舍弃这些数据，直到符合
        if !buf.starts_with(&0xCAFEBABE_u32.to_le_bytes()) {
            info!(
                "full received: {:?}",
                buf.chunk().iter().map(|x| *x as char).collect::<String>()
            );
            for i in 0..buf.len() {
                if buf[i].eq(&0xCAu8) {
                    buf.advance(i);
                }
            }
            return None;
        }
        // 如果[4..8]不足，说明没有read完，需要继续read
        if buf.len() <= 8 {
            return None;
        }
        // 拿到数据长度，如果缓冲区大小还不够，就说明没有read完
        let size = buf.get(4..8).unwrap();
        let size = size.clone().get_u32_le();
        info!("buf.len： {} {} {}", buf.len(), buf.capacity(), size + 8);
        if buf.len() < (size + 8) as usize {
            return None;
        }

        // 分离出单个package，多余的数据会被保留到下一次解析
        debug!(
            "full received: {:?}",
            buf.chunk().iter().map(|x| *x as char).collect::<String>()
        );
        let mut ret = buf.split_to(size as usize + 8);
        // 跳过前面的标志位
        debug!(
            "only data package: {:?}",
            ret.chunk().iter().map(|x| *x as char).collect::<String>()
        );
        ret.advance(8);
        if let Ok(package) = bincode::deserialize(ret.chunk()) {
            info!("good");
            return Some(package);
        }
        warn!("this is not DataPackage");
        None
    }
}

#[cfg(test)]
mod test {

    #[test]
    fn test_split() {
        let magic = 0xCAFEBABE_u32;
        let data = "aaaaa".to_string();
        let data_len = data.as_bytes().len();
        let mut bytes: Vec<u8> = Vec::new();
        bytes.append(&mut magic.to_le_bytes().to_vec());
        bytes.append(&mut data_len.to_le_bytes().to_vec());
        bytes.append(&mut data.as_bytes().to_vec());
        println!("{:?}", bytes);
        println!("{:?}", bytes.iter().map(|x| *x as char).collect::<String>());
    }

    use tokio::io::{self, AsyncWriteExt};

    #[tokio::test]
    async fn main() -> io::Result<()> {
        let mut writer = Vec::new();

        writer.write_u32(267).await?;
        writer.write_u32(1205419366).await?;

        assert_eq!(writer, b"\x00\x00\x01\x0b\x47\xd9\x3d\x66");
        Ok(())
    }
}
