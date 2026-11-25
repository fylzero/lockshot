mod autostart;
mod blur;
mod capture;
mod commands;
mod corners;
mod drag_out;
mod export;
mod frame;
mod hotkeys;
#[cfg(target_os = "macos")]
mod menu;
mod ocr;
mod placement;
mod platform;
mod quick_save;
mod scrolling;
mod settings;
mod stitch;
mod system_shortcuts;
mod tray;
mod window_list;

use std::sync::Mutex;

static PENDING_ROUTE: Mutex<Option<String>> = Mutex::new(None);

/// The route the window was last asked to show, cleared once read.
#[tauri::command]
fn take_pending_route() -> Option<String> {
    PENDING_ROUTE.lock().unwrap().take()
}

use commands::AppState;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

/// Bring the main window forward, optionally on a route like
/// "/settings/hotkeys" or "/editor/<id>".
pub fn show_main(app: &AppHandle, route: Option<&str>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    /* The page may still be loading and miss the event; it asks on mount. */
    *PENDING_ROUTE.lock().unwrap() = route.map(str::to_string);
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
        if cfg!(debug_assertions) {
            let _ = app.run_on_main_thread(platform::restore_dock_icon);
        }
    }
    if let Some(route) = route {
        let _ = window.emit("navigate", route);
    }
    let _ = window.unminimize();
    /*
     * Open where the user is: on their virtual desktop, and on the screen
     * they captured on or picked in Settings. Window calls from a capture's
     * worker thread queue up on the main thread in order, so these land
     * before the show.
     */
    let target = window.clone();
    let handle = app.clone();
    let _ = window.run_on_main_thread(move || {
        platform::move_to_current_desktop(&target);
        placement::place_main(&handle, &target);
    });
    let _ = window.show();
    let _ = window.set_focus();
}

/// Back to living in the tray. On macOS that also drops the Dock icon.
pub fn hide_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}

/// `lockshot --area` / `--window` / `--scroll` / `--screen`, for desktops (Wayland) where
/// hotkeys have to be bound in the system settings instead.
fn cli_action(args: &[String]) -> Option<&'static str> {
    args.iter().skip(1).find_map(|arg| match arg.as_str() {
        "--area" => Some("area"),
        "--window" => Some("window"),
        "--scroll" => Some("scroll"),
        "--screen" => Some("screen"),
        "--open" => Some("editor"),
        _ => None,
    })
}

