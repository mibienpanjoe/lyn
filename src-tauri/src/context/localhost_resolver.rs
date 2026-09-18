//! Localhost port-to-process-cwd resolution for local web dev-server context binding.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

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

pub(crate) fn parse_listen_inodes(tcp_table: &str, port: u16) -> Vec<String> {
    let hex_port = format!("{port:04X}");
    let mut matching_inodes = Vec::new();
    for line in tcp_table.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // fields: [sl, local_address, rem_address, st, tx_queue:rx_queue, tr:tm->when, retrnsmt, uid, timeout, inode, ...]
        if fields.len() > 9 && fields[3] == "0A" {
            // "0A" is TCP_LISTEN
            if let Some((_addr, p)) = fields[1].split_once(':')
                && p.eq_ignore_ascii_case(&hex_port)
            {
                matching_inodes.push(format!("socket:[{}]", fields[9]));
            }
        }
    }
    matching_inodes
}

pub(crate) fn unique_directory(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    let mut unique = HashSet::new();
    for path in paths {
        unique.insert(path.canonicalize().unwrap_or(path));
    }
    if unique.len() == 1 {
        unique.into_iter().next()
    } else {
        None
    }
}

pub(crate) fn skip_listener_comm(comm: &str) -> bool {
    matches!(comm.trim(), "docker-proxy")
}

#[cfg(target_os = "linux")]
pub(crate) fn find_listening_port_cwd(port: u16) -> Option<PathBuf> {
    find_listening_port_cwd_at(port, Path::new("/proc"), current_uid())
}

#[cfg(target_os = "linux")]
fn current_uid() -> u32 {
    unsafe { libc::getuid() }
}

