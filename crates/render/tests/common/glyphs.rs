use animathio_core::{ClipId, ClipKind, Scene, SceneClip, Time, TrackId, Transform};
use serde_json::json;

pub fn scene(kind: &str) -> Scene {
    let mut params = json!({
        "text": "0 O A B 8 @ % & ∞", "font_size": 0.12,
        "units": "normalized", "align": "center",
        "color": [255, 255, 255, 255],
    });
    match kind {
        "canvas" => {
            params["units"] = json!("pixels");
            params["font_size"] = json!(28);
        }
        "title" | "classic" => {
            params["fit_width"] = json!(0.9);
            params["fit_height"] = json!(0.4);
            if kind == "classic" {
                params["caption_style"] = json!("classic");
            }
        }
        _ => {}
    }
    Scene {
        canvas: (0, 0),
        clips: vec![SceneClip {
            track: TrackId(0),
            id: ClipId(0),
            kind: ClipKind::Plugin {
                plugin_id: "org.animathio.text".into(),
                params,
            },
            source_time: Time(0),
            fit: animathio_core::Fit::Stretch,
            transform: if matches!(kind, "title" | "classic") {
                Transform {
                    x: 0.05,
                    y: 0.3,
                    scale_x: 0.9,
                    scale_y: 0.4,
                    ..Transform::default()
                }
            } else {
                Transform::default()
            },
            params: Default::default(),
            effects: vec![],
            transition: None,
        }],
    }
}
