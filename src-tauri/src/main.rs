#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    {
        let argv0 = std::env::args_os().next();
        let argv1 = std::env::args_os().nth(1);
        match lyn_lib::linux_helper_mode(argv0.as_deref(), argv1.as_deref()) {
            Some(lyn_lib::LinuxHelperMode::BrowserHost) => {
                std::process::exit(match lyn_lib::run_browser_host_helper() {
                    code if code == std::process::ExitCode::SUCCESS => 0,
                    _ => 1,
                });
            }
            Some(lyn_lib::LinuxHelperMode::ShellWatch) => {
                std::process::exit(match lyn_lib::run_shell_context_helper() {
                    code if code == std::process::ExitCode::SUCCESS => 0,
                    _ => 1,
                });
            }
            None => {}
        }
    }
    lyn_lib::run();
}
