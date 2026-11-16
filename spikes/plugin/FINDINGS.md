# WebAssembly Component Model Plugin Spike: Findings & Decision

**Spike Goal:** Evaluate whether the WebAssembly Component Model (`wasmtime` + WIT) is suitable for AniMathIO 2.0's plugin architecture, where plugins are pure sandboxed functions of `(params, time)` returning vector scenes rendered by the host using `vello_cpu` and cached by `hash(plugin@version, params, t)`.

---

## 1. Verdict: GO

**Decision:** **GO** with high confidence.

The WebAssembly Component Model meets all requirements for AniMathIO 2.0:
1. **Low Overhead:** Per-call boundary overhead is **~29 µs (0.029 ms)**, less than 0.2% of a 16.6 ms (60 fps) frame budget.
2. **Instant Cached Load:** Precompiled component deserialization takes **0.185 ms**, enabling instant project and plugin loading without compilation pauses.
3. **Rock-Solid Sandboxing:** Deterministic fuel metering and memory limits completely contain infinite loops, traps, and memory bloat. The host process survives all guest failures and recovers immediately.
4. **Zero-Ambient-Authority Sandbox:** WASI Preview 2 host context is instantiated without filesystem preopens or socket permissions, ensuring untrusted plugins cannot access user files or the network.
5. **Host-Side Services:** Host-provided text shaping (`skrifa`) via WIT imports allows vector plugins to stay small (**~90 KB**) without bundling fonts or text engines.

---

## 2. Environment & Versions

| Component | Version |
|---|---|
| OS | Linux (x86_64) |
| Rust toolchain | `rustc 1.99.0`, `cargo 1.99.0` |
| Compilation Target | `wasm32-wasip2` (LLVM `wasm-component-ld` bundled) |
| WebAssembly Runtime | `wasmtime 50.0.0` (with `component-model`, fuel metering) |
| WASI Implementation | `wasmtime-wasi 50.0.0` (Preview 2) |
| WIT Bindings Generator | `wit-bindgen 0.40.0` |
| Renderer | `vello_cpu 0.4.0` (CPU rasterization via sparse strips SIMD) |
| Font Shaping Engine | `skrifa 0.49.0` (outlining glyphs to Bézier curves) |
| Curves & Primitives | `kurbo 0.13.1`, `peniko 0.6.1` |
| Cryptographic Hash | `blake3 1.8.7` |
| Cache Engine | `lru 0.19.0` |

---

## 3. Measured Numbers

All benchmarks were measured on release builds under `nice -n 19 ionice -c3` with 3 build jobs:

| Metric | Result | Target / Budget | Assessment |
|---|---|---|---|
| **Plugin `.wasm` size** | **92,909 bytes** (90.7 KB) | < 500 KB | Excellent. Minimal footprint without font libraries. |
| **Component cold compile** | **17.91 ms** | < 100 ms | Fast initial compilation time. |
| **Component deserialize (cache)** | **0.185 ms** (185 µs) | < 5 ms | **97x faster** than cold compile; instantaneous startup. |
| **Plugin instantiate time** | **0.340 ms** (340 µs) | < 2 ms | Cheap instantiation per clip/track instance. |
| **Per-call `render` overhead (p50)** | **29.00 µs** (0.029 ms) | < 500 µs | **~34,500 calls/sec** per core; negligible latency. |
| **Per-call `render` overhead (p95)** | **30.00 µs** (0.030 ms) | < 1,000 µs | Extremely tight variance. |
| **Per-call `render` overhead (p99)** | **37.00 µs** (0.037 ms) | < 2,000 µs | No GC pauses or tail latency spikes. |
| **LRU Cache-hit cost (p50)** | **16.36 µs** | < 50 µs | Dominated by BLAKE3 hashing 92 KB plugin bytes. |
| **LRU Cache-hit cost (p95)** | **16.50 µs** | < 100 µs | Deterministic lookup. |
| **Infinite loop / Fuel exhaustion** | **Trapped cleanly** | Host must survive | Execution trapped on fuel limit; host undamaged. |
| **Host resilience after trap** | **Clean recovery** | Host must survive | Discard store, instantiate new store; 100% stable. |

