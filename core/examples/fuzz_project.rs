//! Mutation fuzzer for the `.rvm` reader (pure Rust instead of cargo-fuzz/libFuzzer,
//! which compile C++). Not coverage-guided: it keeps the inputs that give
//! a new outcome (error message) as a cheap novelty signal. Two kinds of inputs:
//! - byte mutations of the seed corpus (the security-test archives) and of kept inputs;
//! - structure-aware ones: valid ZIPs whose manifest/document JSON text is mutated (past the CRC
//!   and ratio checks, into serde and the semantic validation).
//!
//! Run: `cargo run --profile fuzz -p rv-core --example fuzz_project -- [seconds=600] [seed]`
//! Panics are caught, the input saved to `%TEMP%\rview-fuzz\crash-<n>.rvm`, exit code 1.

#[path = "../tests/common/projects.rs"]
mod projects;

use rv_core::decode::DecodeOptions;
use std::collections::HashSet;
use std::time::{Duration, Instant};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
    fn chance(&mut self, one_in: usize) -> bool {
        self.below(one_in) == 0
    }
}

const INTERESTING: &[u64] = &[
    0,
    1,
    2,
    3,
    0x7F,
    0x80,
    0xFF,
    0x100,
    0x7FFF,
    0x8000,
    0xFFFF,
    0x1_0000,
    0x7FFF_FFFF,
    0xFFFF_FFFF,
];

fn mutate_bytes(r: &mut Rng, mut v: Vec<u8>, other: &[u8]) -> Vec<u8> {
    for _ in 0..1 + r.below(8) {
        let len = v.len();
        match r.below(9) {
            0 if len > 0 => {
                let i = r.below(len);
                v[i] ^= 1 << r.below(8);
            }
            1 if len > 0 => {
                let i = r.below(len);
                v[i] = r.next() as u8;
            }
            2 if len >= 4 => {
                // an interesting 16/32-bit value, little endian (ZIP fields)
                let x = INTERESTING[r.below(INTERESTING.len())];
                let w = if r.chance(2) { 2 } else { 4 };
                let i = r.below(len - w + 1);
                v[i..i + w].copy_from_slice(&x.to_le_bytes()[..w]);
            }
            3 if len > 1 => v.truncate(r.below(len)),
            4 if len > 0 => {
                let (a, n) = (r.below(len), 1 + r.below(64));
                v.drain(a..(a + n).min(len));
            }
            5 if len > 0 => {
                // duplicate a chunk somewhere
                let (a, n) = (r.below(len), 1 + r.below(256));
                let chunk = v[a..(a + n).min(len)].to_vec();
                let at = r.below(v.len() + 1);
                v.splice(at..at, chunk);
            }
            6 if !other.is_empty() => {
                // splice with another input
                let cut = r.below(v.len() + 1);
                let from = r.below(other.len());
                v.truncate(cut);
                v.extend_from_slice(&other[from..]);
            }
            7 if len > 0 => {
                let at = r.below(len + 1);
                let n = 1 + r.below(16);
                let junk: Vec<u8> = (0..n).map(|_| r.next() as u8).collect();
                v.splice(at..at, junk);
            }
            _ => {
                // a signature where it doesn't belong
                let sig: &[u8] = [&b"PK\x03\x04"[..], b"PK\x01\x02", b"PK\x05\x06", b"PK\x06\x06"][r.below(4)];
                if len >= 4 {
                    let i = r.below(len - 3);
                    v[i..i + 4].copy_from_slice(sig);
                }
            }
        }
    }
    v
}

const JSON_TOKENS: &[&str] = &[
    "0",
    "-1",
    "1e39",
    "-1e39",
    "4294967296",
    "18446744073709551615",
    "1.5",
    "null",
    "true",
    "\"\"",
    "[]",
    "{}",
    "[[[[[[[[",
    "\"Rect\"",
    "\"Pen\"",
    "{\"x\":0,\"y\":0}",
    "\"\\u0000\"",
    ",",
    ":",
    "\"next_id\"",
    "\"shapes\"",
];

fn mutate_json(r: &mut Rng, s: &str) -> String {
    let mut s = s.to_string();
    for _ in 0..1 + r.below(4) {
        if s.is_empty() {
            break;
        }
        // positions of numbers / tokens to replace
        let at = r.below(s.len());
        let at = (0..=at).rev().find(|&i| s.is_char_boundary(i)).unwrap_or(0);
        let end = s[at..]
            .char_indices()
            .find(|(_, c)| matches!(c, ',' | '}' | ']' | ':'))
            .map_or(s.len(), |(i, _)| at + i);
        match r.below(4) {
            0 => s.replace_range(at..end, JSON_TOKENS[r.below(JSON_TOKENS.len())]),
            1 => s.replace_range(at..end, ""),
            2 => {
                let piece = s[at..end].to_string();
                s.insert_str(end, &format!(",{piece}"));
            }
            _ => {
                let n = r.next() as i64 >> r.below(60);
                s.replace_range(at..end, &n.to_string());
            }
        }
    }
    s
}

