//! Profiler panel with frame time, function time, and GPU time tracking.
//!
//! Implements Godot's Profiler panel which records per-frame timing data
//! for CPU functions, physics steps, and GPU operations. Supports:
//!
//! - **Frame recording**: capture frame timings over a window of frames.
//! - **Function profiling**: track named function/scope timings with call counts.
//! - **GPU timing**: separate GPU frame time tracking.
//! - **Statistics**: min, max, average, and percentile calculations.
//! - **Profiling sessions**: start/stop recording with bounded history.

use std::collections::HashMap;
use std::time::Duration;

// ---------------------------------------------------------------------------
// ProfilerEntry
// ---------------------------------------------------------------------------

/// A single profiling entry for a named scope (function, system, etc.).
#[derive(Debug, Clone)]
pub struct ProfilerEntry {
    /// Name of the profiled scope (e.g. `"_process"`, `"physics_step"`).
    pub name: String,
    /// Cumulative time spent in this scope during the frame.
    pub time: Duration,
    /// Number of times this scope was entered during the frame.
    pub call_count: u32,
}

impl ProfilerEntry {
    /// Creates a new profiler entry.
    pub fn new(name: impl Into<String>, time: Duration, call_count: u32) -> Self {
        Self {
            name: name.into(),
            time,
            call_count,
        }
    }

    /// Returns the time in milliseconds.
    pub fn time_ms(&self) -> f64 {
        self.time.as_secs_f64() * 1000.0
    }

    /// Returns the average time per call in milliseconds.
    pub fn avg_time_per_call_ms(&self) -> f64 {
        if self.call_count == 0 {
            0.0
        } else {
            self.time_ms() / self.call_count as f64
        }
    }
}

// ---------------------------------------------------------------------------
// FrameProfile
// ---------------------------------------------------------------------------

/// Timing data for a single frame.
#[derive(Debug, Clone)]
pub struct FrameProfile {
    /// Sequential frame number.
    pub frame_number: u64,
    /// Total CPU frame time.
    pub cpu_time: Duration,
    /// Total GPU frame time (if available).
    pub gpu_time: Duration,
    /// Physics step time.
    pub physics_time: Duration,
    /// Per-function/scope profiling entries.
    pub entries: Vec<ProfilerEntry>,
}

impl FrameProfile {
    /// Creates a new frame profile.
    pub fn new(frame_number: u64) -> Self {
        Self {
            frame_number,
            cpu_time: Duration::ZERO,
            gpu_time: Duration::ZERO,
            physics_time: Duration::ZERO,
            entries: Vec::new(),
        }
    }

    /// Returns total CPU time in milliseconds.
    pub fn cpu_time_ms(&self) -> f64 {
        self.cpu_time.as_secs_f64() * 1000.0
    }

    /// Returns total GPU time in milliseconds.
    pub fn gpu_time_ms(&self) -> f64 {
        self.gpu_time.as_secs_f64() * 1000.0
    }

    /// Returns physics step time in milliseconds.
    pub fn physics_time_ms(&self) -> f64 {
        self.physics_time.as_secs_f64() * 1000.0
    }

    /// Returns the effective FPS based on CPU frame time.
    pub fn fps(&self) -> f64 {
        let secs = self.cpu_time.as_secs_f64();
        if secs > 0.0 {
            1.0 / secs
        } else {
            0.0
        }
    }

    /// Adds a profiling entry for a named scope.
    pub fn add_entry(&mut self, name: impl Into<String>, time: Duration, call_count: u32) {
        self.entries
            .push(ProfilerEntry::new(name, time, call_count));
    }

    /// Returns an entry by name, if it exists.
    pub fn get_entry(&self, name: &str) -> Option<&ProfilerEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Returns entries sorted by time (descending — hottest first).
    pub fn sorted_by_time(&self) -> Vec<&ProfilerEntry> {
        let mut sorted: Vec<&ProfilerEntry> = self.entries.iter().collect();
        sorted.sort_by(|a, b| b.time.cmp(&a.time));
        sorted
    }
}

// ---------------------------------------------------------------------------
// ProfilerStats
// ---------------------------------------------------------------------------

