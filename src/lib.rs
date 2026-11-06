//! gdtest-runner: a vitest-like parallel runner for Godot + GUT suites.
//!
//! One fresh headless Godot process per test script (or per batch of
//! scripts, judged per script), a pool of workers fed by a dynamic queue
//! (slowest first), live events from a GDScript reporter,
//! a cross-check of every process against GUT's own summary and against
//! static discovery, and one summary at the end. See README.md.

pub mod batch;
pub mod config;
pub mod discover;
pub mod events;
pub mod run;
pub mod summary;
pub mod timings;
pub mod ui;
pub mod verdict;
pub mod worker;
