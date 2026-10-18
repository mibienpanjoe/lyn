//! Native producer to existing capture contract, without application/UI state.

use std::sync::{Arc, Mutex};

use super::{
    gnome_broker::{NativeBroker, NativeSourceLease},
    gnome_process::ProcessInvocation,
    session_registry::LiveContextSource,
};
use crate::{
    contract::{ContextCandidate, ContextProviderKind, ContextResolution, ContextSelection},
    storage::{Database, contexts::ContextRepository},
};

/// Worker-only preparation. All ownership/proc/PTY/Git/SQLite I/O happens before
/// the invocation ticket is committed and the source enters the registry.
pub(crate) fn prepare(
    broker: Arc<NativeBroker>,
    window: u32,
    invocation: ProcessInvocation,
    database: &Mutex<Database>,
) -> Option<(ContextResolution, LiveContextSource)> {
    let lease = NativeSourceLease::query(broker, window, invocation).ok()?;
    let source = LiveContextSource::from_native(lease)?;
    let identity = source.identity();
    let context = {
        let database = database.lock().ok()?;
        ContextRepository::new(database.connection())
            .ensure_project(
                source.context().id,
                &source.context().name,
                identity.project_key.as_deref(),
                &identity.project_path,
            )
            .ok()?
    };
    if !source.revalidate_native_lifetime() {
        return None;
    }
    let resolution = ContextResolution::Resolved {
        candidate: ContextCandidate {
            context,
            branch_name: identity.branch_name.clone(),
            provider: ContextProviderKind::Shell,
            requires_confirmation: false,
        },
        selection: Some(ContextSelection::LiveSource {
            source_id: source.source_id(),
        }),
    };
    Some((resolution, source))
}