/// Aggregate statistics over a range of frame profiles.
#[derive(Debug, Clone)]
pub struct ProfilerStats {
    /// Number of frames in the sample.
    pub frame_count: usize,
    /// Average CPU frame time in ms.
    pub avg_cpu_ms: f64,
    /// Minimum CPU frame time in ms.
    pub min_cpu_ms: f64,
    /// Maximum CPU frame time in ms.
    pub max_cpu_ms: f64,
    /// Average GPU frame time in ms.
    pub avg_gpu_ms: f64,
    /// Minimum GPU frame time in ms.
    pub min_gpu_ms: f64,
    /// Maximum GPU frame time in ms.
    pub max_gpu_ms: f64,
    /// Average FPS.
    pub avg_fps: f64,
    /// Minimum FPS (from slowest frame).
    pub min_fps: f64,
    /// Maximum FPS (from fastest frame).
    pub max_fps: f64,
    /// Average physics time in ms.
    pub avg_physics_ms: f64,
}

impl ProfilerStats {
    /// Computes statistics from a slice of frame profiles.
    pub fn from_frames(frames: &[FrameProfile]) -> Self {
        if frames.is_empty() {
            return Self {
                frame_count: 0,
                avg_cpu_ms: 0.0,
                min_cpu_ms: 0.0,
                max_cpu_ms: 0.0,
                avg_gpu_ms: 0.0,
                min_gpu_ms: 0.0,
                max_gpu_ms: 0.0,
                avg_fps: 0.0,
                min_fps: 0.0,
                max_fps: 0.0,
                avg_physics_ms: 0.0,
            };
        }

        let n = frames.len() as f64;
        let cpu_times: Vec<f64> = frames.iter().map(|f| f.cpu_time_ms()).collect();
        let gpu_times: Vec<f64> = frames.iter().map(|f| f.gpu_time_ms()).collect();
        let fps_values: Vec<f64> = frames.iter().map(|f| f.fps()).collect();
        let physics_times: Vec<f64> = frames.iter().map(|f| f.physics_time_ms()).collect();

        Self {
            frame_count: frames.len(),
            avg_cpu_ms: cpu_times.iter().sum::<f64>() / n,
            min_cpu_ms: cpu_times.iter().cloned().fold(f64::MAX, f64::min),
            max_cpu_ms: cpu_times.iter().cloned().fold(0.0, f64::max),
            avg_gpu_ms: gpu_times.iter().sum::<f64>() / n,
            min_gpu_ms: gpu_times.iter().cloned().fold(f64::MAX, f64::min),
            max_gpu_ms: gpu_times.iter().cloned().fold(0.0, f64::max),
            avg_fps: fps_values.iter().sum::<f64>() / n,
            min_fps: fps_values.iter().cloned().fold(f64::MAX, f64::min),
            max_fps: fps_values.iter().cloned().fold(0.0, f64::max),
            avg_physics_ms: physics_times.iter().sum::<f64>() / n,
        }
    }
}

// ---------------------------------------------------------------------------
// FunctionStats
// ---------------------------------------------------------------------------

/// Aggregate statistics for a single function across multiple frames.
#[derive(Debug, Clone)]
pub struct FunctionStats {
    /// Function/scope name.
    pub name: String,
    /// Total time across all frames.
    pub total_time: Duration,
    /// Total call count across all frames.
    pub total_calls: u32,
    /// Number of frames this function appeared in.
    pub frame_count: usize,
    /// Average time per frame in ms.
    pub avg_time_ms: f64,
    /// Maximum time in a single frame in ms.
    pub max_time_ms: f64,
}

// ---------------------------------------------------------------------------
// ProfilerPanel
// ---------------------------------------------------------------------------

/// The editor profiler panel — records and analyzes per-frame timing data.
///
/// Mirrors Godot's Profiler panel:
/// - Start/stop profiling sessions.
/// - Bounded frame history (oldest evicted).
/// - Per-function aggregate statistics.
/// - CPU, GPU, and physics time tracking.
#[derive(Debug)]
pub struct ProfilerPanel {
    /// Recorded frame profiles (bounded ring buffer).
    frames: Vec<FrameProfile>,
    /// Maximum number of frames to retain.
    max_frames: usize,
    /// Whether profiling is currently active.
    recording: bool,
    /// Next frame number to assign.
    next_frame: u64,
    /// Total frames recorded in this session (including evicted).
    total_recorded: u64,
}

