//! The scheduler: N worker threads pull the next script, or the next batch
//! of scripts, from one shared queue as they finish (no shards fixed up
//! front); the calling thread gets every start, event and result as a
//! message, in arrival order.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::batch::{event_script, judge_batch};
use crate::config::Batch;
use crate::discover::ScriptInfo;
use crate::events::Event;
use crate::verdict::{ProcessRun, Verdict, judge};
use crate::worker::{RunSpec, STOP, log_path, run_batch, run_script};

/// Which scripts may share a process, and how many per process.
#[derive(Debug, Clone, Default)]
pub struct Batching {
    pub batchable: HashSet<String>,
    pub max_scripts: usize,
    /// 0: no time budget.
    pub max_seconds: f64,
    /// Timings-file seconds by script (the budget's input).
    pub seconds: HashMap<String, f64>,
    /// `[batch] separate_dirs`: script -> index of its `dirs` entry. A batch
    /// takes only scripts of its first script's group; scripts with no
    /// entry (batchable under --mode batch) form one group of their own.
    /// Empty: no grouping.
    pub groups: HashMap<String, usize>,
}

/// The `groups` of `Batching` for `[batch] separate_dirs`: each script under
/// a `dirs` entry mapped to that entry's index (the first that matches).
/// Empty when `separate_dirs` is off.
pub fn batch_groups(batch: &Batch, scripts: &[String]) -> HashMap<String, usize> {
    if !batch.separate_dirs {
        return HashMap::new();
    }
    scripts
        .iter()
        .filter_map(|path| batch.dir_entry(path).map(|i| (path.clone(), i)))
        .collect()
}

impl Batching {
    /// Every script in its own process.
    pub fn none() -> Batching {
        Batching {
            max_scripts: 1,
            ..Batching::default()
        }
    }
}

/// The next unit of work: the queue's front, plus (when it is batchable) the
/// next batchable scripts in queue order while the batch stays within
/// `max_scripts` and the time budget. Scripts it skips keep their places.
pub fn take_batch(queue: &mut VecDeque<String>, b: &Batching) -> Vec<String> {
    let Some(first) = queue.pop_front() else {
        return Vec::new();
    };
    if !b.batchable.contains(&first) {
        return vec![first];
    }
    let cost = |s: &str| b.seconds.get(s).copied().unwrap_or(0.0);
    let group = b.groups.get(&first).copied();
    let mut total = cost(&first);
    let mut batch = vec![first];
    let mut i = 0;
    while i < queue.len() && batch.len() < b.max_scripts {
        let next = &queue[i];
        if !b.batchable.contains(next) || b.groups.get(next).copied() != group {
            i += 1;
            continue;
        }
        let t = cost(next);
        if b.max_seconds > 0.0 && total + t > b.max_seconds {
            i += 1;
            continue;
        }
        total += t;
        batch.push(queue.remove(i).expect("index in range"));
    }
    batch
}

#[derive(Debug, Clone)]
pub struct ScriptResult {
    pub script: String,
    pub slot: usize,
    pub verdict: Verdict,
    pub run: ProcessRun,
    pub log: PathBuf,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Started {
        slot: usize,
        script: String,
    },
    Event {
        slot: usize,
        event: Event,
    },
    Finished {
        slot: usize,
        result: Box<ScriptResult>,
    },
}

/// Runs `order` (project-relative script paths) on `workers` threads, each
/// taking its next unit of work with `take_batch`: one script in its own
/// process, or a batch in one process (`run_batch`, `judge_batch`), whose
/// never-started scripts go back to the queue's front. `on_msg` runs on the
/// calling thread for every message; `on_idle` every 100 ms without one
/// (the TUI's clock). Returns the results in finish order.
pub fn run_all(
    spec: &RunSpec,
    scripts: &[ScriptInfo],
    order: Vec<String>,
    workers: usize,
    batching: &Batching,
    on_msg: &mut dyn FnMut(&Msg),
    on_idle: &mut dyn FnMut(),
) -> Vec<ScriptResult> {
    let infos: Arc<HashMap<String, ScriptInfo>> = Arc::new(
        scripts
            .iter()
            .map(|s| (s.path.clone(), s.clone()))
            .collect(),
    );
    let workers = workers.max(1).min(order.len().max(1));
    let queue = Arc::new(Mutex::new(order.into_iter().collect::<VecDeque<_>>()));
    let (tx, rx) = mpsc::channel::<Msg>();
    let mut handles = Vec::new();
    for slot in 0..workers {
        let queue = Arc::clone(&queue);
        let infos = Arc::clone(&infos);
        let tx = tx.clone();
        let spec = spec.clone();
        let batching = batching.clone();
        handles.push(std::thread::spawn(move || {
            loop {
                if STOP.load(Ordering::SeqCst) {
                    break;
                }
                let batch = take_batch(&mut queue.lock().expect("queue lock"), &batching);
                if batch.len() > 1 {
                    let requeue = run_one_batch(&spec, slot, &batch, &infos, &tx);
                    if !STOP.load(Ordering::SeqCst) {
                        let mut q = queue.lock().expect("queue lock");
                        for s in requeue.into_iter().rev() {
                            q.push_front(s);
                        }
                    }
                    continue;
                }
                let Some(script) = batch.into_iter().next() else {
                    break;
                };
                let _ = tx.send(Msg::Started {
                    slot,
                    script: script.clone(),
                });
                let ev_tx = tx.clone();
                let run = run_script(&spec, slot, &script, &mut |e: &Event| {
                    let _ = ev_tx.send(Msg::Event {
                        slot,
                        event: e.clone(),
                    });
                });
                let info = infos.get(&script).expect("queued scripts are discovered");
                let verdict = judge(info, &run);
                let result = ScriptResult {
                    log: log_path(&spec, &script),
                    script,
                    slot,
                    verdict,
                    run,
                };
                let _ = tx.send(Msg::Finished {
                    slot,
                    result: Box::new(result),
                });
            }
        }));
    }
    drop(tx);
    let mut results = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(msg) => {
                on_msg(&msg);
                if let Msg::Finished { result, .. } = msg {
                    results.push(*result);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => on_idle(),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    for h in handles {
        let _ = h.join();
    }
    results
}

/// One batch on one worker: a `Started` message as each script's first
/// event arrives, then a `Finished` per judged script. Returns the scripts
/// that never started.
fn run_one_batch(
    spec: &RunSpec,
    slot: usize,
    batch: &[String],
    infos: &HashMap<String, ScriptInfo>,
    tx: &mpsc::Sender<Msg>,
) -> Vec<String> {
    let mut current = batch[0].clone();
    let _ = tx.send(Msg::Started {
        slot,
        script: current.clone(),
    });
    let (run, marks) = run_batch(spec, slot, batch, &mut |e: &Event| {
        if let Some(s) = event_script(e)
            && s != current
            && batch.iter().any(|b| b == s)
        {
            current = s.to_string();
            let _ = tx.send(Msg::Started {
                slot,
                script: current.clone(),
            });
        }
        let _ = tx.send(Msg::Event {
            slot,
            event: e.clone(),
        });
    });
    let judged = judge_batch(batch, infos, &run, &marks);
    for r in judged.results {
        let _ = tx.send(Msg::Finished {
            slot,
            result: Box::new(ScriptResult {
                log: log_path(spec, &r.script),
                script: r.script,
                slot,
                verdict: r.verdict,
                run: r.run,
            }),
        });
    }
    judged.requeue
}
