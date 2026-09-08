#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    if std::env::args_os()
        .nth(1)
        .is_some_and(|argument| argument == "watch")
    {
        std::process::exit(match lyn_lib::run_shell_context_helper() {
            code if code == std::process::ExitCode::SUCCESS => 0,
            _ => 1,
        });
    }
    lyn_lib::run();
}
