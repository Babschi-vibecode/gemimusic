use gemimusic::App;
use slint::ComponentHandle;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("Starting gemimusic Desktop / Development Runner");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let app = App::new(rt.handle().clone());
    app.setup_callbacks();
    let _timer = app.start_playback_timer();

    log::info!("UI initialized, running event loop...");
    app.ui.run()?;

    Ok(())
}
