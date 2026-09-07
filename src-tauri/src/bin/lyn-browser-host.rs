#[cfg(target_os = "linux")]
fn main() -> std::process::ExitCode {
    lyn_lib::run_browser_host_helper()
}

#[cfg(not(target_os = "linux"))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::FAILURE
}
