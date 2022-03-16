use log::LevelFilter;

#[test]
fn basic() {
    env_logger::builder().filter_level(LevelFilter::Info).init();
    log::trace!("from adsead");
    log::debug!("debug");
    log::info!("start...");
    log::warn!("warning");
    log::error!("error!");
}
