use anyhow::{Context, Error, Result};
use bytes::BytesMut;
use log::{error, info, warn};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

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
    addr: SocketAddr,
}

impl Handler {
    pub async fn run(&mut self) -> Result<()> {
        // let client_state = ClientState::new();
        let mut buf = BytesMut::new();
        let size = self.stream.read_buf(&mut buf).await?;
        if let protocal::ClientMsg::Register { name, secret } = bincode::deserialize(&buf[..size])?
        {
            if let Some(client) = self
                .config
                .client
                .iter()
                .find(|&c| c.name == name && c.secret_key == secret)
            {
                let uuid = Uuid::new_v4();
                self.clients.lock().unwrap().insert(
                    uuid,
                    ClientState {
                        name: client.name.clone(),
                    },
                );
                let res = protocal::ServerMsg::Success {
                    uuid: uuid.to_string(),
                };
                self.stream
                    .write_all(&bincode::serialize(&res)?)
                    .await
                    .with_context(|| format!("response to client failed"))?;
                println!("{client:?}");
            } else {
                let res = protocal::ServerMsg::Failed {
                    reason: "your client is not allowed".to_string(),
                };
                println!("{:?}", res);
                self.stream
                    .write_all(&bincode::serialize(&res)?)
                    .await
                    .with_context(|| format!("response to client failed"))?;
            }
        } else {
            let res = protocal::ServerMsg::Failed {
                reason: "your msg is not for register".to_string(),
            };
            self.stream
                .write_all(&bincode::serialize(&res)?)
                .await
                .with_context(|| format!("response to client failed"))?;
        }
        Ok(())
    }
}
