//! Sandbox containment fixture: `render-clip` never returns, exhausting the host fuel budget.

wit_bindgen::generate!({
    path: "../../crates/plugin-api/wit",
    world: "source-plugin",
});

use animathio::plugin::types::*;

struct TrapPlugin;

impl Guest for TrapPlugin {
    fn describe() -> Manifest {
        Manifest {
            name: "trap".to_string(),
            version: "1.0.0".to_string(),
            description: "Infinite-loop fixture for sandbox containment tests".to_string(),
            kinds: vec!["clip-source".into()],
            metadata: vec![],
            params: vec![],
        }
    }

    fn render_clip(
        _params: Vec<ParamEntry>,
        _time_ticks: i64,
        _size: Size,
    ) -> Result<Scene, Diagnostic> {
        let mut n: u64 = 0;
        loop {
            n = n.wrapping_add(1);
            std::hint::black_box(n);
        }
    }
}

export!(TrapPlugin);
