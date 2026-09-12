//! Opt-in throughput benchmark for the Space measurement.
//!
//! Not a test: never run under `cargo test` or CI. Run by hand, in release
//! (a debug build is not representative):
//!
//!     cargo run --release --example space_bench
//!
//! It resolves the same known folders the Space screen measures
//! (`space::known_folder_roots`, i.e. `SHGetKnownFolderPath` for Downloads,
//! Desktop, Documents, Pictures, Videos and Music), times each one on its own
//! through `space::walk_root` — the very function the screen runs, one thread
//! per root — and then reports the concurrent wall time of the whole
//! measurement through `space::scan_space_with`. This is READ-ONLY: it walks
//! directories and reads file metadata, and never deletes or modifies anything.

use std::time::Instant;
use wincleaner_lib::rules::{canonical_profile_with, system_env};
use wincleaner_lib::space::{known_folder_roots, scan_space_with, walk_root};

fn main() {
    println!("Space measurement benchmark\n");

    let profile = canonical_profile_with(&system_env).expect("could not resolve the profile");
    let roots = known_folder_roots();
    println!("{} known folders resolved\n", roots.len());

    println!("per root (sequential, one at a time):");
    let mut sequential = std::time::Duration::ZERO;
    for (name, path) in &roots {
        let start = Instant::now();
        let scan = walk_root(name, path, &profile);
        let elapsed = start.elapsed();
        sequential += elapsed;
        let state = if scan.refused {
            "REFUSED (outside the profile, or a reparse point)"
        } else if scan.missing {
            "absent"
        } else {
            "ok"
        };
        println!(
            "  {:<12} {:>8.3}s  {:>10} files  {:>12} bytes  {:>5} skipped  {}",
            name,
            elapsed.as_secs_f64(),
            scan.files,
            scan.bytes,
            scan.skipped,
            state
        );
        println!("               {path}");
    }
    println!("\nsum of the roots: {:.3}s", sequential.as_secs_f64());

    let start = Instant::now();
    let result = scan_space_with(&roots, &profile, &mut |_| {});
    let elapsed = start.elapsed();
    let files: u64 = result.roots.iter().map(|r| r.files).sum();
    let bytes: u64 = result.roots.iter().map(|r| r.bytes).sum();
    println!(
        "total measurement: {:.3}s (wall clock, one thread per root) over {} files, {} bytes",
        elapsed.as_secs_f64(),
        files,
        bytes
    );
    println!(
        "{} files ranked, {} folders ranked, {} entries skipped, {} roots refused",
        result.files.len(),
        result.folders.len(),
        result.skipped_files,
        result.skipped_roots.len()
    );

    println!("\nNothing was deleted or modified: this benchmark only measures.");
}
