//! Headless frame-cost benchmark for the whole-program view (`cargo test` ignores it).
//!
//! Loads a real C repository the way `argo-server` does (front end, then `c_chunk` rows, then
//! the GUI's rebuild), opens the whole-program view, and drives pan, zoom, hover and idle frames
//! through Rune's headless runtime and its wgpu renderer:
//!
//! ```sh
//! ARGO_BENCH_REPO=target/perf/TinyEMU cargo test --release -p argo-ui --features frame-profile \
//!     perf::whole_program -- --ignored --nocapture
//! ```
//!
//! `ARGO_BENCH_FRAMES` (default 60) frames per scenario, `ARGO_BENCH_SAMPLES` (default 5) runs;
//! `ARGO_BENCH_ONLY=pan` runs one scenario (for a profiler); `ARGO_BENCH_PNG=dir` writes each scenario's last frame for before/after pixel comparison.
//! Prints `METRIC scenario=.. metric=.. median=.. min=.. max=.. n=..` lines.

use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

use argo_protocol::NodeKind;
use rune_ui::render::{Gpu, OFFSCREEN_FORMAT, Offscreen, Renderer};
use rune_ui::{Env, Runtime};

use crate::app::{Item, State};
use crate::model::Claim;
use crate::project::{self, Project};

const SIZE: (f32, f32) = (1440.0, 900.0);
const SCALE: f32 = 2.0;

/// The repository as the GUI receives it, kept so each run starts from the same state.
struct Loaded {
    snap: argo_protocol::Snapshot,
    calls: std::sync::Arc<Vec<(argo_protocol::NodeId, argo_protocol::NodeId)>>,
}

fn load(repo: &Path) -> Loaded {
    let root = std::fs::canonicalize(repo).expect("ARGO_BENCH_REPO");
    let opts = argo_c::project::Options::default();
    let name = root.file_name().map_or("program".into(), |n| n.to_string_lossy().into_owned());
    let opened = argo_c::project::open(&root, &name, &opts, &|_| {}).expect("open repository");
    let origin = argo_db_client::Origin { locator: root.display().to_string(), commit: None, units: vec![], flags: vec![] };
    // As `argo-server` converts it (`repo.rs`).
    let rows = argo_db_client::to_rows(&opened.snapshot, &origin, &|file, pos| {
        let (path, line) = opened.places.at(file, pos)?;
        Some((path.to_string(), line))
    })
    .expect("rows");
    let snap = argo_db_client::rebuild(&rows).expect("rebuild").snapshot;
    let calls = std::sync::Arc::new(crate::calls::calls(&snap));
    Loaded { snap, calls }
}

/// The whole-program view of `l`, fitted, with workers at work.
fn state(l: &Loaded) -> State {
    let mut s = State::from_snapshot(l.snap.clone(), None, HashSet::new());
    let name = l.snap.graph.nodes.iter().find(|n| n.kind == NodeKind::Program).map_or("program".into(), |n| n.name.clone());
    let (modules, declarations) = (project::index(&l.snap), project::declarations(&l.snap));
    s.project = Some(Project { name, source: String::new(), checkout: String::new(), units: vec![], flags: vec![], modules, declarations });
    if let Some(live) = &mut s.live {
        live.calls = Some(l.calls.clone());
    }
    project::open_program(&mut s);
    // Workers at work: every 20th function is being translated (it glows), as during a run.
    for (i, n) in s.view.nodes.iter_mut().enumerate() {
        if i % 20 == 3 {
            n.claim = Some(Claim { worker: (i % 7) as u8, active: true });
        }
    }
    s
}

struct Frame {
    cpu_ms: f64,
    view_ms: f64,
    commit_ms: f64,
    layout_ms: f64,
    text_ms: f64,
    apply_ms: f64,
    paint_ms: f64,
    encode_ms: f64,
    gpu_ms: f64,
    instances: f64,
}

fn render(gpu: &Gpu, out: &Offscreen, renderer: &mut Renderer, rt: &mut Runtime<State>) -> (f64, f64) {
    let t0 = Instant::now();
    renderer.prepare(&gpu.device, &gpu.queue, &mut rt.text.atlas, &rt.instances, (out.width, out.height));
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    renderer.draw(&mut enc, &out.view, rt.clear_color());
    renderer.submit(&gpu.queue, enc);
    let encode = t0.elapsed().as_secs_f64() * 1000.0;
    let t1 = Instant::now();
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).ok();
    (encode, t1.elapsed().as_secs_f64() * 1000.0)
}

