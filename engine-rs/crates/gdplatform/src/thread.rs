//! User-space threading primitives mirroring Godot's Thread, Mutex, and
//! Semaphore classes.
//!
//! These wrap Rust's standard library threading to provide a Godot-compatible
//! API surface for GDScript interop.

use std::sync::{Arc, Condvar, Mutex as StdMutex};
use std::thread::{self, JoinHandle};

// ---------------------------------------------------------------------------
// GodotThread
// ---------------------------------------------------------------------------

/// A user-space thread mirroring Godot's `Thread` class.
///
/// Wraps [`std::thread::JoinHandle`] with Godot-compatible semantics:
/// start with a callable, check if alive, and wait for completion.
pub struct GodotThread {
    handle: Option<JoinHandle<()>>,
    is_started: bool,
}

impl GodotThread {
    /// Creates a new, unstarted thread.
    pub fn new() -> Self {
        Self {
            handle: None,
            is_started: false,
        }
    }

    /// Starts the thread with the given closure.
    ///
    /// Returns `true` if the thread was started successfully, `false` if it
    /// was already started.
    pub fn start<F>(&mut self, f: F) -> bool
    where
        F: FnOnce() + Send + 'static,
    {
        if self.is_started {
            return false;
        }
        self.handle = Some(thread::spawn(f));
        self.is_started = true;
        true
    }

    /// Returns `true` if the thread has been started and has not yet been
    /// joined.
    pub fn is_started(&self) -> bool {
        self.is_started
    }

    /// Returns `true` if the thread is still running.
    pub fn is_alive(&self) -> bool {
        match &self.handle {
            Some(h) => !h.is_finished(),
            None => false,
        }
    }

    /// Waits for the thread to finish (blocks the caller).
    ///
    /// Mirrors Godot's `Thread.wait_to_finish()`. After this call,
    /// `is_started()` returns `false`.
    pub fn wait_to_finish(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.is_started = false;
    }

    /// Returns a unique identifier for this thread, if started.
    pub fn get_id(&self) -> Option<u64> {
        self.handle.as_ref().map(|h| {
            let id = h.thread().id();
            // Convert ThreadId to u64 via Debug format (stable approach)
            let s = format!("{:?}", id);
            s.chars()
                .filter(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u64>()
                .unwrap_or(0)
        })
    }
}

impl Default for GodotThread {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// GodotMutex
// ---------------------------------------------------------------------------

/// A user-space mutex mirroring Godot's `Mutex` class.
///
/// Wraps [`std::sync::Mutex`] with Godot-compatible lock/unlock semantics.
/// The mutex is non-recursive (locking twice from the same thread will
/// deadlock, matching Godot 4.x behavior).
#[derive(Debug)]
pub struct GodotMutex {
    inner: Arc<StdMutex<()>>,
}

impl GodotMutex {
    /// Creates a new, unlocked mutex.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(StdMutex::new(())),
        }
    }

    /// Locks the mutex, blocking until it becomes available.
    ///
    /// Mirrors Godot's `Mutex.lock()`.
    pub fn lock(&self) -> GodotMutexGuard {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        GodotMutexGuard {
            _guard: Some(guard),
        }
    }

    /// Attempts to lock the mutex without blocking.
    ///
    /// Returns `true` if the lock was acquired, `false` if it was already
    /// held. Mirrors Godot's `Mutex.try_lock()`.
    pub fn try_lock(&self) -> Option<GodotMutexGuard> {
        match self.inner.try_lock() {
            Ok(guard) => Some(GodotMutexGuard {
                _guard: Some(guard),
            }),
            Err(std::sync::TryLockError::WouldBlock) => None,
            Err(std::sync::TryLockError::Poisoned(e)) => Some(GodotMutexGuard {
                _guard: Some(e.into_inner()),
            }),
        }
    }

    /// Returns a cloneable handle to the inner Arc for sharing across threads.
    pub fn share(&self) -> Arc<StdMutex<()>> {
        Arc::clone(&self.inner)
    }
}

