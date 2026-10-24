//! Read-only compatibility and explicit guarded definition synchronization.
//! Never starts, stops or restarts a service.

mod overrides;
mod owned;
pub(super) use owned::OwnedActivation;

use gio::{
    glib::{self, variant::ToVariant},
    prelude::*,
};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const SERVER: &str = "/usr/libexec/gnome-terminal-server";
const UNIT: &str = "gnome-terminal-server.service";
const BUS: &str = "org.gnome.Terminal";
const MANAGER: &str = "org.freedesktop.systemd1";
const UNIT_IFACE: &str = "org.freedesktop.systemd1.Unit";
const SERVICE_IFACE: &str = "org.freedesktop.systemd1.Service";
const PROPERTIES: [(&str, &str); 12] = [
    (UNIT_IFACE, "LoadState"),
    (UNIT_IFACE, "FragmentPath"),
    (UNIT_IFACE, "DropInPaths"),
    (UNIT_IFACE, "NeedDaemonReload"),
    (UNIT_IFACE, "Transient"),
    (UNIT_IFACE, "Names"),
    (SERVICE_IFACE, "Type"),
    (SERVICE_IFACE, "BusName"),
    (SERVICE_IFACE, "ExecStart"),
    (SERVICE_IFACE, "ExecStartPre"),
    (SERVICE_IFACE, "ExecStartPost"),
    (SERVICE_IFACE, "ExecCondition"),
];
type Properties = BTreeMap<&'static str, glib::Variant>;
type Exec = (String, Vec<String>, bool, u64, u64, u64, u64, u32, i32, i32);

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ServiceRefusal {
    Unavailable,
    Unsupported,
    Changed,
}
type Result<T> = std::result::Result<T, ServiceRefusal>;

fn value<T: glib::variant::FromVariant>(properties: &Properties, name: &str) -> Result<T> {
    properties
        .get(name)
        .and_then(|value| value.get())
        .ok_or(ServiceRefusal::Unsupported)
}

