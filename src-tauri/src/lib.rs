mod capture;
mod commands;
mod context;
mod enrichment;
mod error;
mod intelligence;
mod library;
mod media;
mod platform;
mod storage;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