impl Default for GodotMutex {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for GodotMutex {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// RAII guard returned by [`GodotMutex::lock`].
pub struct GodotMutexGuard<'a> {
    _guard: Option<std::sync::MutexGuard<'a, ()>>,
}

impl<'a> GodotMutexGuard<'a> {
    /// Explicitly unlocks the mutex (drops the guard).
    pub fn unlock(mut self) {
        self._guard.take();
    }
}

// ---------------------------------------------------------------------------
// GodotSemaphore
// ---------------------------------------------------------------------------

/// A counting semaphore mirroring Godot's `Semaphore` class.
///
/// Threads can `wait()` to decrement the count (blocking if zero) and
/// `post()` to increment it (waking one waiting thread).
#[derive(Debug, Clone)]
pub struct GodotSemaphore {
    inner: Arc<SemaphoreInner>,
}

#[derive(Debug)]
struct SemaphoreInner {
    mutex: StdMutex<i32>,
    condvar: Condvar,
}

impl GodotSemaphore {
    /// Creates a new semaphore with the given initial count.
    pub fn new(initial_count: i32) -> Self {
        Self {
            inner: Arc::new(SemaphoreInner {
                mutex: StdMutex::new(initial_count),
                condvar: Condvar::new(),
            }),
        }
    }

    /// Decrements the semaphore count. Blocks if the count is zero until
    /// another thread calls [`post`](Self::post).
    ///
    /// Mirrors Godot's `Semaphore.wait()`.
    pub fn wait(&self) {
        let mut count = self.inner.mutex.lock().unwrap();
        while *count <= 0 {
            count = self.inner.condvar.wait(count).unwrap();
        }
        *count -= 1;
    }

    /// Attempts to decrement without blocking. Returns `true` if successful.
    pub fn try_wait(&self) -> bool {
        let mut count = self.inner.mutex.lock().unwrap();
        if *count > 0 {
            *count -= 1;
            true
        } else {
            false
        }
    }

    /// Increments the semaphore count, waking one waiting thread if any.
    ///
    /// Mirrors Godot's `Semaphore.post()`.
    pub fn post(&self) {
        let mut count = self.inner.mutex.lock().unwrap();
        *count += 1;
        self.inner.condvar.notify_one();
    }

    /// Returns the current count (for testing/debugging).
    pub fn count(&self) -> i32 {
        *self.inner.mutex.lock().unwrap()
    }
}

impl Default for GodotSemaphore {
    fn default() -> Self {
        Self::new(0)
    }
}

// ---------------------------------------------------------------------------
// WorkerThreadPool
// ---------------------------------------------------------------------------

/// A task ID returned by [`WorkerThreadPool::add_task`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(u64);

impl TaskId {
    /// Returns the raw numeric ID.
    pub fn raw(&self) -> u64 {
        self.0
    }
}

/// Task completion status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task is waiting in the queue.
    Pending,
    /// Task is currently running on a worker thread.
    Running,
    /// Task has completed successfully.
    Completed,
}

struct TaskEntry {
    status: Arc<StdMutex<TaskStatus>>,
    handle: Option<JoinHandle<()>>,
}

/// A thread pool for submitting and awaiting background tasks.
///
/// Mirrors Godot's `WorkerThreadPool` API: submit tasks, check their
/// status, and wait for completion. For v1, each task spawns a dedicated
/// OS thread (no fixed-size pool). Future versions may use a bounded
/// pool with work-stealing.
pub struct WorkerThreadPool {
    tasks: StdMutex<std::collections::HashMap<u64, TaskEntry>>,
    next_id: StdMutex<u64>,
}

impl WorkerThreadPool {
    /// Creates a new, empty thread pool.
    pub fn new() -> Self {
        Self {
            tasks: StdMutex::new(std::collections::HashMap::new()),
            next_id: StdMutex::new(1),
        }
    }

