use super::*;
use std::sync::mpsc;

#[test]
fn concurrent_children_share_capacity_and_release_on_drop() {
    let budget = Budget {
        capacity: 4,
        used: Mutex::new(0),
        available: Condvar::new(),
    };
    let cancel = CancelToken::new();
    let held = budget.acquire(3, &cancel).unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let permit = budget.acquire(2, &cancel).unwrap();
            tx.send(()).unwrap();
            assert_eq!(*budget.used.lock().unwrap(), 2);
            drop(permit);
        });
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
        assert_eq!(*budget.used.lock().unwrap(), 3);
        drop(held);
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
    });
    assert_eq!(*budget.used.lock().unwrap(), 0);
}

#[test]
fn queued_deadline_never_consumes_capacity_or_extends_the_task() {
    let budget = Budget {
        capacity: 1,
        used: Mutex::new(0),
        available: Condvar::new(),
    };
    let cancel = CancelToken::new();
    let _held = budget.acquire(1, &cancel).unwrap();
    let start = Instant::now();
    let error = budget
        .acquire_before(1, &cancel, Some(start + Duration::from_millis(10)))
        .err()
        .unwrap();
    assert!(error.contains("deadline"));
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(*budget.used.lock().unwrap(), 1);
}
#[test]
fn queued_cancellation_never_consumes_capacity() {
    let budget = Budget {
        capacity: 1,
        used: Mutex::new(0),
        available: Condvar::new(),
    };
    let held = budget.acquire(1, &CancelToken::new()).unwrap();
    let cancel = CancelToken::new();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| budget.acquire(1, &cancel).err().unwrap());
        cancel.cancel();
        assert!(worker.join().unwrap().contains("cancelled"));
    });
    assert_eq!(*budget.used.lock().unwrap(), 1);
    drop(held);
    assert_eq!(*budget.used.lock().unwrap(), 0);
}
