use super::*;

#[test]
fn explicit_worker_budget_is_positive_and_capped_by_available_cpus() {
    assert_eq!(worker_parallelism(8, None), 8);
    assert_eq!(worker_parallelism(8, Some("2")), 2);
    assert_eq!(worker_parallelism(8, Some("20")), 8);
    for invalid in ["0", "bad", "-1", " 2", "184467440737095516160"] {
        assert_eq!(worker_parallelism(8, Some(invalid)), 8, "{invalid}");
    }
    assert_eq!(worker_parallelism(0, None), 1);
    assert_eq!(worker_parallelism(0, Some("2")), 1);
}

#[test]
fn concurrency_preserves_explicit_bounds_and_the_host_default() {
    assert_eq!(Concurrency::new(0), Concurrency::serial());
    assert_eq!(Concurrency::new(2).get(), 2);
    assert_eq!(Concurrency::new(1024).get(), MAX_CONCURRENCY);
    assert_eq!(Concurrency::new(usize::MAX).get(), MAX_CONCURRENCY);
    assert_eq!(Concurrency::default().get(), compute_parallelism());
}