    /// Submits a task for background execution.
    ///
    /// Returns a [`TaskId`] that can be used to check status or wait for
    /// completion.
    pub fn add_task<F>(&self, f: F) -> TaskId
    where
        F: FnOnce() + Send + 'static,
    {
        let id = {
            let mut next = self.next_id.lock().unwrap();
            let id = *next;
            *next += 1;
            id
        };

        let status = Arc::new(StdMutex::new(TaskStatus::Running));
        let status_clone = Arc::clone(&status);

        let handle = thread::spawn(move || {
            f();
            *status_clone.lock().unwrap() = TaskStatus::Completed;
        });

        let entry = TaskEntry {
            status,
            handle: Some(handle),
        };

        self.tasks.lock().unwrap().insert(id, entry);
        TaskId(id)
    }

    /// Returns the status of a task.
    pub fn get_task_status(&self, task_id: TaskId) -> TaskStatus {
        let tasks = self.tasks.lock().unwrap();
        match tasks.get(&task_id.0) {
            Some(entry) => *entry.status.lock().unwrap(),
            None => TaskStatus::Completed, // Unknown tasks are treated as done
        }
    }

    /// Returns `true` if the task has completed.
    pub fn is_task_completed(&self, task_id: TaskId) -> bool {
        self.get_task_status(task_id) == TaskStatus::Completed
    }

    /// Blocks until the given task completes.
    pub fn wait_for_task_completion(&self, task_id: TaskId) {
        let handle = {
            let mut tasks = self.tasks.lock().unwrap();
            tasks.get_mut(&task_id.0).and_then(|e| e.handle.take())
        };
        if let Some(h) = handle {
            let _ = h.join();
        }
    }

    /// Returns the number of tasks currently tracked (pending + running + completed).
    pub fn task_count(&self) -> usize {
        self.tasks.lock().unwrap().len()
    }
}

impl Default for WorkerThreadPool {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::time::Duration;

