//! Live output. On a terminal: a redrawn block with a progress bar and one
//! pane per worker, failures scrolling above it as they happen. Otherwise
//! (a pipe, a file, an agent, CI): plain lines, no escape codes.

use std::io::Write;
use std::time::{Duration, Instant};

use crate::events::{Event, Status, strip_ansi};
use crate::run::Msg;
use crate::summary::failure_lines;
use crate::verdict::Outcome;

#[derive(Debug, Clone)]
struct Slot {
    script: Option<String>,
    test: Option<String>,
    since: Instant,
}

/// The counters both outputs show.
#[derive(Debug, Clone)]
pub struct Progress {
    pub total_scripts: usize,
    pub total_tests: usize,
    pub done_scripts: usize,
    pub passed: u64,
    pub failed: u64,
    pub other: u64,
    pub bad_scripts: usize,
    start: Instant,
    slots: Vec<Slot>,
}

impl Progress {
    pub fn new(total_scripts: usize, total_tests: usize, workers: usize) -> Progress {
        let now = Instant::now();
        Progress {
            total_scripts,
            total_tests,
            done_scripts: 0,
            passed: 0,
            failed: 0,
            other: 0,
            bad_scripts: 0,
            start: now,
            slots: vec![
                Slot {
                    script: None,
                    test: None,
                    since: now,
                };
                workers
            ],
        }
    }