fn policy(properties: &Properties) -> Result<String> {
    policy_for(properties, None).map(|(fragment, _)| fragment)
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum DefinitionState {
    Stock,
    NeedsReload,
    ManagedLoaded,
}

fn policy_for(
    properties: &Properties,
    owned_paths: Option<(&str, &str)>,
) -> Result<(String, DefinitionState)> {
    let fragment: String = value(properties, "FragmentPath")?;
    let starts: Vec<Exec> = value(properties, "ExecStart")?;
    if value::<String>(properties, "LoadState")? != "loaded"
        || ![
            "/usr/lib/systemd/user/gnome-terminal-server.service",
            "/lib/systemd/user/gnome-terminal-server.service",
        ]
        .contains(&fragment.as_str())
        || value::<bool>(properties, "Transient")?
        || value::<Vec<String>>(properties, "Names")? != [UNIT]
        || value::<String>(properties, "Type")? != "dbus"
        || value::<String>(properties, "BusName")? != BUS
        || starts.len() != 1
        || starts[0].2
        || !value::<Vec<Exec>>(properties, "ExecStartPre")?.is_empty()
        || !value::<Vec<Exec>>(properties, "ExecStartPost")?.is_empty()
    {
        return Err(ServiceRefusal::Unsupported);
    }
    if !value::<Vec<Exec>>(properties, "ExecCondition")?.is_empty() {
        return Err(ServiceRefusal::Unsupported);
    }
    let reload: bool = value(properties, "NeedDaemonReload")?;
    let drop_ins: Vec<String> = value(properties, "DropInPaths")?;
    let stock = starts[0].0 == SERVER && starts[0].1 == [SERVER] && drop_ins.is_empty();
    let state = match owned_paths {
        None if stock && !reload => DefinitionState::Stock,
        Some(_) if stock => DefinitionState::NeedsReload,
        Some((launcher, drop_in))
            if starts[0].0 == launcher && starts[0].1 == [launcher] && drop_ins == [drop_in] =>
        {
            if reload {
                DefinitionState::NeedsReload
            } else {
                DefinitionState::ManagedLoaded
            }
        }
        _ => return Err(ServiceRefusal::Unsupported),
    };
    Ok((fragment, state))
}

fn retired_policy(
    properties: &Properties,
    paths: (&str, &str),
) -> Result<(String, DefinitionState)> {
    let (fragment, state) = policy_for(properties, Some(paths))?;
    let starts: Vec<Exec> = value(properties, "ExecStart")?;
    if starts[0].0 == SERVER
        && starts[0].1 == [SERVER]
        && value::<Vec<String>>(properties, "DropInPaths")?.is_empty()
        && !value::<bool>(properties, "NeedDaemonReload")?
    {
        return Ok((fragment, DefinitionState::Stock));
    }
    Ok((fragment, state))
}

struct Budget {
    deadline: Instant,
    cancel: gio::Cancellable,
    done: Option<mpsc::Sender<()>>,
    watchdog: Option<thread::JoinHandle<()>>,
}
impl Budget {
    fn new() -> Result<Self> {
        Self::with_duration(Duration::from_secs(1))
    }
    fn with_duration(duration: Duration) -> Result<Self> {
        let deadline = Instant::now() + duration;
        let cancel = gio::Cancellable::new();
        let watched = cancel.clone();
        let (done, receiver) = mpsc::channel();
        let watchdog = thread::Builder::new()
            .name("lyn-service-probe".into())
            .spawn(move || {
                if matches!(
                    receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    watched.cancel();
                }
            })
            .map_err(|_| ServiceRefusal::Unavailable)?;
        Ok(Self {
            deadline,
            cancel,
            done: Some(done),
            watchdog: Some(watchdog),
        })
    }
    fn remaining(&self) -> Result<i32> {
        self.remaining_cap(500)
    }
    fn remaining_cap(&self, cap: u128) -> Result<i32> {
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or(ServiceRefusal::Unavailable)?;
        Ok(remaining.as_millis().clamp(1, cap) as i32)
    }
}
impl Drop for Budget {
    fn drop(&mut self) {
        self.done.take();
        if let Some(watchdog) = self.watchdog.take() {
            let _ = watchdog.join();
        }
    }
}

fn call(
    connection: &gio::DBusConnection,
    budget: &Budget,
    destination: &str,
    path: &str,
    interface: &str,
    method: &str,
    parameters: glib::Variant,
) -> Result<glib::Variant> {
    call_capped(
        connection,
        budget,
        destination,
        path,
        interface,
        method,
        parameters,
        500,
    )
}

fn call_capped(
    connection: &gio::DBusConnection,
    budget: &Budget,
    destination: &str,
    path: &str,
    interface: &str,
    method: &str,
    parameters: glib::Variant,
    cap: u128,
) -> Result<glib::Variant> {
    let reply = connection
        .call_sync(
            Some(destination),
            path,
            interface,
            method,
            Some(&parameters),
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            budget.remaining_cap(cap)?,
            Some(&budget.cancel),
        )
        .map_err(|_| ServiceRefusal::Unavailable)?;
    budget.remaining()?;
    if reply.size() > 16384 {
        return Err(ServiceRefusal::Unsupported);
    }
    Ok(reply)
}

fn owner(connection: &gio::DBusConnection, budget: &Budget) -> Result<String> {
    let reply = call(
        connection,
        budget,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "GetNameOwner",
        (MANAGER,).to_variant(),
    )?;
    let (name,) = reply
        .get::<(String,)>()
        .ok_or(ServiceRefusal::Unsupported)?;
    if !name.starts_with(':') || name.len() > 255 {
        return Err(ServiceRefusal::Unsupported);
    }
    Ok(name)
}

fn properties(
    connection: &gio::DBusConnection,
    budget: &Budget,
    owner: &str,
    path: &str,
) -> Result<Properties> {
    let mut properties = BTreeMap::new();
    for (interface, name) in PROPERTIES {
        let reply = call(
            connection,
            budget,
            owner,
            path,
            "org.freedesktop.DBus.Properties",
            "Get",
            (interface, name).to_variant(),
        )?;
        let (value,) = reply
            .get::<(glib::Variant,)>()
            .ok_or(ServiceRefusal::Unsupported)?;
        properties.insert(name, value);
    }
    Ok(properties)
}

fn unit_paths(
    connection: &gio::DBusConnection,
    budget: &Budget,
    owner: &str,
) -> Result<Vec<String>> {
    let reply = call(
        connection,
        budget,
        owner,
        "/org/freedesktop/systemd1",
        "org.freedesktop.DBus.Properties",
        "Get",
        ("org.freedesktop.systemd1.Manager", "UnitPath").to_variant(),
    )?;
    let (value,) = reply
        .get::<(glib::Variant,)>()
        .ok_or(ServiceRefusal::Unsupported)?;
    value
        .get::<Vec<String>>()
        .ok_or(ServiceRefusal::Unsupported)
}

struct TrustedFile {
    file: File,
    path: &'static str,
    metadata: fs::Metadata,
}
impl TrustedFile {
    fn open(path: &'static str) -> Result<Self> {
        let canonical = fs::canonicalize(path).map_err(|_| ServiceRefusal::Unsupported)?;
        for parent in canonical.ancestors().skip(1) {
            let metadata = fs::metadata(parent).map_err(|_| ServiceRefusal::Unsupported)?;
            if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
                return Err(ServiceRefusal::Unsupported);
            }
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&canonical)
            .map_err(|_| ServiceRefusal::Unsupported)?;
        let metadata = file.metadata().map_err(|_| ServiceRefusal::Unsupported)?;
        if !metadata.is_file()
            || metadata.uid() != 0
            || metadata.mode() & 0o022 != 0
            || metadata.nlink() != 1
        {
            return Err(ServiceRefusal::Unsupported);
        }
        let result = Self {
            file,
            path,
            metadata,
        };
        result.recheck()?;
        Ok(result)
    }
    fn recheck(&self) -> Result<()> {
        for metadata in [self.file.metadata(), fs::metadata(self.path)] {
            let metadata = metadata.map_err(|_| ServiceRefusal::Changed)?;
            if (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
                metadata.len(),
                metadata.mode(),
                metadata.uid(),
                metadata.nlink(),
            ) != (
                self.metadata.dev(),
                self.metadata.ino(),
                self.metadata.ctime(),
                self.metadata.ctime_nsec(),
                self.metadata.len(),
                self.metadata.mode(),
                self.metadata.uid(),
                self.metadata.nlink(),
            ) {
                return Err(ServiceRefusal::Changed);
            }
        }
        Ok(())
    }
}