    #[test]
    fn thread_start_and_join() {
        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = flag.clone();

        let mut t = GodotThread::new();
        assert!(!t.is_started());
        assert!(!t.is_alive());

        t.start(move || {
            flag_clone.store(true, Ordering::SeqCst);
        });
        assert!(t.is_started());

        t.wait_to_finish();
        assert!(!t.is_started());
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn thread_double_start_fails() {
        let mut t = GodotThread::new();
        assert!(t.start(|| {}));
        assert!(!t.start(|| {})); // second start fails
        t.wait_to_finish();
    }

    #[test]
    fn thread_get_id() {
        let mut t = GodotThread::new();
        assert!(t.get_id().is_none());
        t.start(|| {
            std::thread::sleep(Duration::from_millis(10));
        });
        assert!(t.get_id().is_some());
        t.wait_to_finish();
    }

    #[test]
    fn mutex_lock_unlock() {
        let m = GodotMutex::new();
        let guard = m.lock();
        guard.unlock();
    }

    #[test]
    fn mutex_try_lock_succeeds_when_free() {
        let m = GodotMutex::new();
        let guard = m.try_lock();
        assert!(guard.is_some());
    }

    #[test]
    fn mutex_protects_shared_state() {
        let counter = Arc::new(StdMutex::new(0u32));
        let m = GodotMutex::new();
        let mut threads = Vec::new();

        for _ in 0..4 {
            let c = counter.clone();
            let m2 = m.clone();
            threads.push(thread::spawn(move || {
                for _ in 0..100 {
                    let _guard = m2.lock();
                    let mut val = c.lock().unwrap();
                    *val += 1;
                }
            }));
        }

        for t in threads {
            t.join().unwrap();
        }

        assert_eq!(*counter.lock().unwrap(), 400);
    }

    #[test]
    fn semaphore_post_and_wait() {
        let sem = GodotSemaphore::new(0);
        assert_eq!(sem.count(), 0);

        sem.post();
        assert_eq!(sem.count(), 1);

        sem.wait();
        assert_eq!(sem.count(), 0);
    }

    #[test]
    fn semaphore_try_wait() {
        let sem = GodotSemaphore::new(1);
        assert!(sem.try_wait());
        assert!(!sem.try_wait()); // count is now 0
    }

    #[test]
    fn semaphore_producer_consumer() {
        let sem = GodotSemaphore::new(0);
        let sem2 = sem.clone();
        let result = Arc::new(AtomicU32::new(0));
        let result2 = result.clone();

        let consumer = thread::spawn(move || {
            sem2.wait(); // blocks until producer posts
            result2.store(42, Ordering::SeqCst);
        });

        thread::sleep(Duration::from_millis(10));
        assert_eq!(result.load(Ordering::SeqCst), 0); // consumer still waiting
        sem.post(); // unblock consumer

        consumer.join().unwrap();
        assert_eq!(result.load(Ordering::SeqCst), 42);
    }

    #[test]
    fn semaphore_initial_count() {
        let sem = GodotSemaphore::new(3);
        assert_eq!(sem.count(), 3);
        assert!(sem.try_wait());
        assert!(sem.try_wait());
        assert!(sem.try_wait());
        assert!(!sem.try_wait());
    }

    // -- pat-ao5fs: WorkerThreadPool tests -----------------------------------

    #[test]
    fn pool_add_task_returns_id() {
        let pool = WorkerThreadPool::new();
        let id = pool.add_task(|| {});
        assert_eq!(id.raw(), 1);
        pool.wait_for_task_completion(id);
    }

    #[test]
    fn pool_task_ids_increment() {
        let pool = WorkerThreadPool::new();
        let id1 = pool.add_task(|| {});
        let id2 = pool.add_task(|| {});
        assert_ne!(id1, id2);
        assert_eq!(id2.raw(), id1.raw() + 1);
        pool.wait_for_task_completion(id1);
        pool.wait_for_task_completion(id2);
    }

    #[test]
    fn pool_wait_for_completion() {
        let result = Arc::new(AtomicU32::new(0));
        let r2 = result.clone();

        let pool = WorkerThreadPool::new();
        let id = pool.add_task(move || {
            thread::sleep(Duration::from_millis(20));
            r2.store(99, Ordering::SeqCst);
        });

        pool.wait_for_task_completion(id);
        assert_eq!(result.load(Ordering::SeqCst), 99);
    }

    #[test]
    fn pool_is_task_completed() {
        let pool = WorkerThreadPool::new();
        let id = pool.add_task(|| {});
        pool.wait_for_task_completion(id);
        assert!(pool.is_task_completed(id));
    }

    #[test]
    fn pool_task_status_transitions() {
        let barrier = Arc::new((StdMutex::new(false), Condvar::new()));
        let b2 = barrier.clone();

        let pool = WorkerThreadPool::new();
        let id = pool.add_task(move || {
            let (lock, cvar) = &*b2;
            let mut started = lock.lock().unwrap();
            while !*started {
                started = cvar.wait(started).unwrap();
            }
        });

        // Task is running (blocked on barrier).
        assert_eq!(pool.get_task_status(id), TaskStatus::Running);

        // Release the barrier.
        {
            let (lock, cvar) = &*barrier;
            *lock.lock().unwrap() = true;
            cvar.notify_one();
        }

        pool.wait_for_task_completion(id);
        assert_eq!(pool.get_task_status(id), TaskStatus::Completed);
    }

    #[test]
    fn pool_multiple_tasks_concurrent() {
        let counter = Arc::new(AtomicU32::new(0));
        let pool = WorkerThreadPool::new();
        let mut ids = Vec::new();

        for _ in 0..10 {
            let c = counter.clone();
            ids.push(pool.add_task(move || {
                c.fetch_add(1, Ordering::SeqCst);
            }));
        }

        for id in &ids {
            pool.wait_for_task_completion(*id);
        }

        assert_eq!(counter.load(Ordering::SeqCst), 10);
        assert_eq!(pool.task_count(), 10);
    }

    #[test]
    fn pool_task_count() {
        let pool = WorkerThreadPool::new();
        assert_eq!(pool.task_count(), 0);

        let id1 = pool.add_task(|| {});
        let id2 = pool.add_task(|| {});
        assert_eq!(pool.task_count(), 2);

        pool.wait_for_task_completion(id1);
        pool.wait_for_task_completion(id2);
    }

    #[test]
    fn pool_unknown_task_is_completed() {
        let pool = WorkerThreadPool::new();
        assert!(pool.is_task_completed(TaskId(999)));
    }
}
