// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{
    Manager,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};

fn main() {
    // Set cwd to the brivva project root so .env.local is found
    if let Ok(root) = std::env::var("BRIVVA_ROOT") {
        let _ = std::env::set_current_dir(&root);
    } else {
        let candidates = [
            std::env::current_dir().ok(),
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent()?.parent()?.parent()?.parent().map(|p| p.to_path_buf())),
        ];
        for candidate in candidates.into_iter().flatten() {
            if candidate.join(".env.local").exists() || candidate.join(".env").exists() {
                let _ = std::env::set_current_dir(&candidate);
                break;
            }
        }
    }

    // Spawn the Axum backend on a dedicated thread with its own Tokio runtime
    std::thread::spawn(|| {
        let rt = tokio::runtime::Runtime::new().expect("failed to create Tokio runtime");
        rt.block_on(server_rs::run_server());
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // System tray: minimize to tray instead of quitting
            let show = MenuItem::with_id(app, "show", "Show Brivva", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Brivva")
                .menu(&menu)
                .on_menu_event(|app, event| {
                    match event.id.as_ref() {
                        "show" => {
                            if let Some(win) = app.get_webview_window("main") {
                                let _ = win.show();
                                let _ = win.set_focus();
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click { .. } = event {
                        if let Some(win) = tray.app_handle().get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            // Minimize to tray on close instead of quitting
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Brivva");
}