impl Default for ProfilerPanel {
    fn default() -> Self {
        Self::new(3600) // 60 seconds at 60fps
    }
}

impl ProfilerPanel {
    /// Creates a new profiler panel with the given frame capacity.
    pub fn new(max_frames: usize) -> Self {
        Self {
            frames: Vec::with_capacity(max_frames.min(1024)),
            max_frames,
            recording: false,
            next_frame: 1,
            total_recorded: 0,
        }
    }

    /// Starts a profiling session. Clears previous data.
    pub fn start(&mut self) {
        self.frames.clear();
        self.next_frame = 1;
        self.total_recorded = 0;
        self.recording = true;
    }

    /// Stops the profiling session. Data is retained for inspection.
    pub fn stop(&mut self) {
        self.recording = false;
    }

    /// Returns whether profiling is currently active.
    pub fn is_recording(&self) -> bool {
        self.recording
    }

    /// Records a new frame profile. Only works when recording is active.
    ///
    /// Returns the frame number assigned, or `None` if not recording.
    pub fn record_frame(
        &mut self,
        cpu_time: Duration,
        gpu_time: Duration,
        physics_time: Duration,
        entries: Vec<ProfilerEntry>,
    ) -> Option<u64> {
        if !self.recording {
            return None;
        }

        let frame_num = self.next_frame;
        self.next_frame += 1;
        self.total_recorded += 1;

        let frame = FrameProfile {
            frame_number: frame_num,
            cpu_time,
            gpu_time,
            physics_time,
            entries,
        };

        if self.frames.len() >= self.max_frames && self.max_frames > 0 {
            self.frames.remove(0);
        }
        if self.max_frames > 0 {
            self.frames.push(frame);
        }

        Some(frame_num)
    }

    /// Returns the number of stored frames.
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Returns the total number of frames recorded (including evicted).
    pub fn total_recorded(&self) -> u64 {
        self.total_recorded
    }

    /// Returns a reference to all stored frames.
    pub fn frames(&self) -> &[FrameProfile] {
        &self.frames
    }

    /// Returns the most recent frame profile.
    pub fn latest_frame(&self) -> Option<&FrameProfile> {
        self.frames.last()
    }

    /// Returns a frame by its frame number.
    pub fn get_frame(&self, frame_number: u64) -> Option<&FrameProfile> {
        self.frames.iter().find(|f| f.frame_number == frame_number)
    }

    /// Computes aggregate statistics over all stored frames.
    pub fn stats(&self) -> ProfilerStats {
        ProfilerStats::from_frames(&self.frames)
    }

    /// Computes aggregate statistics over the last N frames.
    pub fn stats_last_n(&self, n: usize) -> ProfilerStats {
        let start = self.frames.len().saturating_sub(n);
        ProfilerStats::from_frames(&self.frames[start..])
    }

    /// Computes per-function aggregate statistics across all stored frames.
    pub fn function_stats(&self) -> Vec<FunctionStats> {
        let mut accum: HashMap<String, (Duration, u32, usize, f64)> = HashMap::new();

        for frame in &self.frames {
            for entry in &frame.entries {
                let e = accum.entry(entry.name.clone()).or_default();
                e.0 += entry.time;
                e.1 += entry.call_count;
                e.2 += 1;
                let ms = entry.time_ms();
                if ms > e.3 {
                    e.3 = ms;
                }
            }
        }

        let mut stats: Vec<FunctionStats> = accum
            .into_iter()
            .map(|(name, (total_time, total_calls, frame_count, max_ms))| {
                let avg_ms = if frame_count > 0 {
                    total_time.as_secs_f64() * 1000.0 / frame_count as f64
                } else {
                    0.0
                };
                FunctionStats {
                    name,
                    total_time,
                    total_calls,
                    frame_count,
                    avg_time_ms: avg_ms,
                    max_time_ms: max_ms,
                }
            })
            .collect();

        // Sort by total time descending (hottest functions first).
        stats.sort_by(|a, b| b.total_time.cmp(&a.total_time));
        stats
    }

