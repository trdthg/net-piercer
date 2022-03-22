use anyhow::{Context, Error, Result};
use bytes::{Buf, BufMut, BytesMut};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::{self, UnboundedSender};
use uuid::Uuid;

use crate::protocal::{ClientMsg, DataPackage, ServerMsg};
use crate::{client, protocal};

pub struct Server {
    clients: Arc<Mutex<HashMap<Uuid, ClientState>>>,
    config: Config,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    // 自己的ip，端口
    pub host: String,
    pub port: u16,
    // 所有可能连接的客户端
    pub client: Vec<ClientConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClientConfig {
    pub name: String,
    pub port: u16,
    pub protocol: String,
    pub secret_key: String,
}

#[derive(Debug, Deserialize, Clone)]
struct ClientState {
    name: String,
}

impl Server {
    pub fn from_toml(path: &str) -> Result<Self> {
        let mut buf = String::new();
        BufReader::new(
            File::open(path)
                .with_context(|| format!("can't find server config file: {:?}", path))?,
        )
        .read_to_string(&mut buf)
        .with_context(|| format!("read server config failed: {}", path))?;
        let config: Config = toml::from_str(&buf)
            .with_context(|| format!("deseralize server config from {} failed", path))?;
        let clients = Arc::new(Mutex::new(HashMap::new()));
        Ok(Self { config, clients })
    }

    pub async fn run(&self) -> Result<()> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        let listener = TcpListener::bind(&addr)
            .await
            .with_context(|| format!("server can't bind on {} now", &addr))?;
        loop {
            if let Ok((stream, addr)) = listener.accept().await {
                let mut handler = Handler {
                    config: self.config.clone(),
                    clients: self.clients.clone(),
                    stream,
                    addr,
                };
                tokio::spawn(async move {
                    if let Err(e) = handler.run().await {
                        error!("{}", e);
                    }
                });
            }
        }
    }
}

#[derive(Debug)]
struct Handler {
    config: Config,
    clients: Arc<Mutex<HashMap<Uuid, ClientState>>>,
    stream: TcpStream,
    addr: SocketAddr, // 客户端的addr
}

impl Handler {
    pub async fn run(&mut self) -> Result<()> {
        // 检查客户端的注册请求，如果成功，就与监听客户端
        let (listener, uuid) = self
            .check_register()
            .await
            .map_err(|e| anyhow::anyhow!("{e:?}"))?
            .ok_or(anyhow::Error::msg("client not found"))
            .with_context(|| "client not found")?;
        loop {
            // 等待client再次bind到server
            let (mut stream, addr) = listener
                .accept()
                .await
                .with_context(|| "try to accept failed")?;
            if !addr.ip().eq(&self.addr.ip()) {
                stream
                    .write(
                        &bincode::serialize(&ServerMsg::CheckFailed {
                            reason: "Your ip has changed".to_string(),
                        })
                        .with_context(|| {
                            "failed to seralize response msg: ServerMsg::CheckFailed"
                        })?,
                    )
                    .await
                    .with_context(|| "unable to write to client_stream now")?;
                stream
                    .shutdown()
                    .await
                    .with_context(|| "ip changed, but failed to chutdown this connection")?;
            }
            let mut buf = BytesMut::new();
            let n = stream.read_buf(&mut buf).await?;
            let r_uuid: Uuid = bincode::deserialize(&buf[0..n])?;
            if !r_uuid.eq(&uuid) {
                break;
            }
            let res = bincode::serialize(&ServerMsg::CheckPassed)?;
            stream
                .write(&res)
                .await
                .with_context(|| "failed to write register success message back")?;

            handle_connection(stream, &listener).await;
        }

        // 监听客户端的信息

        Ok(())
    }

