use std::{any::Any, collections::BTreeMap, panic::{AssertUnwindSafe, catch_unwind, resume_unwind}, sync::{Condvar, Mutex}};

enum Failure<E> {
    Error(E),
    Panic(Box<dyn Any + Send>),
}

fn completion<T, E>(operation: impl FnOnce() -> Result<T, E>) -> Result<T, Failure<E>> {
    catch_unwind(AssertUnwindSafe(operation))
        .map_err(Failure::Panic)?
        .map_err(Failure::Error)
}

struct Prefetch<T, E> {
    next: usize,
    consumed: usize,
    cancelled: bool,
    ready: BTreeMap<usize, Result<T, Failure<E>>>,
}

// The window includes active, ready and currently consumed work.
// Reached failures are exposed only after all speculative workers join.
/// Stack for prefetch workers. Reserved virtual memory; pages are only touched
/// by unusually deep guards.
pub(crate) const WORKER_STACK_BYTES: usize = 1 << 30;

pub fn ordered_prefetch<T: Send, U, E: Send>(
    count: usize,
    workers: usize,
    prepare: impl Fn(usize) -> Result<T, E> + Sync,
    mut consume: impl FnMut(usize, T) -> Result<U, E>,
) -> Result<Vec<U>, E> {
    let workers = workers.max(1).min(count);
    let state = Mutex::new(Prefetch {
        next: 0,
        consumed: 0,
        cancelled: false,
        ready: BTreeMap::new(),
    });
    let changed = Condvar::new();
    let result = std::thread::scope(|scope| {
        for _ in 0..workers {
            let state = &state;
            let changed = &changed;
            let prepare = &prepare;
            // Workers run guard and body analysis, which recurses as deep as
            // guard_control::MAX_GUARD_DEPTH; the 2 MB default stack is too small.
            std::thread::Builder::new()
                .stack_size(WORKER_STACK_BYTES)
                .spawn_scoped(scope, move || {
                loop {
                    let index = {
                        let mut state = state.lock().unwrap();
                        while !state.cancelled
                            && state.next < count
                            && state.next >= state.consumed + workers
                        {
                            state = changed.wait(state).unwrap();
                        }
                        if state.cancelled || state.next == count {
                            return;
                        }
                        let index = state.next;
                        state.next += 1;
                        index
                    };
                    let result = completion(|| prepare(index));
                    let mut state = state.lock().unwrap();
                    if state.cancelled {
                        return;
                    }
                    state.ready.insert(index, result);
                    changed.notify_all();
                }
                })
                .expect("spawn ordered_prefetch worker");
        }
        let mut output = Vec::with_capacity(count);
        for index in 0..count {
            let prepared = {
                let mut state = state.lock().unwrap();
                loop {
                    if let Some(result) = state.ready.remove(&index) {
                        break result;
                    }
                    state = changed.wait(state).unwrap();
                }
            };
            match prepared.and_then(|prepared| completion(|| consume(index, prepared))) {
                Ok(value) => output.push(value),
                Err(error) => {
                    let mut state = state.lock().unwrap();
                    state.cancelled = true;
                    state.ready.clear();
                    changed.notify_all();
                    return Err(error);
                }
            }
            let mut state = state.lock().unwrap();
            state.consumed = index + 1;
            changed.notify_all();
        }
        Ok(output)
    });
    // All producers have joined before exposing the reached slot's failure.
    match result {
        Ok(output) => Ok(output),
        Err(Failure::Error(error)) => Err(error),
        Err(Failure::Panic(payload)) => resume_unwind(payload),
    }
}


/// Applies `f` to every item on all cores; results keep input order.
pub fn parallel_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(items.len());
    if workers <= 1 {
        return items.iter().map(f).collect();
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut results: Vec<Option<R>> = (0..items.len()).map(|_| None).collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                std::thread::Builder::new()
                    .stack_size(WORKER_STACK_BYTES)
                    .spawn_scoped(scope, || {
                        let mut done = Vec::new();
                        loop {
                            let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(item) = items.get(i) else { break };
                            done.push((i, f(item)));
                        }
                        done
                    })
                    .expect("spawn analysis worker")
            })
            .collect();
        for handle in handles {
            for (i, r) in handle.join().expect("analysis worker panicked") {
                results[i] = Some(r);
            }
        }
    });
    results.into_iter().map(|r| r.expect("every item analyzed")).collect()
}

