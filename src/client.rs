use anyhow::{Context, Result};
use bytes::BytesMut;
use log::{error, info, warn};
use serde::Deserialize;
use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::protocal;

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
            let msg = protocal::ClientMsg::Register {
                name: client.name.clone(),
                secret: client.secret_key.clone(),
            };
            let server_addr = format!("{}:{}", self.config.server_host, self.config.server_port);
            let mut stream = TcpStream::connect(&server_addr)
                .await
                .with_context(|| format!("can't connect to frps now: {}", &server_addr))?;
            stream
                .write_all(&bincode::serialize(&msg)?)
                .await
                .with_context(|| format!("can't send register request to frps"))?;
            let mut buf = BytesMut::new();
            let size = stream
                .read_buf(&mut buf)
                .await
                .with_context(|| format!("waiting for register result timeout"))?;
            match bincode::deserialize::<protocal::ServerMsg>(&buf[0..size])
                .with_context(|| format!("deseralize fron register response failed"))?
            {
                protocal::ServerMsg::Success { uuid } => {
                    println!("{}", uuid);
                }
                protocal::ServerMsg::Failed { reason } => {
                    println!("{} is not allowed: {}", client.name, reason);
                }
            }
        }
        Ok(())
    }
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