#[cfg(target_os = "linux")]
pub(crate) fn find_listening_port_cwd_at(
    port: u16,
    proc_root: &Path,
    expected_uid: u32,
) -> Option<PathBuf> {
    use std::fs;
    use std::os::unix::fs::MetadataExt;

    let mut matching_inodes = Vec::new();
    for name in ["tcp", "tcp6"] {
        let Ok(contents) = fs::read_to_string(proc_root.join("net").join(name)) else {
            continue;
        };
        matching_inodes.extend(parse_listen_inodes(&contents, port));
    }

    if matching_inodes.is_empty() {
        return None;
    }

    let Ok(entries) = fs::read_dir(proc_root) else {
        return None;
    };

    let mut cwds = Vec::new();
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

        let process = entry.path();
        let Ok(metadata) = fs::metadata(&process) else {
            continue;
        };
        if metadata.uid() != expected_uid {
            continue;
        }

        let comm = fs::read_to_string(process.join("comm")).unwrap_or_default();
        if skip_listener_comm(&comm) {
            continue;
        }

        let fd_dir = process.join("fd");
        let Ok(fd_entries) = fs::read_dir(&fd_dir) else {
            continue;
        };

        let mut owns_socket = false;
        for fd_entry in fd_entries.flatten() {
            if let Ok(link_target) = fs::read_link(fd_entry.path()) {
                let target_str = link_target.to_string_lossy();
                if matching_inodes.iter().any(|inode| target_str == *inode) {
                    owns_socket = true;
                    break;
                }
            }
        }
        if !owns_socket {
            continue;
        }

        if let Ok(cwd) = fs::read_link(process.join("cwd"))
            && cwd.is_dir()
        {
            cwds.push(cwd);
        }
    }

    unique_directory(cwds)
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
    fn listen_table_matches_port_on_ipv4_and_ipv6() {
        let ipv4 = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 0100007F:1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 4242 1 0000000000000000 100 0 0 10 0\n   1: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 99 1 0000000000000000 100 0 0 10 0\n";
        let ipv6 = "  sl  local_address                         rem_address                       st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000000000000000000001000000:1435 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 4242 1 0000000000000000 100 0 0 10 0\n";
        assert_eq!(
            parse_listen_inodes(ipv4, 5173),
            vec!["socket:[4242]".to_string()]
        );
        assert_eq!(
            parse_listen_inodes(ipv6, 5173),
            vec!["socket:[4242]".to_string()]
        );
        assert!(parse_listen_inodes(ipv4, 3000).is_empty());
    }

    #[test]
    fn unique_directory_requires_one_path() {
        let dir = std::env::temp_dir();
        assert_eq!(unique_directory(std::iter::empty::<PathBuf>()), None);
        assert_eq!(
            unique_directory([dir.clone(), dir.clone()]).as_deref(),
            dir.canonicalize().ok().as_deref().or(Some(dir.as_path()))
        );
        assert_eq!(
            unique_directory([dir.clone(), PathBuf::from("/tmp/lyn-missing-a")]),
            None
        );
    }

    #[test]
    fn docker_proxy_listeners_are_not_project_evidence() {
        assert!(skip_listener_comm("docker-proxy\n"));
        assert!(!skip_listener_comm("node\n"));
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

    #[test]
    #[cfg(target_os = "linux")]
    fn two_listener_cwds_are_ambiguous() {
        use std::fs;
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let proc_root = root.path().join("proc");
        let proj_a = root.path().join("alpha");
        let proj_b = root.path().join("beta");
        fs::create_dir(&proj_a).unwrap();
        fs::create_dir(&proj_b).unwrap();
        fs::create_dir_all(proc_root.join("net")).unwrap();
        let table = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 0100007F:1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 11 1 0000000000000000 100 0 0 10 0\n   1: 0100007F:1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 22 1 0000000000000000 100 0 0 10 0\n";
        fs::write(proc_root.join("net/tcp"), table).unwrap();
        fs::write(proc_root.join("net/tcp6"), "  sl\n").unwrap();

        for (pid, inode, cwd, comm) in [
            ("1001", "11", &proj_a, "node\n"),
            ("1002", "22", &proj_b, "node\n"),
        ] {
            let process = proc_root.join(pid);
            fs::create_dir_all(process.join("fd")).unwrap();
            symlink(cwd, process.join("cwd")).unwrap();
            symlink(format!("socket:[{inode}]"), process.join("fd/3")).unwrap();
            fs::write(process.join("comm"), comm).unwrap();
        }

        assert_eq!(
            find_listening_port_cwd_at(5173, &proc_root, current_uid()),
            None
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn ipv4_and_ipv6_same_cwd_is_unique() {
        use std::fs;
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let proc_root = root.path().join("proc");
        let proj = root.path().join("app");
        fs::create_dir(&proj).unwrap();
        fs::create_dir_all(proc_root.join("net")).unwrap();
        fs::write(
            proc_root.join("net/tcp"),
            "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 0100007F:1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 11 1 0000000000000000 100 0 0 10 0\n",
        )
        .unwrap();
        fs::write(
            proc_root.join("net/tcp6"),
            "  sl  local_address                         rem_address                       st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000000000000000000001000000:1435 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 22 1 0000000000000000 100 0 0 10 0\n",
        )
        .unwrap();

        let process = proc_root.join("1001");
        fs::create_dir_all(process.join("fd")).unwrap();
        symlink(&proj, process.join("cwd")).unwrap();
        symlink("socket:[11]", process.join("fd/3")).unwrap();
        symlink("socket:[22]", process.join("fd/4")).unwrap();
        fs::write(process.join("comm"), "node\n").unwrap();

        let resolved = find_listening_port_cwd_at(5173, &proc_root, current_uid()).unwrap();
        assert_eq!(resolved, proj.canonicalize().unwrap());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn docker_proxy_is_ignored_when_the_app_cwd_is_unique() {
        use std::fs;
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let proc_root = root.path().join("proc");
        let proj = root.path().join("app");
        let docker = root.path().join("docker");
        fs::create_dir(&proj).unwrap();
        fs::create_dir(&docker).unwrap();
        fs::create_dir_all(proc_root.join("net")).unwrap();
        fs::write(
            proc_root.join("net/tcp"),
            "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 0100007F:1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 11 1 0000000000000000 100 0 0 10 0\n   1: 0100007F:1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 22 1 0000000000000000 100 0 0 10 0\n",
        )
        .unwrap();
        fs::write(proc_root.join("net/tcp6"), "  sl\n").unwrap();

        let proxy = proc_root.join("9");
        fs::create_dir_all(proxy.join("fd")).unwrap();
        symlink(&docker, proxy.join("cwd")).unwrap();
        symlink("socket:[11]", proxy.join("fd/3")).unwrap();
        fs::write(proxy.join("comm"), "docker-proxy\n").unwrap();

        let app = proc_root.join("1001");
        fs::create_dir_all(app.join("fd")).unwrap();
        symlink(&proj, app.join("cwd")).unwrap();
        symlink("socket:[22]", app.join("fd/3")).unwrap();
        fs::write(app.join("comm"), "node\n").unwrap();

        let resolved = find_listening_port_cwd_at(5173, &proc_root, current_uid()).unwrap();
        assert_eq!(resolved, proj.canonicalize().unwrap());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn docker_proxy_alone_is_not_resolved() {
        use std::fs;
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let proc_root = root.path().join("proc");
        let docker = root.path().join("docker");
        fs::create_dir(&docker).unwrap();
        fs::create_dir_all(proc_root.join("net")).unwrap();
        fs::write(
            proc_root.join("net/tcp"),
            "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 0100007F:1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 11 1 0000000000000000 100 0 0 10 0\n",
        )
        .unwrap();
        fs::write(proc_root.join("net/tcp6"), "  sl\n").unwrap();

        let proxy = proc_root.join("9");
        fs::create_dir_all(proxy.join("fd")).unwrap();
        symlink(&docker, proxy.join("cwd")).unwrap();
        symlink("socket:[11]", proxy.join("fd/3")).unwrap();
        fs::write(proxy.join("comm"), "docker-proxy\n").unwrap();

        assert_eq!(
            find_listening_port_cwd_at(5173, &proc_root, current_uid()),
            None
        );
    }
}
