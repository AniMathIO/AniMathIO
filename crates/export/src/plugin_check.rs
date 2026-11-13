//! Export must never bake placeholder boxes into a video: a plugin clip that was not
//! resolved to a scene, or that resolved to an error, fails the export instead.

use animathio_core::{ClipId, ClipKind, Scene};

use crate::Error;

/// Plugin clips of an evaluated scene, in scene order.
pub(crate) fn pending_plugins(scene: &Scene) -> Vec<(ClipId, String)> {
    scene
        .clips
        .iter()
        .filter_map(|clip| match &clip.kind {
            ClipKind::Plugin { plugin_id, .. } => Some((clip.id, plugin_id.clone())),
            _ => None,
        })
        .collect()
}

/// Fail if any of the `pending` plugin clips is still unresolved or errored in `resolved`.
pub(crate) fn check_resolved(pending: &[(ClipId, String)], resolved: &Scene) -> Result<(), Error> {
    for (id, plugin) in pending {
        let Some(clip) = resolved.clips.iter().find(|clip| clip.id == *id) else {
            continue;
        };
        let message = match &clip.kind {
            ClipKind::Plugin { .. } => {
                "plugin is not loaded (build the plugins with scripts/build-plugins.sh, or set \
                 ANIMATHIO_PLUGIN_DIR)"
                    .to_owned()
            }
            ClipKind::Error(message) => message.clone(),
            _ => continue,
        };
        return Err(Error::Plugin {
            clip: id.0,
            plugin: plugin.clone(),
            message,
        });
    }
    Ok(())
}
