use std::sync::Mutex;

use tauri::Manager;

mod capture;
mod commands;
mod context;
pub mod contract;
mod enrichment;
pub mod error;
mod intelligence;
mod library;
mod media;
mod platform;
mod storage;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let database_path = app.path().app_data_dir()?.join("lyn.db");
            let database = storage::Database::open(database_path)?;
            app.manage(Mutex::new(database));
            app.manage(Mutex::new(context::DirectorySelectionRegistry::default()));
            app.manage(Mutex::new(
                capture::session::CaptureSessionService::default(),
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::capture::get_active_capture_session,
            commands::capture::cancel_capture_session,
            commands::capture::select_capture_context_source,
            commands::capture::save_text_capture,
            commands::context::pick_project_directory,
            commands::context::create_context,
            commands::context::list_contexts,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Lyn");
}

#[cfg(test)]
mod tests {
    #[test]
    fn rust_test_harness_is_available() {
        assert_eq!(2 + 2, 4);
    }
}
