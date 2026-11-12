//! Icon helpers shared by the editor chrome.
//!
//! The UI never uses Unicode symbol glyphs as pictograms. Every icon is either a
//! Lucide SVG from the full catalog shipped by `gpui-kit-assets`, or a
//! Lucide-style SVG bundled under `apps/animathio/assets/icons` and embedded via
//! [`gpui_component::Icon::data`].

use animathio_core::ClipKind;
use gpui_component::Icon;
pub(crate) use gpui_kit_assets::IconName;

/// Build an [`Icon`] for a Lucide [`IconName`].
pub(crate) fn icon(name: IconName) -> Icon {
    Icon::new(name)
}

/// Keyframe diamond: filled when the keyframe is active, outlined otherwise.
/// The filled variant is bundled locally because Lucide only ships the outline.
pub(crate) fn diamond(filled: bool) -> Icon {
    if filled {
        Icon::default().data(include_bytes!("../assets/icons/diamond-filled.svg"))
    } else {
        Icon::new(IconName::Diamond)
    }
}

/// The Lucide pictogram for a clip's source kind.
pub(crate) fn kind_icon(kind: &ClipKind) -> IconName {
    match kind {
        ClipKind::Video(_) => IconName::Video,
        ClipKind::Image(_) => IconName::Image,
        ClipKind::Audio(_) => IconName::AudioWaveform,
        ClipKind::Builtin(builtin) => match builtin {
            animathio_core::builtin::Builtin::Text { .. } => IconName::Type,
            animathio_core::builtin::Builtin::Shape { .. } => IconName::Shapes,
        },
        ClipKind::Plugin { plugin_id, .. } => {
            if plugin_id.contains("latex") || plugin_id.contains("math") {
                IconName::Sigma
            } else if plugin_id.contains("graph") {
                IconName::ChartNoAxesColumn
            } else {
                IconName::Sparkles
            }
        }
        ClipKind::Vector(_) => IconName::Layers,
        ClipKind::Error(_) => IconName::TriangleAlert,
    }
}