    pub async fn check_register(&mut self) -> Result<Option<(TcpListener, Uuid)>> {
        let mut buf = BytesMut::new();
        let size = self.stream.read_buf(&mut buf).await?;
        if let ClientMsg::Register { name, secret } = bincode::deserialize(&buf[..size])
            .with_context(|| "failed to deseralize to ClientMsg::Register")?
        {
            if let Some(client) = self
                .config
                .client
                .iter()
                .find(|&c| c.name == name && c.secret_key == secret)
            {
                // server尝试添加client的注册信息
                let uuid = Uuid::new_v4();
                self.clients.lock().unwrap().insert(
                    uuid,
                    ClientState {
                        name: client.name.clone(),
                    },
                );
                // 尝试建立本地server绑定到远程客户端
                let fake_server_addr = format!("{}:{}", &self.config.host, client.port);
                let listener = TcpListener::bind(&fake_server_addr)
                    .await
                    .with_context(|| {
                        format!("failed to bind on {} to listen client", &fake_server_addr)
                    })?;
                // 尝试返回注册成功信息
                let res = protocal::ServerMsg::Success { uuid };
                self.stream
                    .write_all(
                        &bincode::serialize(&res)
                            .with_context(|| "failed to seralize response msg")?,
                    )
                    .await
                    .with_context(|| format!("response to client failed"))?;
                // 如果上面的过程都正常
                return Ok(Some((listener, uuid)));
            } else {
                error!("failed {name}, {secret}");
                self.stream
                    .write_all(
                        &bincode::serialize(&ServerMsg::Failed {
                            reason: "your client is not allowed".to_string(),
                        })
                        .with_context(|| "seralize response msg(ServerMsg::Failed) failed")?,
                    )
                    .await
                    .with_context(|| format!("response to client failed"))?;
            }
        } else {
            self.stream
                .write_all(&bincode::serialize(&ServerMsg::Failed {
                    reason: "your msg is not register".to_string(),
                })?)
                .await
                .with_context(|| format!("response to client failed"))?;
        }
        Ok(None)
    }
}

pub async fn handle_connection(
    client_stream: TcpStream, // server <- stream <- client
    listener: &TcpListener,   // fake_server <- users
) {
    let map: Arc<Mutex<HashMap<Uuid, UnboundedSender<BytesMut>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    loop {
        let (mut client_reader, mut client_writer) = client_stream.into_split();

        let (sender, mut receiver) = mpsc::unbounded_channel();

        // server的receiver接收到后write到client
        tokio::spawn(async move {
            while let Some(package) = receiver.recv().await {
                info!("user-server channel receiver receivd new package, writing to client");
                let bin = bincode::serialize(&package).unwrap();
                client_writer
                    .write(&0xCAFEBABE_u32.to_ne_bytes())
                    .await
                    .unwrap();
                client_writer.write(&bin.len().to_ne_bytes()).await.unwrap();
                if let Err(_) = client_writer.write_all(&bin).await {
                    break;
                }
            }
        });

        let map_cloned = map.clone();
        // server从client读取返回的数据，通过channel发送到receiver
        tokio::spawn(async move {
            let mut buf = BytesMut::with_capacity(128);
            loop {
                info!("trying read from client...");
                let n = client_reader.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                while let Some(DataPackage { uuid, data }) = DataPackage::try_from_buf(&mut buf) {
                    info!("read from client, server-client receiver has processed the package, server received the result, sending to user-server channel receiver");
                    if let Some(sender) = map_cloned.lock().unwrap().get(&uuid) {
                        sender.send(data).unwrap();
                    }
                }
            }
        });

        loop {
            let sender = sender.clone();
            let (user_stream, user_addr) = listener.accept().await.unwrap();
            info!("new user accepted: {}", user_addr);
            let (mut user_reader, mut user_writer) = user_stream.into_split();

            let user_uuid = Uuid::new_v4();
            let (tx, mut rx) = mpsc::unbounded_channel();
            map.lock().unwrap().insert(user_uuid, tx.clone());
            // sender将user数据通过channel发送到receiver
            tokio::spawn(async move {
                let mut buf = [0u8; 512];
                loop {
                    let n = user_reader.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    info!("package user request, sending to user-server channel receiver");
                    let mut bytes = BytesMut::with_capacity(512);
                    bytes.put(&buf[0..n]);
                    let package = DataPackage::new(user_uuid, bytes);
                    sender.send(package).unwrap();
                }
            });

            // receiver接收到数据，解包后发送给对应的user
            tokio::spawn(async move {
                while let Some(data) = rx.recv().await {
                    info!("user-server channel sender sending to user");
                    user_writer.write(data.chunk()).await.unwrap();
                }
            });
        }
    }
}
