//! Sandbox containment fixture: `render-clip` never returns, exhausting the host fuel budget.

wit_bindgen::generate!({
    path: "../../crates/plugin-api/wit",
    world: "plugin",
});

use animathio::plugin::types::*;

struct TrapPlugin;

impl Guest for TrapPlugin {
    fn describe() -> Manifest {
        Manifest {
            name: "trap".to_string(),
            version: "0.1.0".to_string(),
            description: "Infinite-loop fixture for sandbox containment tests".to_string(),
            kinds: vec![PluginKind::ClipSource],
            params: vec![],
        }
    }

    fn render_clip(_params: Vec<ParamEntry>, _time_ticks: i64, _size: Size) -> Scene {
        let mut n: u64 = 0;
        loop {
            n = n.wrapping_add(1);
            std::hint::black_box(n);
        }
    }

    fn apply_effect(input: Scene, _params: Vec<ParamEntry>, _t: i64, _size: Size) -> Scene {
        input
    }

    fn evaluate_curve(_t: i64, _duration: i64, _params: Vec<ParamEntry>) -> f64 {
        0.0
    }
}

export!(TrapPlugin);
