#![windows_subsystem = "windows"]

fn main() {
    env_logger::init();
    let cfg = rview::Config::from_args(std::env::args_os().skip(1));
    if let Err(e) = rview::run(cfg) {
        log::error!("{e:#}");
        std::process::exit(1);
    }
}
