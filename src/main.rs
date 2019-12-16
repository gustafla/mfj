use simple_logger;
use log;
use clap;

fn main() {
    simple_logger::init().expect("Logger failed to initialize");
    log::info!("Starting version {}", clap::crate_version!());
}