fn activation_agrees(text: &str) -> bool {
    let mut fields = BTreeMap::new();
    let mut section = false;
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        if line == "[D-BUS Service]" && !section {
            section = true;
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            return false;
        };
        if !section || fields.insert(name.trim(), value.trim()).is_some() {
            return false;
        }
    }
    fields == BTreeMap::from([("Name", BUS), ("SystemdService", UNIT), ("Exec", SERVER)])
}

/// Point-in-time compatibility of the unmodified reference service, not an
/// installation lease or proof that a module is loaded. Recheck before writes.
pub(super) fn inspect_unmodified_service() -> Result<()> {
    let address =
        std::env::var("DBUS_SESSION_BUS_ADDRESS").map_err(|_| ServiceRefusal::Unavailable)?;
    inspect_address(&address)
}

fn inspect_address(address: &str) -> Result<()> {
    inspect_address_for(address, None, None).map(|_| ())
}

fn target_is_searched(paths: &[String], target: &std::path::Path) -> bool {
    paths
        .iter()
        .filter(|root| std::path::Path::new(root).join(format!("{UNIT}.d")) == target)
        .count()
        == 1
}

pub(super) fn inspect_unmodified_service_at(target: &std::path::Path) -> Result<()> {
    let address =
        std::env::var("DBUS_SESSION_BUS_ADDRESS").map_err(|_| ServiceRefusal::Unavailable)?;
    inspect_address_for(&address, None, Some(target)).map(|_| ())
}

