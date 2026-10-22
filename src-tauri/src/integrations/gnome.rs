//! Private GNOME activation primitives. No user-session writes or restart here.

use std::path::Path;

mod directories;
mod lock;
mod managed;
mod ownership;
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
