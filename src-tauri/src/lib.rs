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
        .setup(|app| {
            let database_path = app.path().app_data_dir()?.join("lyn.db");
            let database = storage::Database::open(database_path)?;
            app.manage(Mutex::new(database));
            Ok(())
        })
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