pub(super) fn inspect_owned_service(owned: &OwnedActivation) -> Result<DefinitionState> {
    let address =
        std::env::var("DBUS_SESSION_BUS_ADDRESS").map_err(|_| ServiceRefusal::Unavailable)?;
    inspect_address_for(
        &address,
        Some(owned),
        std::path::Path::new(owned.paths().1).parent(),
    )
}

fn inspect_address_for(
    address: &str,
    owned: Option<&OwnedActivation>,
    required_target: Option<&std::path::Path>,
) -> Result<DefinitionState> {
    if let Some(owned) = owned {
        owned.recheck().map_err(|_| ServiceRefusal::Changed)?;
    }
    let budget = Budget::new()?;
    let connection = connect(address, &budget)?;
    let manager = owner(&connection, &budget)?;
    inspect_connected(&connection, &budget, &manager, owned, required_target)
        .map(|inspection| inspection.definition)
}

fn connect(address: &str, budget: &Budget) -> Result<gio::DBusConnection> {
    if address.len() > 4096 || !address.starts_with("unix:path=/") || address.contains(';') {
        return Err(ServiceRefusal::Unavailable);
    }
    let connection = gio::DBusConnection::for_address_sync(
        address,
        gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
            | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
        None,
        Some(&budget.cancel),
    )
    .map_err(|_| ServiceRefusal::Unavailable)?;
    connection.set_exit_on_close(false);
    Ok(connection)
}

struct ServiceInspection {
    definition: DefinitionState,
    properties: Properties,
    path: glib::variant::ObjectPath,
    paths: Vec<String>,
    vendor: TrustedFile,
    executable: TrustedFile,
    activation: TrustedFile,
    disk: overrides::Snapshot,
    allowed: Option<overrides::OwnedDropIn>,
}

impl ServiceInspection {
    fn recheck_files(&self, paths: &[String], owned: &OwnedActivation) -> Result<()> {
        if self.paths != paths {
            return Err(ServiceRefusal::Changed);
        }
        self.vendor.recheck()?;
        self.executable.recheck()?;
        self.activation.recheck()?;
        self.disk.recheck_for(
            paths,
            (self.vendor.metadata.dev(), self.vendor.metadata.ino()),
            self.allowed.as_ref(),
        )?;
        owned.recheck().map_err(|_| ServiceRefusal::Changed)
    }

    fn recheck(
        &self,
        connection: &gio::DBusConnection,
        budget: &Budget,
        manager: &str,
        owned: &OwnedActivation,
    ) -> Result<()> {
        if properties(connection, budget, manager, self.path.as_str())? != self.properties
            || unit_paths(connection, budget, manager)? != self.paths
            || owner(connection, budget)? != manager
        {
            return Err(ServiceRefusal::Changed);
        }
        self.recheck_files(&self.paths, owned)
    }
}

