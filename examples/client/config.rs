use log::{error, info, warn};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    // 需要映射的服务器
    pub server_ip: String,
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
pub fn read_configuration() -> Result<Config, ()> {
    if let Ok(s) = std::fs::read_to_string("client.toml") {
        match toml::from_str(s.as_str()) {
            Ok(conf) => {
                info!("Read Config:{:?}", conf);
                return Ok(conf);
            }
            Err(e) => {
                error!("Error while read client.toml");
                error!("{}", e);
                return Err(());
            }
        };
    } else {
        error!("Cannot read config file.");
        return Err(());
    }
}