    pub fn update(&mut self, msg: &Msg) {
        match msg {
            Msg::Started { slot, script } => {
                self.slots[*slot] = Slot {
                    script: Some(script.clone()),
                    test: None,
                    since: Instant::now(),
                };
            }
            Msg::Event { slot, event } => match event {
                Event::TestStart { test, inner, .. } => {
                    self.slots[*slot].test = Some(if inner.is_empty() {
                        test.clone()
                    } else {
                        format!("{inner}.{test}")
                    });
                }
                Event::TestEnd(t) => match t.status {
                    Status::Passed => self.passed += 1,
                    Status::Failed => self.failed += 1,
                    Status::Pending | Status::Risky => self.other += 1,
                },
                _ => {}
            },
            Msg::Finished { slot, result } => {
                self.done_scripts += 1;
                if result.verdict.outcome != Outcome::Passed {
                    self.bad_scripts += 1;
                }
                self.slots[*slot] = Slot {
                    script: None,
                    test: None,
                    since: Instant::now(),
                };
            }
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

pub trait Ui {
    fn handle(&mut self, msg: &Msg);
    fn tick(&mut self);
    fn finish(&mut self);
}

fn clock(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// Plain lines: one per finished script, and the failures as they happen.
pub struct Plain<W: Write> {
    out: W,
    progress: Progress,
}

impl<W: Write> Plain<W> {
    pub fn new(out: W, progress: Progress) -> Self {
        Plain { out, progress }
    }
}

impl<W: Write> Ui for Plain<W> {
    fn handle(&mut self, msg: &Msg) {
        self.progress.update(msg);
        if let Msg::Finished { result, .. } = msg {
            let p = &self.progress;
            let _ = writeln!(
                self.out,
                "{:<8} {:>6.1} s  {}  ({} tests)  [{}/{} scripts, {}]",
                result.verdict.outcome.label(),
                result.run.seconds,
                result.script,
                result.verdict.counts.tests,
                p.done_scripts,
                p.total_scripts,
                clock(p.elapsed())
            );
            if result.verdict.outcome != Outcome::Passed {
                let _ = write!(self.out, "{}", failure_lines(result));
            }
            let _ = self.out.flush();
        }
    }

    fn tick(&mut self) {}

    fn finish(&mut self) {
        let _ = self.out.flush();
    }
}

/// The live terminal view.
pub struct Tui<W: Write> {
    out: W,
    progress: Progress,
    drawn: usize,
    scroll: Vec<String>,
    last_draw: Option<Instant>,
    width: usize,
}

impl<W: Write> Tui<W> {
    pub fn new(mut out: W, progress: Progress, width: usize) -> Self {
        let _ = write!(out, "\x1b[?25l");
        Tui {
            out,
            progress,
            drawn: 0,
            scroll: Vec::new(),
            last_draw: None,
            width: width.max(40),
        }
    }

    /// One terminal row: a longer line would wrap and break the redraw.
    fn fit(&self, line: &str) -> String {
        let visible = strip_ansi(line);
        if visible.chars().count() < self.width {
            line.to_string()
        } else {
            let mut s: String = visible.chars().take(self.width.saturating_sub(2)).collect();
            s.push('~');
            s
        }
    }

    fn live_lines(&self) -> Vec<String> {
        let p = &self.progress;
        let bar_width = 24;
        let filled = (p.done_scripts * bar_width)
            .checked_div(p.total_scripts)
            .unwrap_or(bar_width);
        let bar = format!("{}{}", "#".repeat(filled), "-".repeat(bar_width - filled));
        let fail_colour = if p.failed > 0 || p.bad_scripts > 0 {
            "\x1b[31m"
        } else {
            ""
        };
        let header = format!(
            "gdtest [{bar}] {}/{} scripts  \x1b[32m{} passed\x1b[0m  {fail_colour}{} failed\x1b[0m  {} pending/risky  {} problem scripts  {}",
            p.done_scripts,
            p.total_scripts,
            p.passed,
            p.failed,
            p.other,
            p.bad_scripts,
            clock(p.elapsed())
        );
        let mut lines = vec![self.fit(&header)];
        for (i, s) in p.slots.iter().enumerate() {
            let pane = match &s.script {
                Some(script) => format!(
                    "w{} {:>5.1}s {}{}",
                    i + 1,
                    s.since.elapsed().as_secs_f64(),
                    script,
                    s.test
                        .as_ref()
                        .map(|t| format!(" > {t}"))
                        .unwrap_or_default()
                ),
                None => format!("w{}  idle", i + 1),
            };
            lines.push(self.fit(&pane));
        }
        lines
    }

    fn draw(&mut self) {
        let mut frame = String::new();
        if self.drawn > 0 {
            frame += &format!("\x1b[{}F", self.drawn);
        }
        frame += "\x1b[J";
        for line in self.scroll.drain(..) {
            frame += &line;
            frame += "\n";
        }
        let live = self.live_lines();
        for line in &live {
            frame += line;
            frame += "\n";
        }
        self.drawn = live.len();
        let _ = self.out.write_all(frame.as_bytes());
        let _ = self.out.flush();
        self.last_draw = Some(Instant::now());
    }
}

impl<W: Write> Ui for Tui<W> {
    fn handle(&mut self, msg: &Msg) {
        self.progress.update(msg);
        match msg {
            Msg::Event {
                event: Event::TestEnd(t),
                ..
            } if t.status == Status::Failed => {
                let script = t.script.trim_start_matches("res://");
                let first = t.messages.first().map(String::as_str).unwrap_or("");
                let first = first.lines().next().unwrap_or("");
                self.scroll.push(format!(
                    "\x1b[31mFAIL\x1b[0m {script} > {}: {first}",
                    if t.inner.is_empty() {
                        t.test.clone()
                    } else {
                        format!("{}.{}", t.inner, t.test)
                    }
                ));
            }
            Msg::Finished { result, .. }
                if result.verdict.outcome != Outcome::Passed
                    && result.verdict.outcome != Outcome::Failed =>
            {
                self.scroll.push(format!(
                    "\x1b[31m{}\x1b[0m {}: {}",
                    result.verdict.outcome.label(),
                    result.script,
                    result.verdict.problems.first().cloned().unwrap_or_default()
                ));
            }
            _ => {}
        }
        let due = self
            .last_draw
            .is_none_or(|t| t.elapsed() >= Duration::from_millis(80));
        if due || !self.scroll.is_empty() {
            self.draw();
        }
    }

    fn tick(&mut self) {
        self.draw();
    }

    fn finish(&mut self) {
        self.draw();
        let _ = write!(self.out, "\x1b[?25h");
        let _ = self.out.flush();
    }
}

/// The terminal's width in columns, or None when stdout is not a terminal.
pub fn terminal_width() -> Option<usize> {
    // SAFETY: TIOCGWINSZ fills a winsize struct we own.
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
            Some(ws.ws_col as usize)
        } else {
            None
        }
    }
}