fn inspect_connected(
    connection: &gio::DBusConnection,
    budget: &Budget,
    manager: &str,
    owned: Option<&OwnedActivation>,
    required_target: Option<&std::path::Path>,
) -> Result<ServiceInspection> {
    if owner(connection, budget)? != manager {
        return Err(ServiceRefusal::Changed);
    }
    let allowed = owned
        .map(|owned| {
            owned.recheck().map_err(|_| ServiceRefusal::Changed)?;
            let metadata = owned
                .drop_in_metadata()
                .map_err(|_| ServiceRefusal::Changed)?;
            metadata
                .map(|metadata| {
                    overrides::OwnedDropIn::pin(std::path::Path::new(owned.paths().1), &metadata)
                })
                .transpose()
        })
        .transpose()?
        .flatten();
    let reply = call(
        &connection,
        &budget,
        &manager,
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
        "LoadUnit",
        (UNIT,).to_variant(),
    )?;
    let (path,) = reply
        .get::<(glib::variant::ObjectPath,)>()
        .ok_or(ServiceRefusal::Unsupported)?;
    let first = properties(&connection, &budget, &manager, path.as_str())?;
    let (fragment, definition) = match owned {
        Some(owned) if owned.is_retired() => retired_policy(&first, owned.paths())?,
        _ => policy_for(&first, owned.map(OwnedActivation::paths))?,
    };
    let fragment = match fragment.as_str() {
        "/usr/lib/systemd/user/gnome-terminal-server.service" => {
            "/usr/lib/systemd/user/gnome-terminal-server.service"
        }
        "/lib/systemd/user/gnome-terminal-server.service" => {
            "/lib/systemd/user/gnome-terminal-server.service"
        }
        _ => return Err(ServiceRefusal::Unsupported),
    };
    let vendor = TrustedFile::open(fragment)?;
    let paths = unit_paths(&connection, &budget, &manager)?;
    if required_target.is_some_and(|target| !target_is_searched(&paths, target)) {
        return Err(ServiceRefusal::Unsupported);
    }
    let vendor_inode = (vendor.metadata.dev(), vendor.metadata.ino());
    let disk = overrides::inspect_for(&paths, vendor_inode, allowed.as_ref())?;
    let executable = TrustedFile::open(SERVER)?;
    if executable.metadata.mode() & 0o111 == 0 || executable.metadata.mode() & 0o6000 != 0 {
        return Err(ServiceRefusal::Unsupported);
    }
    let mut activation =
        TrustedFile::open("/usr/share/dbus-1/services/org.gnome.Terminal.service")?;
    let mut text = String::new();
    (&mut activation.file)
        .take(16385)
        .read_to_string(&mut text)
        .map_err(|_| ServiceRefusal::Unsupported)?;
    if text.len() > 16384 || !activation_agrees(&text) {
        return Err(ServiceRefusal::Unsupported);
    }
    let second = properties(&connection, &budget, &manager, path.as_str())?;
    if first != second {
        #[cfg(test)]
        eprintln!(
            "GNOME service inspection changed properties: {:?}",
            first
                .keys()
                .filter(|key| first.get(*key) != second.get(*key))
                .collect::<Vec<_>>()
        );
        return Err(ServiceRefusal::Changed);
    }
    if paths != unit_paths(&connection, &budget, &manager)?
        || manager != owner(&connection, &budget)?
    {
        #[cfg(test)]
        eprintln!("GNOME service inspection changed manager owner or UnitPath");
        return Err(ServiceRefusal::Changed);
    }
    vendor.recheck()?;
    executable.recheck()?;
    activation.recheck()?;
    disk.recheck_for(&paths, vendor_inode, allowed.as_ref())?;
    if let Some(owned) = owned {
        owned.recheck().map_err(|_| ServiceRefusal::Changed)?;
    }
    budget.remaining()?;
    Ok(ServiceInspection {
        definition,
        properties: first,
        path,
        paths,
        vendor,
        executable,
        activation,
        disk,
        allowed,
    })
}

#[derive(Debug)]
pub(super) struct Synchronized {
    pub(super) definition: DefinitionState,
}

fn ordinary_manager_uid(reply: &glib::Variant, expected_uid: u32) -> Result<()> {
    let (uid,) = reply.get::<(u32,)>().ok_or(ServiceRefusal::Unsupported)?;
    if uid == 0 || uid != expected_uid {
        return Err(ServiceRefusal::Unsupported);
    }
    Ok(())
}

