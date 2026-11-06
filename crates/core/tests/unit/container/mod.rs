use super::*;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

struct Shared(AtomicUsize);
struct Transient(u32);
struct First;
struct Second;
struct CrossA;
struct CrossB;

#[test]
fn singleton_and_transient_lifetimes_are_enforced() {
    let container = Container::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let factory_calls = Arc::clone(&calls);
    container
        .singleton::<Shared, _>(move |_| {
            factory_calls.fetch_add(1, Ordering::Relaxed);
            Ok(Arc::new(Shared(AtomicUsize::new(0))))
        })
        .unwrap();
    container
        .bind::<Transient, _>(|_| Ok(Arc::new(Transient(7))))
        .unwrap();
    let first = container.get::<Shared>().unwrap();
    let second = container.get::<Shared>().unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    first.0.store(11, Ordering::Relaxed);
    assert_eq!(second.0.load(Ordering::Relaxed), 11);
    let a = container.make::<Transient>().unwrap();
    let b = container.make::<Transient>().unwrap();
    assert!(!Arc::ptr_eq(&a, &b));
    assert_eq!(a.0, 7);
    assert!(container.make::<Shared>().is_err());
}

#[test]
fn detects_circular_dependencies_and_stops_resolution() {
    let container = Container::new();
    container
        .singleton::<First, _>(|c| {
            c.get::<Second>()?;
            Ok(Arc::new(First))
        })
        .unwrap();
    container
        .singleton::<Second, _>(|c| {
            c.get::<First>()?;
            Ok(Arc::new(Second))
        })
        .unwrap();
    assert!(matches!(
        container.get::<First>(),
        Err(ContainerError::Circular(_))
    ));
    container.shutdown();
    assert!(matches!(
        container.get::<First>(),
        Err(ContainerError::Stopped)
    ));
}

#[test]
fn concurrent_first_resolution_constructs_exactly_one_shared_singleton() {
    let container = Arc::new(Container::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let factory_calls = Arc::clone(&calls);
    container
        .singleton::<Shared, _>(move |_| {
            factory_calls.fetch_add(1, Ordering::SeqCst);
            thread::sleep(std::time::Duration::from_millis(20));
            Ok(Arc::new(Shared(AtomicUsize::new(5))))
        })
        .unwrap();
    let workers: Vec<_> = (0..32)
        .map(|_| {
            let container = Arc::clone(&container);
            thread::spawn(move || container.get::<Shared>().unwrap())
        })
        .collect();
    let instances: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        instances
            .iter()
            .all(|instance| Arc::ptr_eq(&instances[0], instance))
    );
}

#[test]
fn cross_thread_circular_dependencies_fail_instead_of_deadlocking() {
    let container = Arc::new(Container::new());
    let start_factories = Arc::new(std::sync::Barrier::new(2));
    let start_a = Arc::clone(&start_factories);
    let start_b = Arc::clone(&start_factories);
    let entered_a = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let entered_b = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let entered_a_once = Arc::clone(&entered_a);
    let entered_b_once = Arc::clone(&entered_b);
    container
        .singleton::<CrossA, _>(move |services| {
            if !entered_a_once.swap(true, Ordering::SeqCst) {
                start_a.wait();
            }
            services.get::<CrossB>()?;
            Ok(Arc::new(CrossA))
        })
        .unwrap();
    container
        .singleton::<CrossB, _>(move |services| {
            if !entered_b_once.swap(true, Ordering::SeqCst) {
                start_b.wait();
            }
            services.get::<CrossA>()?;
            Ok(Arc::new(CrossB))
        })
        .unwrap();

    let worker_a = {
        let container = Arc::clone(&container);
        thread::spawn(move || container.get::<CrossA>())
    };
    let worker_b = {
        let container = Arc::clone(&container);
        thread::spawn(move || container.get::<CrossB>())
    };
    let result_a = worker_a.join().unwrap();
    let result_b = worker_b.join().unwrap();
    assert!(
        matches!(result_a, Err(ContainerError::Circular(_)))
            || matches!(result_b, Err(ContainerError::Circular(_)))
    );
}
