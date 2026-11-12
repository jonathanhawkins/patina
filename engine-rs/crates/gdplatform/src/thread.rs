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
    pub fn lock(&self) -> GodotMutexGuard<'_> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        GodotMutexGuard {
            _guard: Some(guard),
        }
    }

    /// Attempts to lock the mutex without blocking.
    ///
    /// Returns `true` if the lock was acquired, `false` if it was already
    /// held. Mirrors Godot's `Mutex.try_lock()`.
    pub fn try_lock(&self) -> Option<GodotMutexGuard<'_>> {
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
    /// Group tasks and their queued elements (pat-s3rrv.217).
    group_tasks: StdMutex<std::collections::HashMap<u64, GroupEntry>>,
    /// Whether work runs on OS threads or is drained cooperatively.
    pool_mode: PoolMode,
}

impl WorkerThreadPool {
    /// Creates a new, empty thread pool.
    pub fn new() -> Self {
        Self {
            tasks: StdMutex::new(std::collections::HashMap::new()),
            next_id: StdMutex::new(1),
            group_tasks: StdMutex::new(std::collections::HashMap::new()),
            pool_mode: PoolMode::Threaded,
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

// ===========================================================================
// Group tasks and the cooperative fallback (pat-s3rrv.217)
// ===========================================================================

/// Identifies a group task submitted to [`WorkerThreadPool`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupTaskId(pub u64);

/// A group task's queued work and progress.
struct GroupEntry {
    /// Elements not yet run. Empty once the group has completed.
    pending: Vec<Box<dyn FnOnce() + Send + 'static>>,
    /// How many elements have run, which is Godot's
    /// `get_group_processed_element_count`.
    processed: usize,
    /// Total elements the group was created with.
    total: usize,
}

/// How a [`WorkerThreadPool`] runs its work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolMode {
    /// Each task gets an OS thread. The native default.
    Threaded,
    /// Work is queued and drained on the caller's thread — the only option on a
    /// web build without cross-origin isolation, where the browser gives the
    /// engine no worker it can share memory with.
    Cooperative,
}

/// A group of `elements` work items, submitted together.
///
/// Mirrors Godot's `WorkerThreadPool.add_group_task(callable, elements, ...)`.
impl WorkerThreadPool {
    /// Creates a pool that runs work cooperatively rather than on OS threads.
    ///
    /// This is the web-without-isolation path: the browser gives the engine no
    /// worker it can share memory with, so the work is queued and drained on
    /// the frame tick instead.
    pub fn new_cooperative() -> Self {
        Self {
            pool_mode: PoolMode::Cooperative,
            ..Self::new()
        }
    }

    /// Creates the pool this platform can actually back.
    ///
    /// 🔑 Keyed on `has_feature("threads")` rather than on the target, because
    /// the same wasm bundle is threaded or not depending on the headers the
    /// server sent. On native this is always the threaded pool.
    pub fn for_current_platform() -> Self {
        if crate::os::has_feature("threads") {
            Self::new()
        } else {
            Self::new_cooperative()
        }
    }

    /// How this pool runs its work.
    pub fn mode(&self) -> PoolMode {
        self.pool_mode
    }

    fn groups(&self) -> &StdMutex<std::collections::HashMap<u64, GroupEntry>> {
        &self.group_tasks
    }

    /// Submits `elements` copies of `f`, each receiving its element index.
    ///
    /// In [`PoolMode::Threaded`] the elements run immediately on OS threads. In
    /// [`PoolMode::Cooperative`] they are QUEUED and run when
    /// [`drain_pending`](Self::drain_pending) is called on the frame tick, or
    /// when [`wait_for_group_task_completion`](Self::wait_for_group_task_completion)
    /// is called — see that method for why the second is not optional.
    pub fn add_group_task<F>(&self, elements: usize, f: F) -> GroupTaskId
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        let id = {
            let mut next = self.next_id.lock().unwrap();
            let id = *next;
            *next += 1;
            id
        };

        let shared = Arc::new(f);
        let pending: Vec<Box<dyn FnOnce() + Send + 'static>> = (0..elements)
            .map(|index| {
                let f = Arc::clone(&shared);
                Box::new(move || f(index)) as Box<dyn FnOnce() + Send + 'static>
            })
            .collect();

        self.groups().lock().unwrap().insert(
            id,
            GroupEntry {
                pending,
                processed: 0,
                total: elements,
            },
        );

        if self.mode() == PoolMode::Threaded {
            self.run_group_now(id);
        }
        GroupTaskId(id)
    }

    /// Runs every queued element of `id` on the calling thread.
    ///
    /// Elements are taken out of the map before running so the pool's lock is
    /// not held across user code — a task that touches the pool would otherwise
    /// deadlock against itself.
    fn run_group_now(&self, id: u64) {
        loop {
            let next = {
                let mut groups = self.groups().lock().unwrap();
                match groups.get_mut(&id) {
                    Some(entry) if !entry.pending.is_empty() => Some(entry.pending.remove(0)),
                    _ => None,
                }
            };
            match next {
                Some(work) => {
                    work();
                    if let Some(entry) = self.groups().lock().unwrap().get_mut(&id) {
                        entry.processed += 1;
                    }
                }
                None => break,
            }
        }
    }

    /// Runs everything queued by every group — the frame-tick drain.
    ///
    /// A no-op in [`PoolMode::Threaded`], where nothing is ever left queued.
    pub fn drain_pending(&self) {
        let ids: Vec<u64> = self.groups().lock().unwrap().keys().copied().collect();
        for id in ids {
            self.run_group_now(id);
        }
    }

    /// Blocks until every element of the group has run.
    ///
    /// 🔑 In [`PoolMode::Cooperative`] this DRAINS rather than waits. There is
    /// no other thread that could make progress, so a wait loop would spin
    /// forever on the very build that needs this path — a hang rather than a
    /// slow frame. Draining inline is what makes the same script work on both
    /// the isolated and non-isolated web exports.
    pub fn wait_for_group_task_completion(&self, group: GroupTaskId) {
        self.run_group_now(group.0);
    }

    /// Whether every element of the group has run.
    pub fn is_group_task_completed(&self, group: GroupTaskId) -> bool {
        match self.groups().lock().unwrap().get(&group.0) {
            Some(entry) => entry.pending.is_empty(),
            // An unknown group is treated as done, matching `get_task_status`.
            None => true,
        }
    }

    /// How many of the group's elements have run
    /// (`WorkerThreadPool.get_group_processed_element_count`).
    pub fn get_group_processed_element_count(&self, group: GroupTaskId) -> usize {
        match self.groups().lock().unwrap().get(&group.0) {
            Some(entry) => entry.processed,
            None => 0,
        }
    }

    /// How many elements the group was submitted with.
    pub fn group_element_count(&self, group: GroupTaskId) -> usize {
        match self.groups().lock().unwrap().get(&group.0) {
            Some(entry) => entry.total,
            None => 0,
        }
    }
}

// ===========================================================================
// Thread-safety checks (pat-s3rrv.217)
// ===========================================================================

std::thread_local! {
    /// Whether the engine's conservative thread-safety checks run on THIS
    /// thread (`Thread.set_thread_safety_checks_enabled`).
    ///
    /// ⚠️ **Thread-local, not global, and that is the whole semantic.** Godot's
    /// documentation says the setting applies "on the current thread" and that
    /// "the default, for every thread, is that they are enabled". A process-wide
    /// flag would let one worker disabling its own checks silently disable them
    /// for the main thread too — turning a deliberate, locally-scoped escape
    /// hatch into an engine-wide one, which is the opposite of what a caller
    /// reaching for this is asking for.
    static THREAD_SAFETY_CHECKS: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Sets whether the engine's thread-safety checks run on the calling thread.
///
/// Godot's checks are conservative: they only permit a call when safety can be
/// guaranteed, so a caller who knows better — and who is taking on the
/// synchronisation themselves, typically with a [`GodotMutex`] — can switch
/// them off for their own thread.
pub fn set_thread_safety_checks_enabled(enabled: bool) {
    THREAD_SAFETY_CHECKS.with(|c| c.set(enabled));
}

/// Whether the engine's thread-safety checks run on the calling thread.
///
/// Defaults to `true` on every thread, including threads spawned after another
/// thread disabled its own.
pub fn is_thread_safety_checks_enabled() -> bool {
    THREAD_SAFETY_CHECKS.with(|c| c.get())
}
