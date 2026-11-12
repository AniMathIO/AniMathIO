//! Playback glue.

use super::*;

impl Editor {
    pub(super) fn request_render(&mut self, time: Time) {
        let size = if self.bench.is_some() {
            BENCH_SIZE
        } else {
            project_size(&self.project)
        };
        self.request_render_at(time, size);
    }

    pub(super) fn request_render_at(&mut self, time: Time, size: (u32, u32)) {
        self.request_generation = self.request_generation.wrapping_add(1);
        self.last_requested = Some(time);
        let scene = evaluate(&self.project, time);
        self.worker.request(RenderRequest {
            generation: self.request_generation,
            scene,
            size,
        });
    }

    /// Run plugin clips through the plugin host so the render worker only ever
    /// sees media, builtin, vector and error clips.
    pub(super) fn resolve_scene(&mut self, scene: Scene, size: (u32, u32)) -> Scene {
        match &mut self.plugin_host {
            Some(host) => host.resolve(&scene, size),
            None => scene,
        }
    }

    pub(super) fn pump_results(&mut self, window: &mut Window) -> bool {
        let mut changed = false;
        while let Some(result) = self.worker.try_result() {
            if result.generation == self.request_generation {
                changed = true;
                if let Some(bench) = &mut self.bench {
                    let now = Instant::now();
                    if let Some(last) = bench.last_delivery.replace(now) {
                        bench.cadence_ms.push((now - last).as_secs_f64() * 1000.0);
                    }
                }
                if let Some(old) = self.image.replace(result.image) {
                    let _ = window.drop_image(old);
                }
            }
        }
        changed
    }

    /// Drive the viewport benchmark when `--bench-viewport` is set: warm up,
    /// then scrub at 60 Hz for ten seconds and report the delivered-frame
    /// cadence before quitting.
    pub(super) fn bench_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bench = self
            .bench
            .as_mut()
            .expect("bench tick only runs with a bench");
        if bench.done {
            return;
        }
        let now = Instant::now();
        if bench.active_start.is_none() {
            if now.duration_since(bench.warmup_started) < BENCH_WARMUP {
                window.request_animation_frame();
                return;
            }
            bench.active_start = Some(now);
            bench.last_delivery = Some(now);
            bench.next_request = Some(now);
        }
        let elapsed = now.duration_since(bench.active_start.expect("active start set"));
        if elapsed >= BENCH_DURATION {
            bench.done = true;
            let mut samples = std::mem::take(&mut bench.cadence_ms);
            samples.sort_by(f64::total_cmp);
            let p50 = percentile(&samples, 0.50);
            let p95 = percentile(&samples, 0.95);
            eprintln!(
                "viewport bench: samples={} p50={:.3}ms p95={:.3}ms ({:.2} fps p50)",
                samples.len(),
                p50,
                p95,
                if p50 > 0.0 { 1000.0 / p50 } else { 0.0 }
            );
            cx.quit();
            return;
        }
        // Request a new deterministic random frame at a 60 Hz cadence so the
        // worker (latest-request-wins) is neither starved nor flooded.
        let due = {
            let bench = self
                .bench
                .as_mut()
                .expect("bench tick only runs with a bench");
            if bench.next_request.is_some_and(|next| now >= next) {
                let duration = timeline::project_duration(&self.project).max(1);
                let tick = bench.next_tick(duration);
                bench.next_request = Some(now + Duration::from_micros(16_667));
                Some(tick)
            } else {
                None
            }
        };
        if let Some(tick) = due {
            self.request_render_at(Time(tick), BENCH_SIZE);
        }
        window.request_animation_frame();
    }
}