    /// Returns the maximum frame capacity.
    pub fn max_frames(&self) -> usize {
        self.max_frames
    }

    /// Clears all recorded data without stopping recording.
    pub fn clear(&mut self) {
        self.frames.clear();
        self.total_recorded = 0;
        self.next_frame = 1;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn dur_ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    fn dur_us(us: u64) -> Duration {
        Duration::from_micros(us)
    }

    // ── ProfilerEntry ────────────────────────────────────────────────

    #[test]
    fn entry_time_ms() {
        let e = ProfilerEntry::new("test", dur_ms(16), 1);
        assert!((e.time_ms() - 16.0).abs() < 0.01);
    }

    #[test]
    fn entry_avg_time_per_call() {
        let e = ProfilerEntry::new("test", dur_ms(30), 3);
        assert!((e.avg_time_per_call_ms() - 10.0).abs() < 0.01);
    }

    #[test]
    fn entry_zero_calls() {
        let e = ProfilerEntry::new("test", dur_ms(10), 0);
        assert!((e.avg_time_per_call_ms()).abs() < 0.01);
    }

    // ── FrameProfile ─────────────────────────────────────────────────

    #[test]
    fn frame_profile_basic() {
        let mut f = FrameProfile::new(1);
        f.cpu_time = dur_ms(16);
        f.gpu_time = dur_ms(12);
        f.physics_time = dur_ms(4);

        assert!((f.cpu_time_ms() - 16.0).abs() < 0.01);
        assert!((f.gpu_time_ms() - 12.0).abs() < 0.01);
        assert!((f.physics_time_ms() - 4.0).abs() < 0.01);
    }

    #[test]
    fn frame_profile_fps() {
        let mut f = FrameProfile::new(1);
        f.cpu_time = dur_ms(16); // ~62.5 fps
        assert!((f.fps() - 62.5).abs() < 0.5);
    }

    #[test]
    fn frame_profile_fps_zero_time() {
        let f = FrameProfile::new(1);
        assert!((f.fps()).abs() < 0.01);
    }

    #[test]
    fn frame_profile_entries() {
        let mut f = FrameProfile::new(1);
        f.add_entry("_process", dur_ms(5), 1);
        f.add_entry("physics_step", dur_ms(3), 1);
        f.add_entry("render", dur_ms(8), 1);

        assert_eq!(f.entries.len(), 3);
        assert!(f.get_entry("_process").is_some());
        assert!(f.get_entry("nonexistent").is_none());
    }

    #[test]
    fn frame_profile_sorted_by_time() {
        let mut f = FrameProfile::new(1);
        f.add_entry("fast", dur_ms(1), 1);
        f.add_entry("slow", dur_ms(10), 1);
        f.add_entry("medium", dur_ms(5), 1);

        let sorted = f.sorted_by_time();
        assert_eq!(sorted[0].name, "slow");
        assert_eq!(sorted[1].name, "medium");
        assert_eq!(sorted[2].name, "fast");
    }

    // ── ProfilerStats ────────────────────────────────────────────────

    #[test]
    fn stats_empty() {
        let stats = ProfilerStats::from_frames(&[]);
        assert_eq!(stats.frame_count, 0);
        assert!((stats.avg_cpu_ms).abs() < 0.01);
    }

    #[test]
    fn stats_single_frame() {
        let mut f = FrameProfile::new(1);
        f.cpu_time = dur_ms(16);
        f.gpu_time = dur_ms(12);
        f.physics_time = dur_ms(4);

        let stats = ProfilerStats::from_frames(&[f]);
        assert_eq!(stats.frame_count, 1);
        assert!((stats.avg_cpu_ms - 16.0).abs() < 0.01);
        assert!((stats.avg_gpu_ms - 12.0).abs() < 0.01);
        assert!((stats.avg_physics_ms - 4.0).abs() < 0.01);
    }

    #[test]
    fn stats_multiple_frames() {
        let frames: Vec<FrameProfile> = (0..3)
            .map(|i| {
                let mut f = FrameProfile::new(i + 1);
                f.cpu_time = dur_ms(10 + i * 5); // 10, 15, 20
                f
            })
            .collect();

        let stats = ProfilerStats::from_frames(&frames);
        assert_eq!(stats.frame_count, 3);
        assert!((stats.avg_cpu_ms - 15.0).abs() < 0.01);
        assert!((stats.min_cpu_ms - 10.0).abs() < 0.01);
        assert!((stats.max_cpu_ms - 20.0).abs() < 0.01);
    }

    // ── ProfilerPanel ────────────────────────────────────────────────

    #[test]
    fn panel_default() {
        let panel = ProfilerPanel::default();
        assert!(!panel.is_recording());
        assert_eq!(panel.frame_count(), 0);
        assert_eq!(panel.max_frames(), 3600);
    }

    #[test]
    fn panel_start_stop() {
        let mut panel = ProfilerPanel::new(100);
        assert!(!panel.is_recording());

        panel.start();
        assert!(panel.is_recording());

        panel.stop();
        assert!(!panel.is_recording());
    }

    #[test]
    fn panel_record_frame() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();

        let num = panel.record_frame(dur_ms(16), dur_ms(12), dur_ms(4), vec![]);
        assert_eq!(num, Some(1));
        assert_eq!(panel.frame_count(), 1);
        assert_eq!(panel.total_recorded(), 1);
    }

