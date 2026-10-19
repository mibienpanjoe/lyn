//! Real desktop fixture only. No production command, mode or auth exception.

use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::PathBuf,
};

fn private_xdg_root(paths: &[PathBuf; 4]) -> Option<PathBuf> {
    let root = paths[0].parent()?;
    if !root.file_name()?.to_str()?.starts_with("lyn-gnome-run-") {
        return None;
    }
    for path in paths
        .iter()
        .map(PathBuf::as_path)
        .chain(std::iter::once(root))
    {
        let metadata = fs::symlink_metadata(path).ok()?;
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
        {
            return None;
        }
    }
    paths
        .iter()
        .all(|path| path.is_absolute() && path.parent() == Some(root))
        .then(|| root.to_path_buf())
}

#[test]
fn desktop_fixture_requires_one_private_owned_xdg_root() {
    let directory = tempfile::Builder::new()
        .prefix("lyn-gnome-run-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let paths = ["runtime", "config", "data", "cache"].map(|name| directory.path().join(name));
    for path in &paths {
        fs::DirBuilder::new().mode(0o700).create(path).unwrap();
    }
    assert_eq!(
        private_xdg_root(&paths),
        Some(directory.path().to_path_buf())
    );
    let mut mixed = paths.clone();
    mixed[1] = PathBuf::from("/tmp");
    assert!(private_xdg_root(&mixed).is_none());
    fs::set_permissions(&paths[1], fs::Permissions::from_mode(0o755)).unwrap();
    assert!(private_xdg_root(&paths).is_none());
    fs::set_permissions(&paths[1], fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(private_xdg_root(&paths).is_none());
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    fs::remove_dir(&paths[1]).unwrap();
    std::os::unix::fs::symlink(&paths[2], &paths[1]).unwrap();
    assert!(private_xdg_root(&paths).is_none());
}

#[test]
fn real_gnome_desktop_entry() {
    if std::env::var_os("LYN_GNOME_UI_FIXTURE").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let paths = [
        "XDG_RUNTIME_DIR",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
    ]
    .map(|name| PathBuf::from(std::env::var_os(name).expect("private XDG required")));
    let root = private_xdg_root(&paths)
        .expect("desktop fixture must run on isolated_run.py's private XDG root");
    let helper_root = root.join("fixture-helper-root");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&helper_root)
        .unwrap();
    let port: u16 = std::env::var("LYN_GNOME_UI_PORT").unwrap().parse().unwrap();
    assert_ne!(port, 0);
    let mut context = tauri::generate_context!();
    // Tauri 2.11.5 Context::config_mut / Builder::any_thread, checked against
    // the pinned crate's source; this localhost URL exists only in cfg(test).
    context.config_mut().build.dev_url = Some(format!("http://127.0.0.1:{port}").parse().unwrap());
    super::desktop_builder(Some(helper_root))
        .any_thread()
        .run(context)
        .unwrap();
}
