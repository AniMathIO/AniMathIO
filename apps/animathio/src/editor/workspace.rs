//! Resizable workspace. Only view state is written to the layout sidecar.

use super::*;
use gpui_component::resizable::{h_resizable, resizable_panel, v_resizable};

impl Editor {
    pub(super) fn workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        // Include visibility in the ID: a reopened panel starts from its saved
        // size rather than inheriting the slot of a different hidden panel.
        let id = SharedString::from(format!(
            "workspace-{}-{}",
            self.dock.left_open, self.dock.right_open
        ));
        let mut middle = h_resizable(id).on_resize(cx.listener(
            |this, state: &Entity<gpui_component::resizable::ResizableState>, _, cx| {
                let sizes = state.read(cx).sizes();
                if this.dock.left_open
                    && let Some(size) = sizes.first()
                {
                    this.dock.left_width = f32::from(*size);
                }
                if this.dock.right_open
                    && let Some(size) = sizes.last()
                {
                    this.dock.right_width = f32::from(*size);
                }
                this.dock.clamp();
                this.dock.save();
            },
        ));
        if self.dock.left_open {
            middle = middle.child(
                resizable_panel()
                    .size(px(self.dock.left_width))
                    .size_range(px(160.)..px(640.))
                    .child(self.media_pool(cx)),
            );
        }
        middle = middle.child(
            resizable_panel()
                .size_range(px(240.)..px(10000.))
                .child(self.viewer(window, cx)),
        );
        if self.dock.right_open {
            middle = middle.child(
                resizable_panel()
                    .size(px(self.dock.right_width))
                    .size_range(px(200.)..px(720.))
                    .child(self.inspector(window, cx)),
            );
        }
        let workspace = v_resizable(if self.dock.bottom_open {
            "workspace-timeline"
        } else {
            "workspace-viewer"
        })
        .child(
            resizable_panel()
                .size_range(px(140.)..px(10000.))
                .child(middle),
        )
        .when(self.dock.bottom_open, |workspace| {
            workspace.child(
                resizable_panel()
                    .size(px(self.dock.bottom_height))
                    .size_range(px(120.)..px(600.))
                    .child(self.timeline_panel(cx)),
            )
        })
        .on_resize(cx.listener(
            |this, state: &Entity<gpui_component::resizable::ResizableState>, _, cx| {
                if this.dock.bottom_open
                    && let Some(size) = state.read(cx).sizes().last()
                {
                    this.dock.bottom_height = f32::from(*size);
                    this.dock.clamp();
                    this.dock.save();
                }
            },
        ));
        div().flex_1().min_h_0().child(workspace)
    }
}