---

## 4. WIT Design Notes & Learnings

### A. Identifier Escaping & WIT Keywords
- WIT reserved keywords like `string`, `bool`, `number` cannot be used directly as variant case names. Writing:
  ```wit
  variant param-kind {
      string(string-param), // Parse error: expected an identifier, found keyword 'string'
  }
  ```
  Causes parser errors in `wasmtime::component::bindgen!`.
- **Solution:** Escape keywords with `%` (e.g. `%string(string-param)`, `%number(number-range)`, `%bool(bool-param)`).

### B. Parameter Schema Mapping
- WIT variants map naturally to Rust algebraic data types.
- Defining a unified `param-spec` with `param-kind` variants (Number, Color, String, EnumChoice, Bool) allows the host UI to auto-generate inspectors and validate keyframes without touching plugin code.
- Plugins declare schemas once in `describe()` and receive `list<param-entry>` in `render()`.

### C. Host Text Shaping Import
- Providing `shape-text(text: string, font-size: f32) -> list<path>` as a host import was straightforward and highly effective.
- The plugin calls `host::shape_text` to receive Bézier path contours from the host's font engine (`skrifa`), allowing vector math animations (morphing, path reveals, writes) without embedding font engines inside each WebAssembly component.

### D. Host Linker & GAT Ergonomics
- In `wasmtime 50.0`, linking host traits with GATs requires explicit type parameter annotation:
  ```rust
  Plugin::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |s| s)?;
  ```
- WASI Preview 2 integration (`wasmtime-wasi`) requires implementing `WasiView` returning `WasiCtxView { ctx, table }`.

---

## 5. Architectural Recommendations for AniMathIO 2.0

1. **Two-Stage Cache Key (Optimization):**
   - In this spike, the cache key computed `blake3(plugin_bytes || params || t)`. Hashing the entire 92 KB binary on every frame took 16 µs.
   - *Recommendation:* Compute `plugin_hash = blake3(plugin_bytes)` once when the plugin is loaded or registered. The per-frame cache key then hashes `blake3(plugin_hash || params || t)`, which executes in **< 0.5 µs**.
2. **Binary Buffer Scene Format for Complex Manim Scenes:**
   - For simple vector scenes (hundreds of path verbs), WIT structured records (`list<draw-command>`) have negligible overhead.
   - For complex Manim scenes with tens of thousands of segments, adopt Issue #156's dual-output design:
     `variant output { vector(list<u8>), raster(list<u8>) }` using flat buffers / bytemuck arrays to eliminate per-verb allocator overhead.
3. **Precompiled Artifact Cache:**
   - Store serialized components (`component.serialize()`) in `~/.cache/animathio/plugins/<blake3>.bin`. At 0.18 ms load time, the editor can load dozens of plugins at startup in under 5 milliseconds.
4. **Per-Track Instance Reuse:**
   - Wasmtime stores can be reused across frame renders by calling `store.set_fuel(N)` before each call, avoiding even the 340 µs instantiation cost during continuous playback.

---

## 6. Output Artifacts

- Sample frame rendered at `t=30` ticks (0.5s): [frame_030.png](file:///home/hunor/.t3/worktrees/AniMathIO/t3-fc5fb01b-9c802e01bbdc1e48/spikes/plugin/out/frame_030.png)
- Plugin implementation: [plugins/shapes/src/lib.rs](file:///home/hunor/.t3/worktrees/AniMathIO/t3-fc5fb01b-9c802e01bbdc1e48/spikes/plugin/plugins/shapes/src/lib.rs)
- Host implementation: [host/src/main.rs](file:///home/hunor/.t3/worktrees/AniMathIO/t3-fc5fb01b-9c802e01bbdc1e48/spikes/plugin/host/src/main.rs)
- WIT definition: [wit/plugin.wit](file:///home/hunor/.t3/worktrees/AniMathIO/t3-fc5fb01b-9c802e01bbdc1e48/spikes/plugin/wit/plugin.wit)