/// Like `setup_map`, but consumes `items` and starts the costliest first so a
/// large item does not start last and extend the phase. Each item is dropped
/// on the worker that processed it. Results keep input order.
pub fn setup_map_owned_by_cost<T: Send, R: Send>(
    items: Vec<T>,
    cost: &[u64],
    f: impl Fn(T) -> R + Sync,
) -> Vec<R> {
    assert_eq!(items.len(), cost.len());
    if sequential() {
        return items.into_iter().map(f).collect();
    }
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(cost[i]));
    let slots: Vec<Mutex<Option<T>>> = items.into_iter().map(|item| Mutex::new(Some(item))).collect();
    parallel_map(&order, |&i| {
        let item = slots[i].lock().unwrap().take().expect("each item taken once");
        (i, f(item))
    })
    .into_iter()
    .fold((0..slots.len()).map(|_| None).collect::<Vec<Option<R>>>(), |mut out, (i, r)| {
        out[i] = Some(r);
        out
    })
    .into_iter()
    .map(|r| r.expect("every item processed"))
    .collect()
}

/// `RUST_DIALYZER_SEQUENTIAL=1` keeps setup and analysis on one thread.
pub fn sequential() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RUST_DIALYZER_SEQUENTIAL").is_some_and(|v| v == "1"))
}

/// `parallel_map`, or a plain ordered map when `sequential()`.
pub fn setup_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    if sequential() {
        items.iter().map(f).collect()
    } else {
        parallel_map(items, f)
    }
}

/// Runs `a` on the calling thread while `b` runs on a worker. When
/// `sequential()`, runs `a` and then `b`.
pub fn join<A, B: Send>(a: impl FnOnce() -> A, b: impl FnOnce() -> B + Send) -> (A, B) {
    if sequential() {
        let a = a();
        return (a, b());
    }
    std::thread::scope(|scope| {
        let handle = std::thread::Builder::new()
            .stack_size(WORKER_STACK_BYTES)
            .spawn_scoped(scope, b)
            .expect("spawn setup worker");
        let a = a();
        match handle.join() {
            Ok(b) => (a, b),
            Err(payload) => resume_unwind(payload),
        }
    })
}

/// What a dependency-scheduled job produced.
pub enum Attempt<R> {
    Done(R),
    /// The job observed state of these earlier jobs before they committed;
    /// run it again once they have.
    Retry(Vec<usize>),
}

/// Jobs with dependency edges, for `dependency_map`.
pub struct JobGraph<'a> {
    /// Jobs that wait for each job; every caller has a larger index.
    pub callers: &'a [Vec<usize>],
    /// Ready jobs with higher priority start first.
    pub priority: &'a [u64],
    /// Failure order: the reported error is the one with the lowest rank.
    pub rank: &'a [u64],
}

enum Message<R> {
    Finished(usize, std::thread::Result<Attempt<R>>),
    Skipped,
}

struct Ready {
    heap: std::collections::BinaryHeap<(u64, std::cmp::Reverse<usize>)>,
    cutoff: u64,
    shutdown: bool,
}

