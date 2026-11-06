//! What the faces and content passes share to use several cores (spec 037): how many threads they may take, and work
//! spread over them that comes back in its order.

/// The threads a pass may take besides its own (REQ-037-03).
pub fn threads() -> usize {
    threads_of(std::thread::available_parallelism().map_or(1, usize::from))
}

/// Half of `cores`, at least one: the rest is left to the UI, scanning and thumbnails (REQ-037-03).
pub fn threads_of(cores: usize) -> usize {
    (cores / 2).max(1)
}

/// `work` done on each of `items`, on up to `threads` threads at once; the results in the order of the items
/// (REQ-037-01, REQ-037-02). A panic in `work` goes on in the caller's thread.
pub fn map_in_order<T: Sync, R: Send>(items: &[T], threads: usize, work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    if threads <= 1 || items.len() <= 1 {
        return items.iter().map(work).collect();
    }
    let per_thread = items.len().div_ceil(threads);
    let work = &work;
    std::thread::scope(|scope| {
        let parts: Vec<_> = items
            .chunks(per_thread)
            .map(|part| scope.spawn(move || part.iter().map(work).collect::<Vec<R>>()))
            .collect();
        parts
            .into_iter()
            .flat_map(|part| part.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Mutex;

    #[test]
    fn req_037_01_keeps_the_order_of_the_items() {
        let items: Vec<u32> = (0..23).collect();
        for threads in [1, 2, 3, 8, 40] {
            let doubled = map_in_order(&items, threads, |item| item * 2);
            assert_eq!(
                doubled,
                items.iter().map(|item| item * 2).collect::<Vec<_>>(),
                "{threads}"
            );
        }
        assert!(map_in_order(&[] as &[u32], 4, |item| *item).is_empty());
    }

    #[test]
    fn req_037_02_runs_on_several_threads() {
        let seen = Mutex::new(HashSet::new());
        let items: Vec<u32> = (0..6).collect();
        map_in_order(&items, 3, |_| {
            seen.lock().unwrap().insert(std::thread::current().id());
            std::thread::sleep(std::time::Duration::from_millis(20));
        });
        assert_eq!(seen.lock().unwrap().len(), 3);
    }

    #[test]
    fn req_037_03_takes_at_most_half_the_cores() {
        assert_eq!(threads_of(0), 1);
        assert_eq!(threads_of(1), 1);
        assert_eq!(threads_of(2), 1);
        assert_eq!(threads_of(4), 2);
        assert_eq!(threads_of(7), 3);
        assert_eq!(threads_of(16), 8);
    }

    #[test]
    fn a_panic_reaches_the_caller() {
        let caught = std::panic::catch_unwind(|| {
            map_in_order(&[1, 2, 3, 4], 2, |item| {
                assert_ne!(*item, 3, "a hostile file");
                *item
            })
        });
        assert!(caught.is_err());
    }
}
