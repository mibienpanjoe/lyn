//! Lyn-managed media staging and storage.

use std::str::FromStr;

use crate::contract::{MediaId, StagedMediaId};

pub(crate) mod audio;
pub(crate) mod images;
pub(crate) mod staging;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LynMediaResource {
    Staged(StagedMediaId),
    Capture(MediaId),
}

pub(crate) fn resource_from_request(host: Option<&str>, path: &str) -> Option<LynMediaResource> {
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        return None;
    }
    let (kind, id) = match host {
        Some(kind @ ("staged" | "capture")) => (kind, path),
        Some("localhost" | "lyn-media.localhost") => path.split_once('/')?,
        _ => return None,
    };
    match kind {
        "staged" => StagedMediaId::from_str(id)
            .ok()
            .map(LynMediaResource::Staged),
        "capture" => MediaId::from_str(id).ok().map(LynMediaResource::Capture),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{LynMediaResource, resource_from_request};

    const ID: &str = "550e8400-e29b-41d4-a716-446655440000";

    #[test]
    fn linux_style_hosts_resolve_staged_and_capture_ids() {
        assert_eq!(
            resource_from_request(Some("staged"), &format!("/{ID}")),
            Some(LynMediaResource::Staged(ID.parse().unwrap()))
        );
        assert_eq!(
            resource_from_request(Some("capture"), &format!("/{ID}")),
            Some(LynMediaResource::Capture(ID.parse().unwrap()))
        );
    }

    #[test]
    fn windows_and_convert_file_src_hosts_resolve_kind_from_path() {
        assert_eq!(
            resource_from_request(Some("localhost"), &format!("/staged/{ID}")),
            Some(LynMediaResource::Staged(ID.parse().unwrap()))
        );
        assert_eq!(
            resource_from_request(Some("lyn-media.localhost"), &format!("/capture/{ID}")),
            Some(LynMediaResource::Capture(ID.parse().unwrap()))
        );
    }

    #[test]
    fn unknown_hosts_and_invalid_ids_are_rejected() {
        assert_eq!(
            resource_from_request(Some("other"), &format!("/{ID}")),
            None
        );
        assert_eq!(resource_from_request(Some("staged"), "/not-a-uuid"), None);
        assert_eq!(resource_from_request(None, &format!("/staged/{ID}")), None);
        assert_eq!(resource_from_request(Some("staged"), "/"), None);
    }
}
