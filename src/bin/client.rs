use net_piercer::client;
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    net_piercer::log::init_logger();
    let mut client = client::Client::from_toml("client.toml")?;
    client.run().await?;
    Ok(())
}
