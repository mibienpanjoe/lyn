//! Localhost port-to-process-cwd resolution for local web dev-server context binding.

use std::path::PathBuf;

/// Parses a URL string and extracts the port if the host is localhost (or 127.0.0.1 / ::1).
pub(crate) fn parse_localhost_port(url_str: &str) -> Option<u16> {
    let trimmed = url_str.trim();
    let without_scheme = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))?;

    // Host and optional port end at the first '/', '?', or '#'
    let host_and_port = without_scheme.split(['/', '?', '#']).next()?.trim();

    if let Some((host, port_str)) = host_and_port.rsplit_once(':') {
        let host = host.trim().trim_start_matches('[').trim_end_matches(']');
        if host.eq_ignore_ascii_case("localhost")
            || host == "127.0.0.1"
            || host == "::1"
            || host == "0.0.0.0"
        {
            return port_str.parse::<u16>().ok();
        }
    }

    None
}

/// Parses file:// URLs to extract local directories if applicable.
pub(crate) fn parse_file_url_path(url_str: &str) -> Option<PathBuf> {
    let trimmed = url_str.trim();
    let file_path = trimmed.strip_prefix("file://")?;
    let path = PathBuf::from(file_path);
    if path.is_dir() {
        Some(path)
    } else {
        path.parent().map(|p| p.to_path_buf())
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn find_listening_port_cwd(port: u16) -> Option<PathBuf> {
    use std::fs;

    let hex_port = format!("{port:04X}");
    let mut matching_inodes = Vec::new();

    for net_file in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(contents) = fs::read_to_string(net_file) else {
            continue;
        };
        for line in contents.lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            // fields: [sl, local_address, rem_address, st, tx_queue:rx_queue, tr:tm->when, retrnsmt, uid, timeout, inode, ...]
            if fields.len() > 9 && fields[3] == "0A" {
                // "0A" is TCP_LISTEN
                if let Some((_addr, p)) = fields[1].split_once(':') {
                    if p.eq_ignore_ascii_case(&hex_port) {
                        let inode = fields[9];
                        matching_inodes.push(format!("socket:[{inode}]"));
                    }
                }
            }
        }
    }

    if matching_inodes.is_empty() {
        return None;
    }

    let Ok(entries) = fs::read_dir("/proc") else {
        return None;
    };

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }

        let file_name = entry.file_name();
        let Some(name_str) = file_name.to_str() else {
            continue;
        };
        // Process directories are numeric PIDs
        if !name_str.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        let fd_dir = entry.path().join("fd");
        let Ok(fd_entries) = fs::read_dir(&fd_dir) else {
            continue;
        };

        for fd_entry in fd_entries.flatten() {
            if let Ok(link_target) = fs::read_link(fd_entry.path()) {
                let target_str = link_target.to_string_lossy();
                if matching_inodes.iter().any(|inode| target_str == *inode) {
                    let cwd_path = entry.path().join("cwd");
                    if let Ok(cwd) = fs::read_link(&cwd_path) {
                        if cwd.is_dir() {
                            return Some(cwd);
                        }
                    }
                }
            }
        }
    }

    None
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn find_listening_port_cwd(_port: u16) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_localhost_urls_accurately() {
        assert_eq!(parse_localhost_port("http://localhost:5173/"), Some(5173));
        assert_eq!(parse_localhost_port("http://localhost:3000"), Some(3000));
        assert_eq!(
            parse_localhost_port("https://127.0.0.1:8080/dashboard?auth=123"),
            Some(8080)
        );
        assert_eq!(
            parse_localhost_port("http://[::1]:9090/#section"),
            Some(9090)
        );
        assert_eq!(parse_localhost_port("http://localhost"), None);
        assert_eq!(parse_localhost_port("https://example.com:5173/"), None);
        assert_eq!(parse_localhost_port("not a url"), None);
    }

    #[test]
    fn parses_file_urls() {
        let dir = std::env::temp_dir();
        let file_url = format!("file://{}", dir.display());
        assert!(parse_file_url_path(&file_url).is_some());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn detects_local_listening_socket() {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let resolved_cwd = find_listening_port_cwd(port);
        assert!(resolved_cwd.is_some());
        assert!(resolved_cwd.unwrap().is_dir());
    }
}
