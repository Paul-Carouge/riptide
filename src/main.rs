mod engine;
mod store;
mod ui;

use gpui::{
    App, AppContext, Application, Bounds, TitlebarOptions, WindowBackgroundAppearance,
    WindowBounds, WindowDecorations, WindowOptions, size,
};

fn main() {
    // Read settings and history before the event loop so the UI thread never waits on disk.
    let persisted = store::load_persisted_state();
    Application::new().run(move |cx: &mut App| {
        ui::initialize(cx);
        let bounds = Bounds::centered(
            None,
            size(ui::theme::WINDOW_WIDTH, ui::theme::WINDOW_HEIGHT),
            cx,
        );
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(
                ui::theme::WINDOW_MIN_WIDTH,
                ui::theme::WINDOW_MIN_HEIGHT,
            )),
            titlebar: Some(TitlebarOptions {
                title: Some("Riptide".into()),
                ..Default::default()
            }),
            app_id: Some("riptide".to_owned()),
            // The app draws its own title bar and frame (see ui::window_frame).
            window_decorations: Some(WindowDecorations::Client),
            window_background: WindowBackgroundAppearance::Transparent,
            ..Default::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            cx.new(|cx| ui::RiptideApp::new(window, cx, persisted))
        });
        if let Err(error) = opened {
            eprintln!("riptide: cannot open the main window: {error}");
            cx.quit();
            return;
        }
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
