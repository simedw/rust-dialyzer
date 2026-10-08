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
const WORKER_STACK_BYTES: usize = 1 << 30;

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