fn structured(r: &mut Rng) -> Vec<u8> {
    let png = projects::tiny_png(projects::W, projects::H);
    let mut doc = String::from_utf8(projects::doc_json(&projects::sample_doc(projects::W, projects::H))).unwrap();
    let mut man = String::from_utf8(projects::manifest(png.len())).unwrap();
    if r.chance(4) {
        man = mutate_json(r, &man);
    } else {
        doc = mutate_json(r, &doc);
    }
    let deflate = |r: &mut Rng| r.chance(2);
    let (a, b, c) = (deflate(r), deflate(r), r.chance(5));
    projects::build(&[
        ("manifest.json", man.as_bytes(), a),
        ("document.json", doc.as_bytes(), b),
        ("original.png", &png, c),
    ])
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let secs: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(600);
    let seed: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0x5EED_F00D);
    let mut r = Rng(seed | 1);
    let out_dir = std::env::temp_dir().join("rview-fuzz");
    let opts = DecodeOptions::default();
    std::panic::set_hook(Box::new(|info| eprintln!("PANIC: {info}")));

    let mut corpus: Vec<Vec<u8>> = projects::cases().into_iter().map(|(_, b)| b).collect();
    let seeds = corpus.len();
    let mut outcomes: HashSet<String> = HashSet::new();
    let (mut execs, mut crashes, mut accepted) = (0u64, 0u64, 0u64);
    let mut slowest = (Duration::ZERO, 0usize);
    let start = Instant::now();
    let deadline = start + Duration::from_secs(secs);
    let mut next_report = start + Duration::from_secs(60);
    while Instant::now() < deadline {
        let input = if r.chance(3) {
            structured(&mut r)
        } else {
            let a = corpus[r.below(corpus.len())].clone();
            let b = &corpus[r.below(corpus.len())];
            mutate_bytes(&mut r, a, b)
        };
        let t = Instant::now();
        let res = std::panic::catch_unwind(|| match rv_core::project::read(&input, &opts) {
            Ok(_) => "ok".to_string(),
            Err(e) => {
                // the message without quoted names and numbers: same kind of failure = same outcome
                let mut quoted = false;
                e.to_string()
                    .chars()
                    .filter(|c| {
                        quoted ^= *c == '"';
                        !quoted && *c != '"' && !c.is_ascii_digit()
                    })
                    .take(90)
                    .collect()
            }
        });
        let dt = t.elapsed();
        execs += 1;
        if dt > slowest.0 {
            slowest = (dt, input.len());
        }
        match res {
            Ok(outcome) => {
                if outcome == "ok" {
                    accepted += 1;
                }
                if outcomes.insert(outcome) && corpus.len() < 4000 {
                    corpus.push(input);
                }
            }
            Err(_) => {
                crashes += 1;
                let _ = std::fs::create_dir_all(&out_dir);
                let p = out_dir.join(format!("crash-{crashes}.rvm"));
                let _ = std::fs::write(&p, &input);
                eprintln!("crash saved to {}", p.display());
            }
        }
        if Instant::now() >= next_report {
            next_report += Duration::from_secs(60);
            println!(
                "{:>4} s  execs {execs}  ({:.0}/s)  outcomes {}  corpus {}  crashes {crashes}",
                start.elapsed().as_secs(),
                execs as f64 / start.elapsed().as_secs_f64(),
                outcomes.len(),
                corpus.len()
            );
        }
    }
    let el = start.elapsed().as_secs_f64();
    println!(
        "FUZZ project: {:.0} s, {execs} execs ({:.0}/s), seed {seed}, {seeds} seeds, {} distinct outcomes, \
         {accepted} accepted, slowest {:.1} ms ({} bytes), crashes {crashes}",
        el,
        execs as f64 / el,
        outcomes.len(),
        slowest.0.as_secs_f64() * 1000.0,
        slowest.1
    );
    // RVIEW_FUZZ_OUTCOMES=1: list every distinct outcome (tens of thousands after 10 minutes)
    if std::env::var_os("RVIEW_FUZZ_OUTCOMES").is_some() {
        let mut list: Vec<_> = outcomes.into_iter().collect();
        list.sort();
        for o in list {
            println!("  outcome: {o}");
        }
    }
    std::process::exit(i32::from(crashes > 0));
}