/// Called only by the coordinator while its installation lock/evidence is held.
pub(super) fn synchronize_owned_service(
    owned: &OwnedActivation,
    before_reload: impl FnOnce() -> Result<()>,
) -> Result<Synchronized> {
    owned.recheck().map_err(|_| ServiceRefusal::Changed)?;
    let address =
        std::env::var("DBUS_SESSION_BUS_ADDRESS").map_err(|_| ServiceRefusal::Unavailable)?;
    let budget = Budget::with_duration(Duration::from_secs(5))?;
    let connection = connect(&address, &budget)?;
    let manager = owner(&connection, &budget)?;
    let reply = call(
        &connection,
        &budget,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "GetConnectionUnixUser",
        (manager.as_str(),).to_variant(),
    )?;
    ordinary_manager_uid(&reply, unsafe { libc::geteuid() })?;
    let target = std::path::Path::new(owned.paths().1).parent();
    let before = inspect_connected(&connection, &budget, &manager, Some(owned), target)?;
    let expected = if owned.is_retired() {
        DefinitionState::Stock
    } else {
        DefinitionState::ManagedLoaded
    };
    if before.definition == expected {
        before.recheck(&connection, &budget, &manager, owned)?;
        return Ok(Synchronized {
            definition: expected,
        });
    }
    before_reload()?;
    before.recheck(&connection, &budget, &manager, owned)?;
    if owner(&connection, &budget)? != manager {
        return Err(ServiceRefusal::Changed);
    }
    let reply = call_capped(
        &connection,
        &budget,
        &manager,
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
        "Reload",
        ().to_variant(),
        2000,
    )?;
    if reply.get::<()>().is_none() {
        return Err(ServiceRefusal::Unsupported);
    }
    let after = inspect_connected(&connection, &budget, &manager, Some(owned), target)?;
    if after.definition != expected {
        return Err(ServiceRefusal::Changed);
    }
    before.recheck_files(&after.paths, owned)?;
    budget.remaining()?;
    Ok(Synchronized {
        definition: expected,
    })
}