    #[test]
    fn panel_record_not_recording() {
        let mut panel = ProfilerPanel::new(100);
        let num = panel.record_frame(dur_ms(16), dur_ms(12), dur_ms(4), vec![]);
        assert_eq!(num, None);
        assert_eq!(panel.frame_count(), 0);
    }

    #[test]
    fn panel_sequential_frame_numbers() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();

        let n1 = panel.record_frame(dur_ms(16), Duration::ZERO, Duration::ZERO, vec![]);
        let n2 = panel.record_frame(dur_ms(16), Duration::ZERO, Duration::ZERO, vec![]);
        let n3 = panel.record_frame(dur_ms(16), Duration::ZERO, Duration::ZERO, vec![]);

        assert_eq!(n1, Some(1));
        assert_eq!(n2, Some(2));
        assert_eq!(n3, Some(3));
    }

    #[test]
    fn panel_max_capacity_evicts_oldest() {
        let mut panel = ProfilerPanel::new(3);
        panel.start();

        for i in 0..5 {
            panel.record_frame(dur_ms(10 + i), Duration::ZERO, Duration::ZERO, vec![]);
        }

        assert_eq!(panel.frame_count(), 3);
        assert_eq!(panel.total_recorded(), 5);
        // Oldest two frames evicted.
        assert_eq!(panel.frames()[0].frame_number, 3);
        assert_eq!(panel.frames()[2].frame_number, 5);
    }

    #[test]
    fn panel_latest_frame() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();

        panel.record_frame(dur_ms(10), Duration::ZERO, Duration::ZERO, vec![]);
        panel.record_frame(dur_ms(20), Duration::ZERO, Duration::ZERO, vec![]);

        let latest = panel.latest_frame().unwrap();
        assert_eq!(latest.frame_number, 2);
        assert!((latest.cpu_time_ms() - 20.0).abs() < 0.01);
    }

    #[test]
    fn panel_get_frame() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();

        panel.record_frame(dur_ms(10), Duration::ZERO, Duration::ZERO, vec![]);
        panel.record_frame(dur_ms(20), Duration::ZERO, Duration::ZERO, vec![]);

        assert!(panel.get_frame(1).is_some());
        assert!(panel.get_frame(2).is_some());
        assert!(panel.get_frame(3).is_none());
    }

    #[test]
    fn panel_stats() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();

        for i in 0..10 {
            panel.record_frame(dur_ms(10 + i * 2), dur_ms(8 + i), dur_ms(3), vec![]);
        }

        let stats = panel.stats();
        assert_eq!(stats.frame_count, 10);
        assert!(stats.avg_cpu_ms > 10.0);
        assert!(stats.min_cpu_ms >= 10.0);
        assert!(stats.max_cpu_ms <= 28.0 + 0.01);
    }

    #[test]
    fn panel_stats_last_n() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();

        for i in 0..10 {
            panel.record_frame(dur_ms(10 + i * 10), Duration::ZERO, Duration::ZERO, vec![]);
        }

        let stats_3 = panel.stats_last_n(3);
        assert_eq!(stats_3.frame_count, 3);
        // Last 3 frames: 80ms, 90ms, 100ms
        assert!((stats_3.avg_cpu_ms - 90.0).abs() < 0.5);
    }

    #[test]
    fn panel_function_stats() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();

        // Frame 1: _process=5ms, render=10ms
        panel.record_frame(
            dur_ms(16),
            Duration::ZERO,
            Duration::ZERO,
            vec![
                ProfilerEntry::new("_process", dur_ms(5), 1),
                ProfilerEntry::new("render", dur_ms(10), 1),
            ],
        );

        // Frame 2: _process=3ms, render=12ms, physics=4ms
        panel.record_frame(
            dur_ms(20),
            Duration::ZERO,
            Duration::ZERO,
            vec![
                ProfilerEntry::new("_process", dur_ms(3), 1),
                ProfilerEntry::new("render", dur_ms(12), 1),
                ProfilerEntry::new("physics", dur_ms(4), 1),
            ],
        );

        let fstats = panel.function_stats();
        assert_eq!(fstats.len(), 3);

        // Sorted by total time descending: render (22ms) > _process (8ms) > physics (4ms).
        assert_eq!(fstats[0].name, "render");
        assert_eq!(fstats[0].total_calls, 2);
        assert_eq!(fstats[0].frame_count, 2);
        assert!((fstats[0].avg_time_ms - 11.0).abs() < 0.01);
        assert!((fstats[0].max_time_ms - 12.0).abs() < 0.01);

        assert_eq!(fstats[1].name, "_process");
        assert_eq!(fstats[2].name, "physics");
        assert_eq!(fstats[2].frame_count, 1);
    }

    #[test]
    fn panel_clear() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();
        panel.record_frame(dur_ms(16), Duration::ZERO, Duration::ZERO, vec![]);
        assert_eq!(panel.frame_count(), 1);

        panel.clear();
        assert_eq!(panel.frame_count(), 0);
        assert_eq!(panel.total_recorded(), 0);
        assert!(panel.is_recording()); // Still recording after clear.
    }

    #[test]
    fn panel_start_clears_data() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();
        panel.record_frame(dur_ms(16), Duration::ZERO, Duration::ZERO, vec![]);

        panel.start(); // Restart clears data.
        assert_eq!(panel.frame_count(), 0);
        assert_eq!(panel.total_recorded(), 0);
    }

    #[test]
    fn panel_record_with_entries() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();

        let entries = vec![
            ProfilerEntry::new("_process", dur_us(500), 1),
            ProfilerEntry::new("_physics_process", dur_us(250), 1),
        ];
        panel.record_frame(dur_ms(1), dur_us(800), dur_us(250), entries);

        let frame = panel.latest_frame().unwrap();
        assert_eq!(frame.entries.len(), 2);
        assert!(frame.get_entry("_process").is_some());
        assert!((frame.gpu_time_ms() - 0.8).abs() < 0.01);
    }

    #[test]
    fn panel_data_retained_after_stop() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();
        panel.record_frame(dur_ms(16), Duration::ZERO, Duration::ZERO, vec![]);
        panel.stop();

        assert_eq!(panel.frame_count(), 1);
        assert!(panel.latest_frame().is_some());
    }

    #[test]
    fn panel_function_stats_empty() {
        let panel = ProfilerPanel::new(100);
        let fstats = panel.function_stats();
        assert!(fstats.is_empty());
    }

    #[test]
    fn frame_profile_entries_microsecond_precision() {
        let mut f = FrameProfile::new(1);
        f.add_entry("micro_func", dur_us(500), 10);

        let entry = f.get_entry("micro_func").unwrap();
        assert!((entry.time_ms() - 0.5).abs() < 0.001);
        assert!((entry.avg_time_per_call_ms() - 0.05).abs() < 0.001);
    }
}
