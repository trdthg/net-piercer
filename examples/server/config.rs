use log::{error, info, warn};
use serde::Deserialize;

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

pub fn read_server_configuration() -> Result<Config, ()> {
    if let Ok(s) = std::fs::read_to_string("server.toml") {
        match toml::from_str(s.as_str()) {
            Ok(conf) => {
                info!("Read Config:{:?}", conf);
                return Ok(conf);
            }
            Err(e) => {
                error!("Error while read server.toml");
                error!("{}", e);
                return Err(());
            }
        };
    } else {
        error!("Cannot read config file.");
        return Err(());
    }
}
