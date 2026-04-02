// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Set cwd to the brivva project root so .env.local is found
    // In dev: the project root is one level up from src-tauri
    // In production: use BRIVVA_ROOT env var or fall back to the resource dir
    if let Ok(root) = std::env::var("BRIVVA_ROOT") {
        let _ = std::env::set_current_dir(&root);
    } else {
        // Try common dev paths: parent of src-tauri, or current dir
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
        .run(tauri::generate_context!())
        .expect("error while running Brivva");
}