#[cfg(test)]
pub(super) fn synchronize_changed_fixture(owned: &OwnedActivation) -> Result<Synchronized> {
    // Called only after the private native root/bus guard and with its lock.
    synchronize_owned_service(owned, || {
        let budget = Budget::new()?;
        let address =
            std::env::var("DBUS_SESSION_BUS_ADDRESS").map_err(|_| ServiceRefusal::Unavailable)?;
        let connection = connect(&address, &budget)?;
        let manager = owner(&connection, &budget)?;
        call(
            &connection,
            &budget,
            &manager,
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "MutateDefinition",
            ().to_variant(),
        )?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_uid_requires_the_bus_uint32_reply_and_same_nonroot_user() {
        assert_eq!(ordinary_manager_uid(&(1000u32,).to_variant(), 1000), Ok(()));
        for reply in [
            (0u32,).to_variant(),
            (1001u32,).to_variant(),
            (1000u64,).to_variant(),
            ("1000",).to_variant(),
            ().to_variant(),
        ] {
            assert_eq!(
                ordinary_manager_uid(&reply, 1000),
                Err(ServiceRefusal::Unsupported)
            );
        }
        assert_eq!(
            ordinary_manager_uid(&(0u32,).to_variant(), 0),
            Err(ServiceRefusal::Unsupported)
        );
    }

    #[test]
    fn first_install_target_must_be_in_the_actual_lookup_roots_once() {
        let roots = vec!["/private/units".to_owned()];
        let target = std::path::Path::new("/private/units/gnome-terminal-server.service.d");
        assert!(target_is_searched(&roots, target));
        assert!(!target_is_searched(&[], target));
        assert!(!target_is_searched(&["/other/units".into()], target));
        assert!(!target_is_searched(
            &[roots[0].clone(), roots[0].clone()],
            target
        ));
        assert!(!target_is_searched(
            &roots,
            std::path::Path::new("/private/units/gnome-.service.d")
        ));
    }

    fn reference() -> Properties {
        let mut properties = BTreeMap::new();
        for (name, text) in [
            ("LoadState", "loaded"),
            (
                "FragmentPath",
                "/usr/lib/systemd/user/gnome-terminal-server.service",
            ),
            ("Type", "dbus"),
            ("BusName", BUS),
        ] {
            properties.insert(name, text.to_variant());
        }
        properties.insert("NeedDaemonReload", false.to_variant());
        properties.insert("Transient", false.to_variant());
        properties.insert("Names", vec![UNIT.to_owned()].to_variant());
        properties.insert("DropInPaths", Vec::<String>::new().to_variant());
        properties.insert(
            "ExecStart",
            vec![(
                SERVER.to_owned(),
                vec![SERVER.to_owned()],
                false,
                0u64,
                0u64,
                0u64,
                0u64,
                0u32,
                0i32,
                0i32,
            )]
            .to_variant(),
        );
        for name in ["ExecStartPre", "ExecStartPost", "ExecCondition"] {
            properties.insert(name, Vec::<Exec>::new().to_variant());
        }
        properties
    }

    #[test]
    fn only_exact_managed_definition_qualifies_and_reload_is_distinct() {
        let launcher = "/private/state/launcher-v1";
        let drop_in = "/private/config/gnome-terminal-server.service.d/90-lyn-native.conf";
        let paths = Some((launcher, drop_in));
        let mut installed = reference();
        let mut start = value::<Vec<Exec>>(&installed, "ExecStart").unwrap();
        start[0].0 = launcher.into();
        start[0].1 = vec![launcher.into()];
        installed.insert("ExecStart", start.clone().to_variant());
        installed.insert("DropInPaths", vec![drop_in.to_owned()].to_variant());
        assert_eq!(
            policy_for(&installed, paths).unwrap().1,
            DefinitionState::ManagedLoaded
        );
        assert_eq!(policy(&installed), Err(ServiceRefusal::Unsupported));
        installed.insert("NeedDaemonReload", true.to_variant());
        assert_eq!(
            policy_for(&installed, paths).unwrap().1,
            DefinitionState::NeedsReload
        );
        for needs_reload in [false, true] {
            let mut stock = reference();
            stock.insert("NeedDaemonReload", needs_reload.to_variant());
            assert_eq!(
                policy_for(&stock, paths).unwrap().1,
                DefinitionState::NeedsReload
            );
        }
        installed.insert(
            "DropInPaths",
            vec![drop_in.to_owned(), "/foreign.conf".into()].to_variant(),
        );
        assert_eq!(
            policy_for(&installed, paths),
            Err(ServiceRefusal::Unsupported)
        );
        installed.insert("DropInPaths", vec![drop_in.to_owned()].to_variant());
        start[0].1.push("foreign-argument".into());
        installed.insert("ExecStart", start.to_variant());
        assert_eq!(
            policy_for(&installed, paths),
            Err(ServiceRefusal::Unsupported)
        );
    }

    #[test]
    fn exact_reference_dbus_types_are_required_before_compatibility() {
        assert!(policy(&reference()).is_ok());
        for (_, name) in PROPERTIES {
            let mut properties = reference();
            properties.remove(name);
            assert_eq!(
                policy(&properties),
                Err(ServiceRefusal::Unsupported),
                "missing {name}"
            );
            properties.insert(name, 42u32.to_variant());
            assert_eq!(
                policy(&properties),
                Err(ServiceRefusal::Unsupported),
                "wrong type {name}"
            );
        }
    }

    #[test]
    fn retired_authority_distinguishes_stock_from_exact_cached_definition() {
        let paths = (
            "/private/state/launcher-v1",
            "/private/units/90-lyn-native.conf",
        );
        let mut properties = reference();
        assert_eq!(
            retired_policy(&properties, paths).unwrap().1,
            DefinitionState::Stock
        );
        properties.insert("NeedDaemonReload", true.to_variant());
        assert_eq!(
            retired_policy(&properties, paths).unwrap().1,
            DefinitionState::NeedsReload
        );
        let mut starts = value::<Vec<Exec>>(&properties, "ExecStart").unwrap();
        starts[0].0 = paths.0.into();
        starts[0].1 = vec![paths.0.into()];
        properties.insert("ExecStart", starts.to_variant());
        properties.insert("DropInPaths", vec![paths.1.to_owned()].to_variant());
        assert_eq!(
            retired_policy(&properties, paths).unwrap().1,
            DefinitionState::NeedsReload
        );
        properties.insert("NeedDaemonReload", false.to_variant());
        assert_eq!(
            retired_policy(&properties, paths).unwrap().1,
            DefinitionState::ManagedLoaded
        );
        properties.insert("DropInPaths", vec!["/foreign.conf".to_owned()].to_variant());
        assert_eq!(
            retired_policy(&properties, paths),
            Err(ServiceRefusal::Unsupported)
        );
    }

    #[test]
    fn customized_or_stale_service_is_refused() {
        for (name, replacement) in [
            ("LoadState", "masked".to_variant()),
            (
                "FragmentPath",
                "/home/user/gnome-terminal-server.service".to_variant(),
            ),
            ("Type", "simple".to_variant()),
            ("BusName", "org.example.Other".to_variant()),
            ("Transient", true.to_variant()),
            (
                "Names",
                vec![UNIT.to_owned(), "custom.service".to_owned()].to_variant(),
            ),
            ("NeedDaemonReload", true.to_variant()),
            (
                "DropInPaths",
                vec!["/tmp/custom.conf".to_owned()].to_variant(),
            ),
        ] {
            let mut properties = reference();
            properties.insert(name, replacement);
            assert_eq!(
                policy(&properties),
                Err(ServiceRefusal::Unsupported),
                "{name}"
            );
        }
        let start: Vec<Exec> = value(&reference(), "ExecStart").unwrap();
        let mut path = start.clone();
        path[0].0 = "/tmp/custom".to_owned();
        let mut args = start.clone();
        args[0].1.push("--custom".to_owned());
        let mut ignore = start.clone();
        ignore[0].2 = true;
        let mut duplicate = start.clone();
        duplicate.extend(start.clone());
        for changed in [vec![], path, args, ignore, duplicate] {
            let mut properties = reference();
            properties.insert("ExecStart", changed.to_variant());
            assert_eq!(policy(&properties), Err(ServiceRefusal::Unsupported));
        }
        for name in ["ExecStartPre", "ExecStartPost", "ExecCondition"] {
            let mut properties = reference();
            properties.insert(name, start.to_variant());
            assert_eq!(policy(&properties), Err(ServiceRefusal::Unsupported));
        }
    }

    #[test]
    fn dbus_activation_must_route_to_the_exact_service_and_server() {
        let reference =
            format!("[D-BUS Service]\nName={BUS}\nSystemdService={UNIT}\nExec={SERVER}\n");
        assert!(activation_agrees(&reference));
        for changed in [
            reference.replace(UNIT, "custom.service"),
            reference.replace(SERVER, "/tmp/custom"),
            reference.replace(BUS, "org.example.Other"),
            reference.replace("SystemdService=", "Other="),
            format!("{reference}Exec={SERVER}\n"),
            format!("{reference}[Other]\n"),
        ] {
            assert!(!activation_agrees(&changed));
        }
    }

    #[test]
    fn silent_bus_authentication_is_cancelled_without_starting_any_service() {
        use std::{
            os::unix::net::UnixListener,
            sync::{
                Arc,
                atomic::{AtomicBool, Ordering},
            },
        };
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("bus");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let done = Arc::new(AtomicBool::new(false));
        let stopped = done.clone();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut stream = None;
            while !stopped.load(Ordering::Relaxed) && Instant::now() < deadline {
                if stream.is_none() {
                    stream = listener.accept().ok();
                }
                thread::sleep(Duration::from_millis(2));
            }
        });
        let start = Instant::now();
        let result = inspect_address(&format!("unix:path={}", path.display()));
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        assert_eq!(result, Err(ServiceRefusal::Unavailable));
        assert!(start.elapsed() < Duration::from_millis(1600));
    }

    #[test]
    fn reference_service_read_only_opt_in() {
        if std::env::var_os("LYN_GNOME_SERVICE_READ_ONLY_TEST").is_none() {
            return;
        }
        assert_eq!(inspect_unmodified_service(), Ok(()));
    }
}
