pub mod api;
pub mod app;
pub mod audio;
pub mod config;

pub use app::App;

#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: slint::android::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("gemimusic"),
    );
    log::info!("Starting gemimusic Native Android Activity");

    slint::android::init(app).expect("Failed to initialize Slint Android backend");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create Tokio multi-thread runtime");

    let gemi_app = App::new(rt.handle().clone());
    gemi_app.setup_callbacks();
    let _timer = gemi_app.start_playback_timer();

    gemi_app.ui.run().expect("Failed to run Slint UI event loop");
}
