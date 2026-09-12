//! Opt-in throughput benchmark for the Recycle Bin deletion path.
//!
//! Not a test: never run under `cargo test` or CI. Run by hand:
//!
//!     cargo run --release --example trash_bench [file_count]
//!
//! It creates its own small files under a `tempfile::TempDir` and measures
//! two strategies on separate fresh fixtures: one `trash::delete` per file,
//! and `trash::delete_all` in batches of `wincleaner_lib::clean::TRASH_BATCH`.
//! Every file this sends to the Recycle Bin is one this example created; the
//! user may empty the bin afterwards.

use std::path::PathBuf;
use std::time::Instant;
use wincleaner_lib::clean::TRASH_BATCH;

fn make_fixture(count: usize) -> (tempfile::TempDir, Vec<PathBuf>) {
    let dir = tempfile::TempDir::new().expect("could not create temp dir");
    let paths: Vec<PathBuf> = (0..count)
        .map(|n| {
            let path = dir.path().join(format!("bench_{n:06}.txt"));
            std::fs::write(&path, b"bench").expect("could not write fixture file");
            path
        })
        .collect();
    (dir, paths)
}

fn report(label: &str, files: usize, elapsed: std::time::Duration) -> f64 {
    let secs = elapsed.as_secs_f64();
    let rate = files as f64 / secs;
    println!("{label}: {files} files in {secs:.3}s = {rate:.1} files/s");
    rate
}

fn main() {
    let count: usize = std::env::args()
        .nth(1)
        .map(|s| s.parse().expect("file_count must be a number"))
        .unwrap_or(2000);

    println!("Recycle Bin throughput benchmark: {count} files per strategy\n");

    let (per_file_dir, per_file_paths) = make_fixture(count);
    let start = Instant::now();
    for path in &per_file_paths {
        trash::delete(path).expect("trash::delete failed");
    }
    let per_file_elapsed = start.elapsed();
    let per_file_rate = report("per-file (trash::delete)", count, per_file_elapsed);
    drop(per_file_dir);

    let (batched_dir, batched_paths) = make_fixture(count);
    let start = Instant::now();
    for batch in batched_paths.chunks(TRASH_BATCH) {
        trash::delete_all(batch).expect("trash::delete_all failed");
    }
    let batched_elapsed = start.elapsed();
    let batched_rate = report(
        &format!("batched (trash::delete_all, batches of {TRASH_BATCH})"),
        count,
        batched_elapsed,
    );
    drop(batched_dir);

    println!("\nspeed-up: {:.2}x", batched_rate / per_file_rate);
    println!(
        "\n{} items were sent to the Recycle Bin by this run; you may empty it.",
        count * 2
    );
}
