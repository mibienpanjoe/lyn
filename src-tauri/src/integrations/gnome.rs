//! Private GNOME activation primitives. No user-session writes or restart here.

use std::path::Path;

pub(crate) mod artifact;
mod directories;
mod lock;
mod managed;
mod module_store;
mod ownership;
mod replacement;
mod service;
pub(crate) mod status;
mod transaction;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ActivationError {
    InvalidPath,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RenderedActivation {
    pub(crate) launcher: String,
    pub(crate) drop_in: String,
}

fn activation_path(path: &Path) -> Result<&str, ActivationError> {
    let text = path.to_str().ok_or(ActivationError::InvalidPath)?;
    if !path.is_absolute()
        || path.file_name().is_none()
        || text.ends_with('/')
        || text.len() > 4095
        || text
            .chars()
            .any(|character| character == ':' || character.is_control())
        || text
            .split('/')
            .any(|component| matches!(component, "." | ".."))
    {
        return Err(ActivationError::InvalidPath);
    }
    Ok(text)
}

pub(crate) fn render_activation(
    module: &Path,
    launcher: &Path,
) -> Result<RenderedActivation, ActivationError> {
    let module = activation_path(module)?;
    let launcher = activation_path(launcher)?;
    let module_literal = format!("'{}'", module.replace('\'', "'\\''"));
    let launcher_literal = launcher
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    Ok(RenderedActivation {
        launcher: format!(
            "#!/bin/sh\n# Managed by Lyn native GNOME v1\nlyn_module={module_literal}\n\
             if [ -n \"${{GTK3_MODULES-}}\" ]; then\n\
             export GTK3_MODULES=\"$GTK3_MODULES:$lyn_module\"\n\
             else\nexport GTK3_MODULES=\"$lyn_module\"\nfi\n\
             exec /usr/libexec/gnome-terminal-server \"$@\"\n"
        ),
        // ':' disables environment expansion; '%%' is still needed for unit
        // specifiers. Keep the executable one quoted literal argument.
        // https://www.freedesktop.org/software/systemd/man/systemd.service.html#Command%20lines
        drop_in: format!(
            "# Managed by Lyn native GNOME v1\n[Service]\nExecStart=\nExecStart=:\"{launcher_literal}\"\n"
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::OsString, os::unix::ffi::OsStringExt, path::Path};

    fn private_managed_bus_address(address: &str, bus: &Path) -> bool {
        let Some(path) = bus.to_str() else {
            return false;
        };
        if !path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/-_.".contains(&byte))
        {
            return false;
        }
        let mut fields = address.split(',');
        if fields.next() != Some(format!("unix:path={path}").as_str()) {
            return false;
        }
        match (fields.next(), fields.next()) {
            (None, None) => true,
            (Some(guid), None) => guid.strip_prefix("guid=").is_some_and(|guid| {
                guid.len() == 32 && guid.bytes().all(|byte| byte.is_ascii_hexdigit())
            }),
            _ => false,
        }
    }

    #[test]
    fn managed_fixture_bus_does_not_accept_extra_address_fields_or_encoded_paths() {
        let bus = Path::new("/tmp/lyn-gnome-run-test/runtime/session-bus");
        let address = format!("unix:path={}", bus.display());
        assert!(private_managed_bus_address(&address, bus));
        assert!(private_managed_bus_address(
            &format!("{address},guid={}", "a".repeat(32)),
            bus
        ));
        for suffix in [
            "-other",
            ",path=/run/user/1000/bus",
            ",abstract=foreign",
            ",guid=bad",
            ";unix:path=/other",
            "/../other",
        ] {
            assert!(
                !private_managed_bus_address(&format!("{address}{suffix}"), bus),
                "{suffix}"
            );
        }
        let encoded = Path::new("/tmp/lyn-gnome-run-%2f..%2f/runtime/session-bus");
        assert!(!private_managed_bus_address(
            &format!("unix:path={}", encoded.display()),
            encoded
        ));
    }

    #[test]
    fn native_activation_fixture_entry() {
        use std::{fs, io::Read, os::unix::fs::MetadataExt};
        let Some(root) = std::env::var_os("LYN_PRIVATE_NATIVE_ACTIVATION_ROOT") else {
            return;
        };
        let root = Path::new(&root);
        let metadata = fs::symlink_metadata(root).unwrap();
        assert!(root.is_absolute() && root.parent() == Some(std::env::temp_dir().as_path()));
        assert!(
            root.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("lyn-na-")
        );
        assert!(
            metadata.is_dir()
                && metadata.uid() == unsafe { libc::geteuid() }
                && metadata.mode() & 0o7777 == 0o700
        );
        assert_eq!(root.canonicalize().unwrap(), root);
        let module = std::env::var_os("LYN_PRIVATE_NATIVE_ACTIVATION_MODULE").unwrap();
        let module = Path::new(&module);
        assert!(module.is_absolute() && module.is_file());
        let state_path = root.join("state");
        let state = directories::open_private(&state_path, true).unwrap();
        let target =
            directories::open_drop_ins(&root.join("units/gnome-terminal-server.service.d"), true)
                .unwrap();
        // Test-only bundle path comes from the guarded controller. Production
        // obtains these same validated bytes only from artifact::embedded().
        let mut bytes = Vec::new();
        fs::File::open(module)
            .unwrap()
            .take((artifact::format::MAX_MODULE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .unwrap();
        let mut manifest = Vec::new();
        fs::File::open(
            module
                .parent()
                .unwrap()
                .join(artifact::format::MANIFEST_NAME),
        )
        .unwrap()
        .take((artifact::format::MAX_MANIFEST_BYTES + 1) as u64)
        .read_to_end(&mut manifest)
        .unwrap();
        let artifact = artifact::Artifact {
            manifest: artifact::format::validate(&manifest, &bytes).unwrap(),
            bytes: bytes.leak(),
        };
        let modules_path = state_path.join("modules");
        let modules = directories::open_private(&modules_path, true).unwrap();
        let store = module_store::ModuleStore::open(&modules).unwrap();
        let stored = store.retain(&artifact).unwrap();
        let module_path = modules_path
            .join(&artifact.manifest.sha256)
            .join(artifact::format::MODULE_NAME);
        store.verify(&stored, &artifact).unwrap();
        let rendered = render_activation(&module_path, &state_path.join("launcher-v1")).unwrap();
        let mut transaction =
            transaction::ActivationTransaction::begin(&state, &target, rendered).unwrap();
        transaction.apply().unwrap();
        assert_eq!(
            transaction::inspect_activation_files(Some(&state), Some(&target)),
            transaction::ActivationFilesState::Owned
        );
    }

    #[test]
    fn native_managed_service_fixture_entry() {
        use std::{
            fs,
            os::unix::fs::{FileTypeExt, MetadataExt},
        };
        let Some(root) = std::env::var_os("LYN_PRIVATE_NATIVE_ACTIVATION_ROOT") else {
            return;
        };
        let root = Path::new(&root);
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").unwrap();
        let runtime = Path::new(&runtime);
        for (path, prefix) in [
            (root, "lyn-na-"),
            (runtime.parent().unwrap(), "lyn-gnome-run-"),
        ] {
            assert_eq!(path.parent(), Some(std::env::temp_dir().as_path()));
            assert!(
                path.file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with(prefix)
            );
            let metadata = fs::symlink_metadata(path).unwrap();
            assert!(
                metadata.is_dir()
                    && metadata.uid() == unsafe { libc::geteuid() }
                    && metadata.mode() & 0o7777 == 0o700
            );
            assert_eq!(path.canonicalize().unwrap(), path);
        }
        assert_eq!(runtime.file_name().unwrap(), "runtime");
        let metadata = fs::symlink_metadata(runtime).unwrap();
        assert!(
            metadata.is_dir()
                && metadata.uid() == unsafe { libc::geteuid() }
                && metadata.mode() & 0o7777 == 0o700
        );
        assert_eq!(runtime.canonicalize().unwrap(), runtime);
        let bus = runtime.join("session-bus");
        let metadata = fs::symlink_metadata(&bus).unwrap();
        assert!(metadata.file_type().is_socket() && metadata.uid() == unsafe { libc::geteuid() });
        let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
        assert!(private_managed_bus_address(&address, &bus));
        let owned = service::OwnedActivation::open(
            &root.join("state"),
            &root.join("units/gnome-terminal-server.service.d"),
        )
        .unwrap();
        let result = service::inspect_owned_service(&owned);
        let expected = std::env::var("LYN_PRIVATE_MANAGED_DEFINITION").unwrap();
        assert_eq!(
            result,
            match expected.as_str() {
                "loaded" => Ok(service::DefinitionState::ManagedLoaded),
                "pending" => Ok(service::DefinitionState::NeedsReload),
                "changing" => Err(service::ServiceRefusal::Changed),
                _ => panic!("unknown private manager mode"),
            }
        );
    }

    #[test]
    fn wrapper_only_appends_the_module_and_execs_the_fixed_server() {
        let plan = render_activation(
            Path::new("/tmp/lyn space/native's.so"),
            Path::new("/tmp/lyn space/launch"),
        )
        .unwrap();
        assert!(plan.launcher.starts_with("#!/bin/sh\n"));
        assert!(
            plan.launcher
                .contains("lyn_module='/tmp/lyn space/native'\\''s.so'")
        );
        assert!(
            plan.launcher
                .contains("GTK3_MODULES=\"$GTK3_MODULES:$lyn_module\"")
        );
        assert!(
            plan.launcher
                .ends_with("exec /usr/libexec/gnome-terminal-server \"$@\"\n")
        );
        assert!(!plan.launcher.contains("GTK_MODULES="));
        assert!(!plan.launcher.contains("systemctl"));
    }

    #[test]
    fn systemd_exec_path_is_one_literal_argument_not_expansion() {
        let plan = render_activation(
            Path::new("/tmp/module.so"),
            Path::new("/tmp/lyn %n $HOME/quote\"\\launch"),
        )
        .unwrap();
        assert_eq!(
            plan.drop_in,
            concat!(
                "# Managed by Lyn native GNOME v1\n[Service]\nExecStart=\n",
                "ExecStart=:\"/tmp/lyn %%n $HOME/quote\\\"\\\\launch\"\n"
            )
        );
    }

    #[test]
    fn unrepresentable_paths_refuse_before_any_activation_is_rendered() {
        for path in [
            "relative",
            "/tmp/../module",
            "/tmp/a:b",
            "/tmp/a\nb",
            "/tmp/a\rb",
            "/tmp/a\0b",
        ] {
            assert_eq!(
                render_activation(Path::new(path), Path::new("/tmp/launch")),
                Err(ActivationError::InvalidPath)
            );
            assert_eq!(
                render_activation(Path::new("/tmp/module.so"), Path::new(path)),
                Err(ActivationError::InvalidPath)
            );
        }
        let non_utf8 = OsString::from_vec(b"/tmp/\xff".to_vec());
        assert_eq!(
            render_activation(Path::new(&non_utf8), Path::new("/tmp/launch")),
            Err(ActivationError::InvalidPath)
        );
    }
}
