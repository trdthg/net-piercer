use anyhow::{Context, Result};
use bytes::{Buf, BytesMut};
use log::{error, info, warn};
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, Read},
    net::SocketAddr,
    path::Path,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc::{self, UnboundedSender},
};
use uuid::Uuid;

use crate::protocal::{self, DataPackage, ServerMsg};

#[derive(Debug, Deserialize, Clone)]
pub struct Client {
    config: Config,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    // 需要映射的服务器
    pub server_host: String,
    pub server_port: u16,
    // 本机的程序信息
    pub client: Vec<ClientConfig>,
}
#[derive(Debug, Deserialize, Clone)]
pub struct ClientConfig {
    pub name: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub local_ip: String,
    pub secret_key: String,
}

impl Client {
    pub fn from_toml(path: &str) -> Result<Self> {
        let mut buf = String::new();
        BufReader::new(
            File::open(path)
                .with_context(|| format!("can't find client config file: {:?}", path))?,
        )
        .read_to_string(&mut buf)
        .with_context(|| format!("read client config failed: {}", path))?;
        let config: Config = toml::from_str(&buf)
            .with_context(|| format!("deseralize client config from {} failed", path))?;
        Ok(Self { config })
    }

    pub async fn run(&mut self) -> Result<()> {
        for client in &self.config.client {
            // 准备注册信息
            let msg = protocal::ClientMsg::Register {
                name: client.name.clone(),
                secret: client.secret_key.clone(),
            };
            let server_addr = format!("{}:{}", self.config.server_host, self.config.server_port);
            // 尝试向server发起注册
            let mut stream = TcpStream::connect(&server_addr)
                .await
                .with_context(|| format!("can't connect to frps now: {}", &server_addr))?;
            stream
                .write_all(&bincode::serialize(&msg)?)
                .await
                .with_context(|| format!("can't send register request to frps"))?;
            let mut buf = BytesMut::new();
            // 获取注册结果
            let size = stream
                .read_buf(&mut buf)
                .await
                .with_context(|| format!("waiting for register result timeout"))?;
            match bincode::deserialize::<ServerMsg>(&buf[0..size])
                .with_context(|| format!("deseralize from register response failed"))?
            {
                protocal::ServerMsg::Success { uuid } => {
                    info!("register success, uuid: {}", uuid);
                    info!("trying to connect to remote fake server...");
                    let mut server_stream = TcpStream::connect(format!(
                        "{}:{}",
                        self.config.server_host, client.remote_port
                    ))
                    .await
                    .with_context(|| "connect to fake server failed")?;
                    // 开始交互前先发送uuid确认身份
                    info!("checking auth info");
                    server_stream
                        .write_all(
                            &bincode::serialize(&uuid)
                                .with_context(|| "failed to seralize uuid")?,
                        )
                        .await
                        .with_context(|| "unable to write to fake_server now")?;
                    info!("trying to get the checking response...");
                    let mut buf = BytesMut::with_capacity(128);
                    let n = server_stream
                        .read_buf(&mut buf)
                        .await
                        .with_context(|| "unable to read from server now")?;
                    info!("{:?}", &buf[0..n]);
                    let check_result: ServerMsg = bincode::deserialize(&buf[0..n])
                        .with_context(|| "failed to deseralize check response")?;
                    match check_result {
                        ServerMsg::CheckPassed => {
                            info!("check auth passed, handling data transfer");
                            handle_server(
                                server_stream,
                                format!("{}:{}", client.local_ip, client.local_port).parse()?,
                            )
                            .await;
                        }
                        ServerMsg::CheckFailed { reason } => {
                            error!("failed to check uuid: {reason}");
                        }
                        _ => {
                            error!("server response is valid, this should not be reachable");
                        }
                    }
                }
                protocal::ServerMsg::Failed { reason } => {
                    info!("{} is not allowed: {}", client.name, reason);
                }
                _ => unimplemented!(),
            }
        }
        Ok(())
    }
}

pub async fn handle_server(fake_server_stream: TcpStream, local_server_addr: SocketAddr) {
    let (mut reader, mut writer) = fake_server_stream.into_split();
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let h1 = tokio::spawn(async move {
        let mut buf = BytesMut::with_capacity(128);
        let mut map: HashMap<Uuid, UnboundedSender<BytesMut>> = HashMap::new();
        loop {
            info!("trying to accept data package");
            let n = reader
                .read_buf(&mut buf)
                .await
                .with_context(|| "unable to read from fake_server now")
                .unwrap();
            if n == 0 {
                warn!("client reader closed");
                return;
            }
            // 监听远程server，接受数据后把数据通过channel发送到receiver
            while let Some(DataPackage { uuid, data }) = DataPackage::try_from_buf(&mut buf) {
                info!("received package, uuid: {}", uuid);
                let (tx, mut rx) = mpsc::unbounded_channel();
                if !map.contains_key(&uuid) {
                    map.insert(uuid, tx);
                }
                // 接收到数据就从channel发送,准备write到本地server
                map.get(&uuid).unwrap().send(data).unwrap();
                // 与本地server建立连接
                let local_server_stream = TcpStream::connect(local_server_addr)
                    .await
                    .expect("failed to connect to local real server");
                let (mut local_server_reader, mut local_server_writer) =
                    local_server_stream.into_split();
                // 尝试向本地server写入数据
                tokio::spawn(async move {
                    while let Some(mut data) = rx.recv().await {
                        while data.has_remaining() {
                            // 不会全部写入完, 会返回写入的大小
                            info!("|1 => writing req to client");
                            local_server_writer
                                .write_buf(&mut data)
                                .await
                                .with_context(|| "unable to write to client now")
                                .unwrap();
                            info!("|1 => write finished");
                        }
                    }
                });
                // 尝试从本地client读取数据, 读取后就通过channel准备发送给user
                let sender = sender.clone();
                tokio::spawn(async move {
                    loop {
                        info!("|2 => trying to receive response from client");
                        let mut buf = BytesMut::with_capacity(512);
                        let n = local_server_reader.read_buf(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        info!("|2 => received response from client");

                        info!("|3 => sending response to channel");
                        sender.send(DataPackage { uuid, data: buf }).unwrap();
                    }
                });
            }
        }
    });
    let h2 = tokio::spawn(async move {
        loop {
            while let Some(package) = receiver.recv().await {
                // todo
                info!("|3 => received response from channel");
                let bin = bincode::serialize(&package).unwrap();
                let len = bin.len();
                info!("|4 => writing response back to fake_server");
                writer
                    .write(0xCAFEBABE_u32.to_le_bytes().as_ref())
                    .await
                    .unwrap();
                writer.write(&(len as u32).to_le_bytes()).await.unwrap();
                if let Err(_) = writer.write_all(&bin).await {
                    break;
                }
                info!("|4 => write finished");
            }
        }
    });
    h1.await.unwrap();
    h2.await.unwrap();
}

#[cfg(test)]
mod test {

    use super::*;
    #[test]
    fn read_toml() {
        let server = Client::from_toml("");
        assert!(server.is_err());
        let server = Client::from_toml("client.toml");
        assert!(server.is_ok());
    }
}