/// Run a hotkey or tray action. Captures run off the event loop so a slow
/// grab of several monitors never stalls the UI.
pub fn run_action(app: &AppHandle, action: &str) {
    let app = app.clone();
    match action {
        "area" | "window" | "scroll" | "screen" => {
            let action = action.to_string();
            tauri::async_runtime::spawn_blocking(move || {
                let result = match action.as_str() {
                    "area" => capture::start_area(&app),
                    "window" => capture::start_window(&app),
                    "scroll" => capture::start_scroll(&app),
                    _ => capture::capture_screen(&app),
                };
                if let Err(err) = result {
                    let _ = app.emit_to("main", "capture-error", err);
                }
            });
        }
        "editor" => show_main(&app, None),
        scrolling::STOP_ACTION => scrolling::stop(),
        _ => {}
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    /*
     * macOS only: replace Tauri's default menu, whose ⌘W closes the window
     * outright, with one where ⌘W closes the active tab first.
     */
    #[cfg(target_os = "macos")]
    let builder = builder
        .menu(menu::build)
        .on_menu_event(|app, event| menu::handle_event(app, event.id().as_ref()));
    builder
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            /* A second launch hands its arguments to the running copy. */
            match cli_action(&args) {
                Some(action) => run_action(app, action),
                None => show_main(app, None),
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(autostart::plugin())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(hotkeys::handle)
                .build(),
        )
        .register_uri_scheme_protocol("capture", capture::protocol)
        .setup(|app| {
            let handle = app.handle().clone();
            /* Spaces: the window opens on the one the user is on (macOS). */
            if let Some(main) = app.get_webview_window("main") {
                platform::follow_active_space(&main);
                /* The window is see-through behind the image (Window opacity in Settings). */
                corners::round(&main);
            }
            /*
             * A bundled build starts menu-bar-only straight away; its .app
             * supplies the name and icon. An unbundled dev binary has to
             * finish launching as a regular app first, or macOS never picks
             * up its name and icon and Cmd+Tab shows a generic "exec" entry.
             * It goes menu-bar-only on Ready (below) if no window is open.
             */
            #[cfg(all(target_os = "macos", not(debug_assertions)))]
            handle.set_activation_policy(tauri::ActivationPolicy::Accessory)?;

            let settings = settings::load(&handle);
            if let Err(err) = autostart::apply(&handle, settings.launch_at_login) {
                eprintln!("{err}");
            }
            app.manage(capture::CaptureState::default());
            app.manage(hotkeys::HotkeyState::default());
            app.manage(quick_save::ToastState::default());
            let errors = hotkeys::register_all(&handle, &settings.hotkeys);
            tray::build(&handle, &settings)?;
            /* Build the capture overlays ahead of time so area capture opens instantly. */
            let warm = handle.clone();
            let warm_text = settings.scroll_text;
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(800));
                capture::prewarm(&warm);
                quick_save::prewarm(&warm);
                /* Slow only the first time a new build reads text. */
                if warm_text {
                    ocr::warm_up();
                }
            });

            let first_run = !settings.onboarded;
            app.manage(AppState {
                settings: Mutex::new(settings),
                hotkey_errors: Mutex::new(errors.clone()),
            });
            /* First launch, or a hotkey that could not be claimed, needs the window. */
            let args: Vec<String> = std::env::args().collect();
            if let Some(action) = cli_action(&args) {
                run_action(&handle, action);
            } else if first_run {
                show_main(&handle, Some("/"));
            } else if !errors.is_empty() {
                show_main(&handle, Some("/settings/hotkeys"));
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    hide_main(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::update_settings,
            blur::set_window_blur,
            commands::set_hotkey,
            commands::reset_hotkeys,
            commands::pause_hotkeys,
            commands::validate_hotkey,
            commands::capture,
            commands::open_image,
            commands::finish_area,
            commands::finish_window,
            commands::finish_scroll,
            commands::scroll_stop,
            commands::scroll_skip_text,
            commands::overlay_pending,
            commands::overlay_refit,
            commands::cancel_area,
            commands::latest_capture,
            commands::release_capture,
            commands::save_image,
            commands::arm_drag,
            commands::drag_file,
            commands::start_drag,
            commands::reveal_path,
            commands::reveal_settings_file,
            commands::open_folder,
            commands::settings_json,
            commands::apply_settings_json,
            commands::import_settings_json,
            commands::export_settings_json,
            commands::screens,
            commands::size_main_window,
            commands::permission_status,
            commands::request_screen_permission,
            commands::request_input_permission,
            commands::open_system_settings,
            commands::system_screenshot_shortcuts,
            commands::reregister_hotkeys,
            commands::set_system_screenshot_shortcuts,
            commands::hide_main,
            commands::toast_pending,
            commands::toast_dismiss,
            commands::toast_edit,
            take_pending_route,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Lockshot")
        .run(|_app, event| match event {
            /* Closing the last window must not quit a tray app. */
            tauri::RunEvent::ExitRequested { api, code, .. } if code.is_none() => {
                api.prevent_exit();
            }
            #[cfg(all(target_os = "macos", debug_assertions))]
            tauri::RunEvent::Ready => {
                platform::set_dev_display_name(&_app.package_info().name);
                let visible = _app
                    .get_webview_window("main")
                    .and_then(|w| w.is_visible().ok())
                    .unwrap_or(false);
                if !visible {
                    let _ = _app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
            }
            _ => {}
        });
}
