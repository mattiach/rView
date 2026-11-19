//! E2E: a corrupt file shows the centred error message. Own process: one event loop.

use rview::driver::Step;
use rview::{Config, run};

fn main() {
    let dir = std::env::temp_dir().join(format!("rview-e2e-err-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let bad = dir.join("broken.png");
    std::fs::write(&bad, b"\x89PNG\r\n\x1a\nthis is not a png").unwrap();
    let r = run(Config {
        files: vec![bad],
        software: true,
        driver: Some(vec![
            Step::WaitIdle,
            Step::Check(Box::new(|o| match &o.error {
                Some(e) if e.starts_with("Error occurred opening\nbroken.png\n") && e.contains("(Error 2)") => Ok(()),
                _ => Err(format!("expected decode error, got {o:?}")),
            })),
        ]),
        ..Default::default()
    });
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = r {
        eprintln!("e2e_error FAILED:\n{e:#}");
        std::process::exit(1);
    }
    println!("e2e_error: OK");
}
