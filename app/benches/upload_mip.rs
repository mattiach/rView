//! Texture upload + linear-light mip generation of a 24 MP image, waited on the GPU.
//! Decides the GPU pre-upload of adjacent images: introduced only if this exceeds 16 ms.
//! `cargo bench -p rview --bench upload_mip`

use rview::render::Gpu;

fn main() {
    let gpu = Gpu::new(false).expect("gpu");
    let (w, h) = (6000u32, 4000u32);
    let rgba: Vec<u8> = (0..w * h * 4).map(|i| (i % 251) as u8).collect();
    for (name, mips) in [("24 MP upload", false), ("24 MP upload + mips", true)] {
        bench(name, 20, || {
            let img = gpu.upload(w, h, &rgba, mips, (w, h));
            gpu.queue.submit([]);
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            img
        });
    }
}

/// Median and best of `n` runs of `f` (after one warm-up), printed as `BENCH <name>: ...`.
/// Plain timing instead of criterion (which pulls C code through `alloca`).
fn bench<T>(name: &str, n: usize, mut f: impl FnMut() -> T) {
    std::hint::black_box(f());
    let mut t: Vec<f64> = (0..n)
        .map(|_| {
            let s = std::time::Instant::now();
            std::hint::black_box(f());
            s.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    t.sort_by(f64::total_cmp);
    println!(
        "BENCH {name}: median {:.1} ms, best {:.1} ms ({n} runs)",
        t[n / 2],
        t[0]
    );
}
