//! Opt-in throughput benchmark for the Analyze (scan) path.
//!
//! Not a test: never run under `cargo test` or CI. Run by hand, in release
//! (a debug build is not representative):
//!
//!     cargo run --release --example scan_bench
//!
//! It loads the rule catalogue exactly as the application does at startup
//! (`commands::catalogue`: native rules from `rules.toml`, then the Winapp2
//! rules whose application is detected on this machine) and scans every rule
//! in it through `commands::scan_rules_with`, the same concurrent
//! orchestration the `scan` Tauri command calls, driving `scan::scan_rule` per
//! rule exactly as production does. This is READ-ONLY: it only walks
//! directories and reads file metadata (plus one `SHQueryRecycleBinW` call for
//! the recycle-bin rule), and never deletes or modifies anything on disk.

use std::sync::Mutex;
use std::time::{Duration, Instant};
use wincleaner_lib::commands::{catalogue, scan_rules_with};
use wincleaner_lib::scan::scan_rule;

fn main() {
    println!("Analyze (scan) benchmark\n");

    let load_start = Instant::now();
    let cat = catalogue().expect("failed to build the rule catalogue");
    let load_elapsed = load_start.elapsed();
    println!(
        "catalogue load: {:.3}s ({} native, {} winapp2 detected, {} winapp2 dropped)",
        load_elapsed.as_secs_f64(),
        cat.summary.native,
        cat.summary.winapp2_detected,
        cat.summary.winapp2_dropped
    );
    println!("total rules to scan: {}\n", cat.rules.len());

    // Per-rule timing, taken from inside the same worker threads
    // `scan_rules_with` spawns: this is the real concurrent wall time, not a
    // second, separate sequential pass.
    let timings: Mutex<Vec<(String, Duration, u64, u32)>> = Mutex::new(Vec::new());
    let scan_start = Instant::now();
    scan_rules_with(
        &cat.rules,
        |rule| {
            let t = Instant::now();
            match scan_rule(rule) {
                Ok(res) => {
                    timings.lock().unwrap().push((
                        rule.id.clone(),
                        t.elapsed(),
                        res.file_count,
                        res.skipped,
                    ));
                    Ok(res)
                }
                Err(e) => {
                    // A machine-specific load error (missing var, outside
                    // profile) is expected for some rules; report it and move
                    // on, exactly as `scan_rules_with` does per-rule in
                    // `commands.rs`.
                    eprintln!("  {} failed: {e}", rule.id);
                    Err(e.to_string())
                }
            }
        },
        &mut |_step| {},
    )
    .expect("scan_rules_with failed");
    let scan_elapsed = scan_start.elapsed();

    let mut timings = timings.into_inner().unwrap();
    println!(
        "total scan time: {:.3}s over {} rules (wall clock, concurrent)\n",
        scan_elapsed.as_secs_f64(),
        timings.len()
    );

    timings.sort_by_key(|t| std::cmp::Reverse(t.1));
    println!("10 slowest rules:");
    for (id, elapsed, file_count, skipped) in timings.iter().take(10) {
        println!(
            "  {:<40} {:>8.3}s  {:>8} files  {:>5} skipped",
            id,
            elapsed.as_secs_f64(),
            file_count,
            skipped
        );
    }

    println!("\nNothing was deleted or modified: this benchmark only scans.");
}