fn scenario(gpu: &Gpu, out: &Offscreen, l: &Loaded, name: &str, frames: usize, png: Option<&Path>) -> Vec<Frame> {
    let s = state(l);
    let n = s.view.nodes.len();
    let mut rt = Runtime::new(s, crate::view, SIZE, Env::new(true, SCALE));
    let mut renderer = Renderer::new(&gpu.device, OFFSCREEN_FORMAT);
    rt.frame(0.0);
    for _ in 0..30 {
        if !rt.frame(1.0 / 120.0) {
            break;
        }
    }
    // Zoomed in to a readable level near the middle, as someone exploring would be.
    rt.update(|s, tx| {
        s.cam.zoom_about(SIZE.0 / 2.0, SIZE.1 / 2.0, 3.0);
        tx.instant();
    });
    for _ in 0..30 {
        if !rt.frame(1.0 / 120.0) {
            break;
        }
    }
    render(gpu, out, &mut renderer, &mut rt);
    let mut rows = Vec::with_capacity(frames);
    for f in 0..frames {
        let t = f as f32 / frames as f32;
        match name {
            "idle" => {}
            "pan" => rt.update(|s, tx| {
                let a = t * std::f32::consts::TAU;
                s.cam.pan[0] += a.cos() * 9.0;
                s.cam.pan[1] += a.sin() * 7.0;
                tx.instant();
            }),
            "zoom" => rt.update(|s, tx| {
                let k = if f < frames / 2 { 1.02 } else { 1.0 / 1.02 };
                s.cam.zoom_about(SIZE.0 / 2.0, SIZE.1 / 2.0, k);
                tx.instant();
            }),
            "hover" => rt.update(|s, _| {
                s.hovered = Some(Item::Node((f * 37) % n));
            }),
            _ => unreachable!(),
        }
        rune_ui::take_frame_phases();
        let cpu0 = rune_ui::thread_cpu_ms();
        rt.frame(1.0 / 120.0);
        let cpu_ms = rune_ui::thread_cpu_ms() - cpu0;
        let ph = rune_ui::take_frame_phases();
        let (encode_ms, gpu_ms) = render(gpu, out, &mut renderer, &mut rt);
        // Phases: View, Commit, Layout, Text, Paint, Apply (wall then CPU columns).
        let p = rune_ui::PHASES;
        rows.push(Frame {
            cpu_ms,
            view_ms: ph[p],
            commit_ms: ph[p + 1],
            layout_ms: ph[p + 2],
            text_ms: ph[p + 3],
            apply_ms: ph[p + 5],
            paint_ms: ph[p + 4],
            encode_ms,
            gpu_ms,
            instances: rt.instances.len() as f64,
        });
    }
    if let Some(dir) = png {
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        renderer.draw(&mut enc, &out.view, rt.clear_color());
        out.copy(&mut enc);
        renderer.submit(&gpu.queue, enc);
        let px = out.read(&gpu.device);
        rune_ui::render::write_png(&dir.join(format!("{name}.png")), out.width, out.height, &px).unwrap();
    }
    rows
}

#[test]
#[ignore]
fn whole_program() {
    let Ok(repo) = std::env::var("ARGO_BENCH_REPO") else {
        eprintln!("set ARGO_BENCH_REPO to a C repository");
        return;
    };
    let env = |k: &str, d: usize| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let (frames, samples) = (env("ARGO_BENCH_FRAMES", 60), env("ARGO_BENCH_SAMPLES", 5));
    let png = std::env::var("ARGO_BENCH_PNG").ok().map(std::path::PathBuf::from);
    if let Some(d) = &png {
        std::fs::create_dir_all(d).unwrap();
    }
    if cfg!(debug_assertions) {
        eprintln!("perf: debug build; time a --release build");
    }
    let t = Instant::now();
    let loaded = load(Path::new(&repo));
    let base = state(&loaded);
    eprintln!(
        "loaded in {:.1}s: {} nodes, {} wires, {} frames",
        t.elapsed().as_secs_f64(),
        base.view.nodes.len(),
        base.view.wires.len(),
        base.view.groups.len()
    );
    crate::ui::set_viewport([SIZE.0, SIZE.1]);
    let gpu = Gpu::headless();
    let out = Offscreen::new(&gpu.device, (SIZE.0 * SCALE) as u32, (SIZE.1 * SCALE) as u32);
    let only = std::env::var("ARGO_BENCH_ONLY").ok();
    for name in ["idle", "pan", "zoom", "hover"] {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        let mut runs: Vec<Vec<Frame>> = vec![];
        for i in 0..samples {
            runs.push(scenario(&gpu, &out, &loaded, name, frames, png.as_deref().filter(|_| i == 0)));
        }
        type Get = fn(&Frame) -> f64;
        let metrics: [(&str, Get); 10] = [
            ("cpu_ms", |f| f.cpu_ms),
            ("view_ms", |f| f.view_ms),
            ("commit_ms", |f| f.commit_ms),
            ("layout_ms", |f| f.layout_ms),
            ("text_ms", |f| f.text_ms),
            ("apply_ms", |f| f.apply_ms),
            ("paint_ms", |f| f.paint_ms),
            ("encode_ms", |f| f.encode_ms),
            ("gpu_wait_ms", |f| f.gpu_ms),
            ("instances", |f| f.instances),
        ];
        for (metric, get) in metrics {
            // Per run: the mean over its frames; then the median across runs.
            let mut v: Vec<f64> = runs.iter().map(|r| r.iter().map(get).sum::<f64>() / r.len() as f64).collect();
            v.sort_by(f64::total_cmp);
            println!(
                "METRIC scenario={name} metric={metric} median={:.3} min={:.3} max={:.3} n={}",
                v[v.len() / 2],
                v[0],
                v[v.len() - 1],
                v.len()
            );
        }
    }
}