/// Runs `run` on every job once all jobs it depends on (the jobs that list
/// it as a caller) have committed, on all cores. `commit` runs on the calling
/// thread, one job at a time, in completion order. Once a job fails, jobs of
/// higher rank are no longer started and the failure of lowest rank among the
/// jobs below it is returned, as if jobs had run one rank at a time.
/// `failed` is a failure known before any job runs.
pub fn dependency_map<R: Send, C>(
    graph: JobGraph<'_>,
    failed: Option<(u64, C)>,
    run: impl Fn(usize) -> Attempt<R> + Sync,
    mut commit: impl FnMut(usize, R) -> Result<(), C>,
) -> Result<(), C> {
    let count = graph.callers.len();
    let mut remaining = vec![0usize; count];
    for callers in graph.callers {
        for &caller in callers {
            remaining[caller] += 1;
        }
    }
    let mut first_error = failed;
    let cutoff = |first_error: &Option<(u64, C)>| first_error.as_ref().map_or(u64::MAX, |(rank, _)| *rank);
    let ready = Mutex::new(Ready {
        heap: Default::default(),
        cutoff: cutoff(&first_error),
        shutdown: false,
    });
    let changed = Condvar::new();
    let workers = std::thread::available_parallelism().map_or(1, usize::from).min(count).max(1);
    let (send, receive) = std::sync::mpsc::channel::<Message<R>>();
    let mut panic = None;
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let (ready, changed, run, send) = (&ready, &changed, &run, send.clone());
            std::thread::Builder::new()
                .stack_size(WORKER_STACK_BYTES)
                .spawn_scoped(scope, move || loop {
                    let job = {
                        let mut state = ready.lock().unwrap();
                        loop {
                            if state.shutdown {
                                return;
                            }
                            if let Some((_, std::cmp::Reverse(job))) = state.heap.pop() {
                                break (graph.rank[job] < state.cutoff).then_some(job);
                            }
                            state = changed.wait(state).unwrap();
                        }
                    };
                    let message = match job {
                        Some(job) => Message::Finished(job, catch_unwind(AssertUnwindSafe(|| run(job)))),
                        None => Message::Skipped,
                    };
                    if send.send(message).is_err() {
                        return;
                    }
                })
                .expect("spawn analysis worker");
        }
        let coordinate = || {
            let mut committed = vec![false; count];
            let mut waiting: Vec<Vec<usize>> = vec![Vec::new(); count];
            let mut blockers = vec![0usize; count];
            let push = |jobs: &mut dyn Iterator<Item = usize>| {
                let mut state = ready.lock().unwrap();
                let before = state.heap.len();
                for job in jobs {
                    state.heap.push((graph.priority[job], std::cmp::Reverse(job)));
                }
                let pushed = state.heap.len() - before;
                changed.notify_all();
                pushed
            };
            let mut in_flight = push(&mut (0..count).filter(|&job| remaining[job] == 0));
            while in_flight > 0 {
                let message = receive.recv().expect("analysis workers alive");
                in_flight -= 1;
                let (job, attempt) = match message {
                    Message::Skipped => continue,
                    Message::Finished(_, Err(payload)) => {
                        panic.get_or_insert(payload);
                        ready.lock().unwrap().cutoff = 0;
                        continue;
                    }
                    Message::Finished(job, Ok(attempt)) => (job, attempt),
                };
                let result = match attempt {
                    Attempt::Retry(after) => {
                        let pending: Vec<usize> = after.into_iter().filter(|&o| !committed[o]).collect();
                        if pending.is_empty() {
                            in_flight += push(&mut std::iter::once(job));
                        } else {
                            blockers[job] = pending.len();
                            for o in pending {
                                waiting[o].push(job);
                            }
                        }
                        continue;
                    }
                    Attempt::Done(result) => result,
                };
                if let Err(error) = commit(job, result) {
                    if graph.rank[job] < cutoff(&first_error) {
                        first_error = Some((graph.rank[job], error));
                        ready.lock().unwrap().cutoff = graph.rank[job];
                    }
                    continue;
                }
                committed[job] = true;
                let mut unblocked = Vec::new();
                for &caller in &graph.callers[job] {
                    remaining[caller] -= 1;
                    if remaining[caller] == 0 {
                        unblocked.push(caller);
                    }
                }
                for waiter in std::mem::take(&mut waiting[job]) {
                    blockers[waiter] -= 1;
                    if blockers[waiter] == 0 {
                        unblocked.push(waiter);
                    }
                }
                let limit = cutoff(&first_error);
                in_flight += push(&mut unblocked.into_iter().filter(|&job| graph.rank[job] < limit));
            }
            assert!(
                panic.is_some() || first_error.is_some() || committed.iter().all(|&done| done),
                "dependency schedule stalled"
            );
        };
        let coordinated = catch_unwind(AssertUnwindSafe(coordinate));
        ready.lock().unwrap().shutdown = true;
        changed.notify_all();
        if let Err(payload) = coordinated {
            panic.get_or_insert(payload);
        }
    });
    if let Some(payload) = panic {
        resume_unwind(payload);
    }
    match first_error {
        Some((_, error)) => Err(error),
        None => Ok(()),
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn ordered_consumption_bounds_prepared_state() {
        let started = AtomicUsize::new(0);
        let consumed = AtomicUsize::new(0);
        let output = ordered_prefetch(
            100,
            4,
            |index| -> Result<usize, String> {
                let outstanding =
                    started.fetch_add(1, Ordering::SeqCst) + 1 - consumed.load(Ordering::SeqCst);
                assert!(outstanding <= 4);
                Ok(index)
            },
            |index, value| {
                assert_eq!(index, value);
                consumed.fetch_add(1, Ordering::SeqCst);
                Ok(value)
            },
        )
        .unwrap();
        assert_eq!(output, (0..100).collect::<Vec<_>>());
    }

    #[test]
    fn first_error_preserves_consumer_prefix_and_cancels() {
        for consumer_error in [false, true] {
            let started = AtomicUsize::new(0);
            let mut prefix = vec![];
            let result = ordered_prefetch(
                100,
                4,
                |index| -> Result<usize, String> {
                    started.fetch_add(1, Ordering::SeqCst);
                    if !consumer_error && index == 2 {
                        Err("first".into())
                    } else if index == 3 {
                        Err("later".into())
                    } else {
                        Ok(index)
                    }
                },
                |index, _| {
                    if consumer_error && index == 2 {
                        return Err("first".into());
                    }
                    prefix.push(index);
                    Ok(index)
                },
            );
            assert_eq!(result, Err("first".into()));
            assert_eq!(prefix, [0, 1]);
            assert!(started.load(Ordering::SeqCst) <= 6);
        }
    }

    #[test]
    fn dependency_map_runs_after_callees_retries_and_reports_lowest_rank() {
        // 0 and 1 are leaves, 2 calls both, 3 calls 2; 4 is independent.
        let callers = vec![vec![2], vec![2], vec![3], vec![], vec![]];
        let priority = [1, 5, 3, 2, 0];
        let rank = [0, 1, 5, 6, 2];
        let graph = || JobGraph { callers: &callers, priority: &priority, rank: &rank };
        let committed: Vec<std::sync::atomic::AtomicBool> = (0..5).map(|_| Default::default()).collect();
        let retried = std::sync::atomic::AtomicBool::new(false);
        let mut order = vec![];
        dependency_map(
            graph(),
            None,
            |job| {
                let seen = |j: usize| committed[j].load(Ordering::SeqCst);
                match job {
                    2 => assert!(seen(0) && seen(1)),
                    3 => assert!(seen(2)),
                    _ => {}
                }
                if job == 3 && !seen(4) && !retried.swap(true, Ordering::SeqCst) {
                    return Attempt::Retry(vec![4]);
                }
                Attempt::Done(job)
            },
            |job, value| -> Result<(), String> {
                assert_eq!(job, value);
                committed[job].store(true, Ordering::SeqCst);
                order.push(job);
                Ok(())
            },
        )
        .unwrap();
        order.sort();
        assert_eq!(order, [0, 1, 2, 3, 4]);
        // Failures at ranks 6 and 1: rank 1 wins and job 3 never starts after it.
        let result = dependency_map(
            graph(),
            Some((6, "prepared".to_string())),
            |job| Attempt::Done(job),
            |job, _| if job == 1 { Err(format!("job {job}")) } else { Ok(()) },
        );
        assert_eq!(result, Err("job 1".into()));
    }
    #[test]
    fn empty_and_duplicate_inputs_keep_order() {
        assert_eq!(ordered_prefetch(0, 4, |_| Ok::<usize, String>(1), |_, x| Ok(x)), Ok(vec![]));
        assert_eq!(
            ordered_prefetch(3, 8, |_| Ok::<usize, String>(1), |_, x| Ok(x)),
            Ok(vec![1, 1, 1])
        );
    }
    #[test]
    fn panic_completion_joins_without_losing_ordered_errors() {
        let (done, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for producer in [true, false] {
                let mut prefix = vec![];
                let panic = catch_unwind(AssertUnwindSafe(|| {
                    ordered_prefetch(
                        100,
                        4,
                        |index| -> Result<usize, String> {
                            if producer && index == 2 {
                                panic!("reached panic");
                            }
                            Ok(index)
                        },
                        |index, value| {
                            if !producer && index == 2 {
                                panic!("reached panic");
                            }
                            prefix.push(index);
                            Ok(value)
                        },
                    )
                }));
                assert_eq!(
                    *panic.unwrap_err().downcast::<&str>().unwrap(),
                    "reached panic"
                );
                assert_eq!(prefix, [0, 1]);
            }
            let result = ordered_prefetch(
                100,
                4,
                |index| -> Result<usize, String> {
                    if index == 3 {
                        panic!("later panic");
                    }
                    if index == 2 {
                        Err("first error".into())
                    } else {
                        Ok(index)
                    }
                },
                |_, value| Ok(value),
            );
            assert_eq!(result, Err("first error".into()));
            done.send(()).unwrap();
        });
        receive
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("panic completion must not hang");
    }
}
