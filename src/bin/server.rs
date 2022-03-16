use anyhow::Result;
use log::error;
use net_piercer::log::init_logger;

#[tokio::main]
async fn main() -> Result<()> {
    init_logger();
    let server = net_piercer::server::Server::from_toml("server.toml")?;
    if let Err(e) = server.run().await {
        error!("{}", e);
    }
    Ok(())
}
