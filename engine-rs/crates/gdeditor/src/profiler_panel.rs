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
// ProfilerMarker
// ---------------------------------------------------------------------------

/// A user-placed marker/bookmark on a specific frame.
#[derive(Debug, Clone)]
pub struct ProfilerMarker {
    /// The frame number this marker is attached to.
    pub frame_number: u64,
    /// User-provided label for this marker.
    pub label: String,
}

// ---------------------------------------------------------------------------
// Helper: percentile calculation
// ---------------------------------------------------------------------------

fn percentile_from_durations(durations: &[Duration], pct: f64) -> f64 {
    if durations.is_empty() {
        return 0.0;
    }
    let mut ms: Vec<f64> = durations.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
    ms.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((pct / 100.0) * (ms.len() - 1) as f64).round() as usize;
    ms[idx.min(ms.len() - 1)]
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
    /// User-placed markers/bookmarks on specific frames.
    markers: Vec<ProfilerMarker>,
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
            markers: Vec::new(),
        }
    }

    /// Starts a profiling session. Clears previous data.
    pub fn start(&mut self) {
        self.frames.clear();
        self.markers.clear();
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

    /// Returns frames where CPU time exceeds the given threshold.
    pub fn spike_frames(&self, threshold: Duration) -> Vec<&FrameProfile> {
        self.frames
            .iter()
            .filter(|f| f.cpu_time > threshold)
            .collect()
    }

    /// Returns the percentile CPU frame time in ms (0.0–100.0).
    ///
    /// E.g., `percentile_cpu_ms(95.0)` returns the 95th percentile.
    pub fn percentile_cpu_ms(&self, pct: f64) -> f64 {
        percentile_from_durations(
            &self.frames.iter().map(|f| f.cpu_time).collect::<Vec<_>>(),
            pct,
        )
    }

    /// Returns the percentile GPU frame time in ms.
    pub fn percentile_gpu_ms(&self, pct: f64) -> f64 {
        percentile_from_durations(
            &self.frames.iter().map(|f| f.gpu_time).collect::<Vec<_>>(),
            pct,
        )
    }

    /// Returns a histogram of CPU frame times bucketed into `bucket_count` bins.
    ///
    /// Each entry is `(bucket_start_ms, bucket_end_ms, count)`.
    pub fn cpu_time_histogram(&self, bucket_count: usize) -> Vec<(f64, f64, usize)> {
        if self.frames.is_empty() || bucket_count == 0 {
            return Vec::new();
        }
        let times: Vec<f64> = self.frames.iter().map(|f| f.cpu_time_ms()).collect();
        let min = times.iter().cloned().fold(f64::MAX, f64::min);
        let max = times.iter().cloned().fold(0.0_f64, f64::max);
        if (max - min).abs() < f64::EPSILON {
            return vec![(min, max, times.len())];
        }
        let width = (max - min) / bucket_count as f64;
        let mut buckets = Vec::with_capacity(bucket_count);
        for i in 0..bucket_count {
            let lo = min + i as f64 * width;
            let is_last = i == bucket_count - 1;
            let hi = lo + width;
            let count = times
                .iter()
                .filter(|&&t| {
                    if is_last {
                        t >= lo && t <= hi + f64::EPSILON
                    } else {
                        t >= lo && t < hi
                    }
                })
                .count();
            buckets.push((lo, hi, count));
        }
        buckets
    }

    /// Returns per-function hotspot analysis: each function's percentage of total CPU time.
    ///
    /// Returns `Vec<(name, total_ms, percentage)>` sorted by percentage descending.
    pub fn function_hotspots(&self) -> Vec<(String, f64, f64)> {
        let total_cpu_ms: f64 = self.frames.iter().map(|f| f.cpu_time_ms()).sum();
        if total_cpu_ms <= 0.0 {
            return Vec::new();
        }
        let mut accum: HashMap<String, f64> = HashMap::new();
        for frame in &self.frames {
            for entry in &frame.entries {
                *accum.entry(entry.name.clone()).or_default() += entry.time_ms();
            }
        }
        let mut hotspots: Vec<(String, f64, f64)> = accum
            .into_iter()
            .map(|(name, ms)| {
                let pct = (ms / total_cpu_ms) * 100.0;
                (name, ms, pct)
            })
            .collect();
        hotspots.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        hotspots
    }

    /// Adds a bookmark/marker at a specific frame number.
    pub fn add_marker(&mut self, frame_number: u64, label: String) {
        self.markers.push(ProfilerMarker {
            frame_number,
            label,
        });
    }

    /// Returns all markers.
    pub fn markers(&self) -> &[ProfilerMarker] {
        &self.markers
    }

    /// Returns markers within a frame range (inclusive).
    pub fn markers_in_range(&self, from: u64, to: u64) -> Vec<&ProfilerMarker> {
        self.markers
            .iter()
            .filter(|m| m.frame_number >= from && m.frame_number <= to)
            .collect()
    }

    /// Compares stats between two frame ranges.
    ///
    /// Returns `(stats_a, stats_b)` for the given frame number ranges.
    pub fn compare_ranges(
        &self,
        range_a: (u64, u64),
        range_b: (u64, u64),
    ) -> (ProfilerStats, ProfilerStats) {
        let frames_a: Vec<FrameProfile> = self
            .frames
            .iter()
            .filter(|f| f.frame_number >= range_a.0 && f.frame_number <= range_a.1)
            .cloned()
            .collect();
        let frames_b: Vec<FrameProfile> = self
            .frames
            .iter()
            .filter(|f| f.frame_number >= range_b.0 && f.frame_number <= range_b.1)
            .cloned()
            .collect();
        (
            ProfilerStats::from_frames(&frames_a),
            ProfilerStats::from_frames(&frames_b),
        )
    }

    /// Returns a summary line suitable for display (e.g., status bar).
    pub fn summary_line(&self) -> String {
        if self.frames.is_empty() {
            return "No profiling data".to_string();
        }
        let stats = self.stats();
        format!(
            "Frames: {} | CPU: {:.1}ms avg ({:.1} FPS) | GPU: {:.1}ms avg | Spikes(>33ms): {}",
            stats.frame_count,
            stats.avg_cpu_ms,
            stats.avg_fps,
            stats.avg_gpu_ms,
            self.spike_frames(Duration::from_millis(33)).len(),
        )
    }
}

// ---------------------------------------------------------------------------
// FrameGraphBar
// ---------------------------------------------------------------------------

/// A single bar in the frame graph visualization.
#[derive(Debug, Clone)]
pub struct FrameGraphBar {
    /// Frame number this bar represents.
    pub frame_number: u64,
    /// CPU time in ms (bar height source).
    pub cpu_ms: f64,
    /// GPU time in ms.
    pub gpu_ms: f64,
    /// Physics time in ms.
    pub physics_ms: f64,
    /// Normalized height (0.0–1.0) relative to the graph's y-axis max.
    pub normalized_height: f64,
    /// Whether this frame is a spike (exceeds target frame time).
    pub is_spike: bool,
}

// ---------------------------------------------------------------------------
// FrameGraphTooltip
// ---------------------------------------------------------------------------

/// Tooltip data shown when hovering over a frame bar.
#[derive(Debug, Clone)]
pub struct FrameGraphTooltip {
    pub frame_number: u64,
    pub cpu_ms: f64,
    pub gpu_ms: f64,
    pub physics_ms: f64,
    pub fps: f64,
    /// Top N hottest functions in this frame.
    pub top_functions: Vec<(String, f64)>,
}

// ---------------------------------------------------------------------------
// FrameGraph
// ---------------------------------------------------------------------------

/// Visual frame graph component for the profiler panel.
///
/// Produces data for rendering a bar chart timeline of frame times,
/// matching Godot's profiler frame graph:
/// - Scrollable/pannable viewport over recorded frames
/// - Zoom control for time resolution
/// - Frame selection with tooltip details
/// - Target FPS line overlay
/// - Color coding for spikes
#[derive(Debug)]
pub struct FrameGraph {
    /// Index into the profiler's frame buffer for viewport start.
    viewport_start: usize,
    /// Number of frames visible in the viewport.
    viewport_size: usize,
    /// Currently selected frame index (within viewport), if any.
    selected_index: Option<usize>,
    /// Target frame time in ms (e.g., 16.67 for 60fps).
    target_frame_ms: f64,
    /// Y-axis maximum in ms (auto or manual).
    y_max_ms: Option<f64>,
    /// Whether to show GPU time bars.
    show_gpu: bool,
    /// Whether to show physics time bars.
    show_physics: bool,
    /// Number of top functions to include in tooltips.
    tooltip_top_n: usize,
}

impl Default for FrameGraph {
    fn default() -> Self {
        Self {
            viewport_start: 0,
            viewport_size: 200,
            selected_index: None,
            target_frame_ms: 16.667, // 60 FPS
            y_max_ms: None,
            show_gpu: true,
            show_physics: true,
            tooltip_top_n: 5,
        }
    }
}

impl FrameGraph {
    /// Creates a frame graph with a given viewport size.
    pub fn new(viewport_size: usize) -> Self {
        Self {
            viewport_size,
            ..Default::default()
        }
    }

    /// Sets the target frame time in ms.
    pub fn set_target_fps(&mut self, fps: f64) {
        if fps > 0.0 {
            self.target_frame_ms = 1000.0 / fps;
        }
    }

    /// Returns the current target frame time in ms.
    pub fn target_frame_ms(&self) -> f64 {
        self.target_frame_ms
    }

    /// Sets a fixed y-axis maximum. Pass `None` for auto-scaling.
    pub fn set_y_max(&mut self, ms: Option<f64>) {
        self.y_max_ms = ms;
    }

    /// Toggles GPU bar visibility.
    pub fn toggle_gpu(&mut self) {
        self.show_gpu = !self.show_gpu;
    }

    /// Toggles physics bar visibility.
    pub fn toggle_physics(&mut self) {
        self.show_physics = !self.show_physics;
    }

    /// Returns whether GPU bars are shown.
    pub fn show_gpu(&self) -> bool {
        self.show_gpu
    }

    /// Returns whether physics bars are shown.
    pub fn show_physics(&self) -> bool {
        self.show_physics
    }

    /// Scrolls the viewport to the right by `n` frames.
    pub fn scroll_right(&mut self, n: usize, total_frames: usize) {
        let max_start = total_frames.saturating_sub(self.viewport_size);
        self.viewport_start = (self.viewport_start + n).min(max_start);
    }

    /// Scrolls the viewport to the left by `n` frames.
    pub fn scroll_left(&mut self, n: usize) {
        self.viewport_start = self.viewport_start.saturating_sub(n);
    }

    /// Scrolls to the end (most recent frames).
    pub fn scroll_to_end(&mut self, total_frames: usize) {
        self.viewport_start = total_frames.saturating_sub(self.viewport_size);
    }

    /// Scrolls to the beginning.
    pub fn scroll_to_start(&mut self) {
        self.viewport_start = 0;
    }

    /// Zooms in (fewer frames visible, more detail).
    pub fn zoom_in(&mut self) {
        if self.viewport_size > 10 {
            self.viewport_size /= 2;
            if self.viewport_size < 10 {
                self.viewport_size = 10;
            }
        }
    }

    /// Zooms out (more frames visible, less detail).
    pub fn zoom_out(&mut self, total_frames: usize) {
        self.viewport_size = (self.viewport_size * 2).min(total_frames.max(10));
    }

    /// Returns the current viewport size.
    pub fn viewport_size(&self) -> usize {
        self.viewport_size
    }

    /// Returns the current viewport start index.
    pub fn viewport_start(&self) -> usize {
        self.viewport_start
    }

    /// Selects a frame by viewport-relative index.
    pub fn select(&mut self, index: usize) {
        if index < self.viewport_size {
            self.selected_index = Some(index);
        }
    }

    /// Clears the selection.
    pub fn deselect(&mut self) {
        self.selected_index = None;
    }

    /// Returns the selected viewport index, if any.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    /// Generates the bar data for the current viewport from profiler frames.
    pub fn bars(&self, frames: &[FrameProfile]) -> Vec<FrameGraphBar> {
        let end = (self.viewport_start + self.viewport_size).min(frames.len());
        let visible = &frames[self.viewport_start..end];

        let y_max = self.y_max_ms.unwrap_or_else(|| {
            visible
                .iter()
                .map(|f| f.cpu_time_ms())
                .fold(self.target_frame_ms, f64::max)
                * 1.1 // 10% headroom
        });

        visible
            .iter()
            .map(|f| {
                let cpu_ms = f.cpu_time_ms();
                FrameGraphBar {
                    frame_number: f.frame_number,
                    cpu_ms,
                    gpu_ms: f.gpu_time_ms(),
                    physics_ms: f.physics_time_ms(),
                    normalized_height: if y_max > 0.0 {
                        (cpu_ms / y_max).min(1.0)
                    } else {
                        0.0
                    },
                    is_spike: cpu_ms > self.target_frame_ms,
                }
            })
            .collect()
    }

    /// Generates tooltip data for a specific viewport index.
    pub fn tooltip(
        &self,
        frames: &[FrameProfile],
        viewport_index: usize,
    ) -> Option<FrameGraphTooltip> {
        let abs_index = self.viewport_start + viewport_index;
        let frame = frames.get(abs_index)?;

        let mut top_funcs: Vec<(String, f64)> = frame
            .entries
            .iter()
            .map(|e| (e.name.clone(), e.time_ms()))
            .collect();
        top_funcs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        top_funcs.truncate(self.tooltip_top_n);

        Some(FrameGraphTooltip {
            frame_number: frame.frame_number,
            cpu_ms: frame.cpu_time_ms(),
            gpu_ms: frame.gpu_time_ms(),
            physics_ms: frame.physics_time_ms(),
            fps: frame.fps(),
            top_functions: top_funcs,
        })
    }

    /// Returns tooltip for the currently selected frame.
    pub fn selected_tooltip(&self, frames: &[FrameProfile]) -> Option<FrameGraphTooltip> {
        self.selected_index.and_then(|i| self.tooltip(frames, i))
    }

    /// Returns the target FPS line position as normalized height (0.0–1.0).
    pub fn target_line_height(&self, frames: &[FrameProfile]) -> f64 {
        let end = (self.viewport_start + self.viewport_size).min(frames.len());
        let visible = &frames[self.viewport_start..end];
        let y_max = self.y_max_ms.unwrap_or_else(|| {
            visible
                .iter()
                .map(|f| f.cpu_time_ms())
                .fold(self.target_frame_ms, f64::max)
                * 1.1
        });
        if y_max > 0.0 {
            (self.target_frame_ms / y_max).min(1.0)
        } else {
            0.0
        }
    }

    /// Returns the number of spike frames in the current viewport.
    pub fn spike_count(&self, frames: &[FrameProfile]) -> usize {
        self.bars(frames).iter().filter(|b| b.is_spike).count()
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

    // ── Spike detection ─────────────────────────────────────────────

    #[test]
    fn panel_spike_frames() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();
        panel.record_frame(dur_ms(10), Duration::ZERO, Duration::ZERO, vec![]);
        panel.record_frame(dur_ms(50), Duration::ZERO, Duration::ZERO, vec![]);
        panel.record_frame(dur_ms(15), Duration::ZERO, Duration::ZERO, vec![]);
        panel.record_frame(dur_ms(40), Duration::ZERO, Duration::ZERO, vec![]);

        let spikes = panel.spike_frames(dur_ms(33));
        assert_eq!(spikes.len(), 2);
        assert_eq!(spikes[0].frame_number, 2);
        assert_eq!(spikes[1].frame_number, 4);
    }

    #[test]
    fn panel_spike_frames_none() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();
        panel.record_frame(dur_ms(10), Duration::ZERO, Duration::ZERO, vec![]);

        let spikes = panel.spike_frames(dur_ms(33));
        assert!(spikes.is_empty());
    }

    // ── Percentiles ─────────────────────────────────────────────────

    #[test]
    fn panel_percentile_cpu_ms() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();
        // 10 frames: 1ms, 2ms, ..., 10ms
        for i in 1..=10 {
            panel.record_frame(dur_ms(i), Duration::ZERO, Duration::ZERO, vec![]);
        }

        let p50 = panel.percentile_cpu_ms(50.0);
        assert!(p50 >= 4.0 && p50 <= 7.0, "p50={}", p50); // ~median

        let p90 = panel.percentile_cpu_ms(90.0);
        assert!(p90 >= 9.0);

        let p0 = panel.percentile_cpu_ms(0.0);
        assert!((p0 - 1.0).abs() < 0.01);

        let p100 = panel.percentile_cpu_ms(100.0);
        assert!((p100 - 10.0).abs() < 0.01);
    }

    #[test]
    fn panel_percentile_empty() {
        let panel = ProfilerPanel::new(100);
        assert!((panel.percentile_cpu_ms(50.0)).abs() < 0.01);
    }

    #[test]
    fn panel_percentile_gpu_ms() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();
        panel.record_frame(Duration::ZERO, dur_ms(5), Duration::ZERO, vec![]);
        panel.record_frame(Duration::ZERO, dur_ms(15), Duration::ZERO, vec![]);

        let p50 = panel.percentile_gpu_ms(50.0);
        assert!(p50 >= 5.0 && p50 <= 15.0);
    }

    // ── Histogram ───────────────────────────────────────────────────

    #[test]
    fn panel_cpu_time_histogram() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();
        // Spread across a range: 10, 12, 14, 16, 18, 20
        for i in 0..6 {
            panel.record_frame(dur_ms(10 + i * 2), Duration::ZERO, Duration::ZERO, vec![]);
        }

        let hist = panel.cpu_time_histogram(3);
        assert_eq!(hist.len(), 3);
        let total: usize = hist.iter().map(|b| b.2).sum();
        assert_eq!(total, 6);
        // Each bucket should have some frames
        assert!(hist.iter().all(|b| b.0 < b.1)); // lo < hi
    }

    #[test]
    fn panel_cpu_time_histogram_empty() {
        let panel = ProfilerPanel::new(100);
        let hist = panel.cpu_time_histogram(5);
        assert!(hist.is_empty());
    }

    #[test]
    fn panel_cpu_time_histogram_uniform() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();
        for _ in 0..10 {
            panel.record_frame(dur_ms(16), Duration::ZERO, Duration::ZERO, vec![]);
        }

        let hist = panel.cpu_time_histogram(3);
        // All same value → single bucket
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].2, 10);
    }

    // ── Function hotspots ───────────────────────────────────────────

    #[test]
    fn panel_function_hotspots() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();

        panel.record_frame(
            dur_ms(20),
            Duration::ZERO,
            Duration::ZERO,
            vec![
                ProfilerEntry::new("render", dur_ms(10), 1),
                ProfilerEntry::new("physics", dur_ms(5), 1),
                ProfilerEntry::new("script", dur_ms(3), 1),
            ],
        );

        let hotspots = panel.function_hotspots();
        assert_eq!(hotspots.len(), 3);
        assert_eq!(hotspots[0].0, "render"); // highest percentage
        assert!(hotspots[0].2 > hotspots[1].2); // render% > physics%
                                                // Percentages should sum to < 100 (functions don't account for all CPU time)
        let total_pct: f64 = hotspots.iter().map(|h| h.2).sum();
        assert!(total_pct <= 100.0 + 0.01);
    }

    #[test]
    fn panel_function_hotspots_empty() {
        let panel = ProfilerPanel::new(100);
        assert!(panel.function_hotspots().is_empty());
    }

    // ── Markers ─────────────────────────────────────────────────────

    #[test]
    fn panel_markers() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();
        panel.record_frame(dur_ms(16), Duration::ZERO, Duration::ZERO, vec![]);
        panel.record_frame(dur_ms(50), Duration::ZERO, Duration::ZERO, vec![]);
        panel.record_frame(dur_ms(16), Duration::ZERO, Duration::ZERO, vec![]);

        panel.add_marker(2, "spike!".to_string());
        panel.add_marker(3, "recovered".to_string());

        assert_eq!(panel.markers().len(), 2);
        assert_eq!(panel.markers()[0].label, "spike!");
    }

    #[test]
    fn panel_markers_in_range() {
        let mut panel = ProfilerPanel::new(100);
        panel.add_marker(1, "a".to_string());
        panel.add_marker(5, "b".to_string());
        panel.add_marker(10, "c".to_string());

        let in_range = panel.markers_in_range(3, 8);
        assert_eq!(in_range.len(), 1);
        assert_eq!(in_range[0].label, "b");
    }

    #[test]
    fn panel_start_clears_markers() {
        let mut panel = ProfilerPanel::new(100);
        panel.add_marker(1, "old".to_string());
        panel.start();
        assert!(panel.markers().is_empty());
    }

    // ── Compare ranges ──────────────────────────────────────────────

    #[test]
    fn panel_compare_ranges() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();

        // Range A: frames 1-3 at 10ms
        for _ in 0..3 {
            panel.record_frame(dur_ms(10), Duration::ZERO, Duration::ZERO, vec![]);
        }
        // Range B: frames 4-6 at 20ms
        for _ in 0..3 {
            panel.record_frame(dur_ms(20), Duration::ZERO, Duration::ZERO, vec![]);
        }

        let (a, b) = panel.compare_ranges((1, 3), (4, 6));
        assert_eq!(a.frame_count, 3);
        assert_eq!(b.frame_count, 3);
        assert!((a.avg_cpu_ms - 10.0).abs() < 0.01);
        assert!((b.avg_cpu_ms - 20.0).abs() < 0.01);
    }

    // ── Summary line ────────────────────────────────────────────────

    #[test]
    fn panel_summary_line_empty() {
        let panel = ProfilerPanel::new(100);
        assert_eq!(panel.summary_line(), "No profiling data");
    }

    #[test]
    fn panel_summary_line_with_data() {
        let mut panel = ProfilerPanel::new(100);
        panel.start();
        panel.record_frame(dur_ms(16), dur_ms(10), Duration::ZERO, vec![]);
        panel.record_frame(dur_ms(50), dur_ms(10), Duration::ZERO, vec![]);

        let line = panel.summary_line();
        assert!(line.contains("Frames: 2"));
        assert!(line.contains("CPU:"));
        assert!(line.contains("GPU:"));
        assert!(line.contains("Spikes"));
    }

    // ── FrameGraph ──────────────────────────────────────────────────

    fn make_test_frames(count: usize) -> Vec<FrameProfile> {
        (0..count)
            .map(|i| {
                let mut f = FrameProfile::new(i as u64 + 1);
                f.cpu_time = dur_ms(10 + (i as u64 % 5) * 5); // 10, 15, 20, 25, 30, 10, ...
                f.gpu_time = dur_ms(8 + (i as u64 % 3) * 2);
                f.physics_time = dur_ms(3);
                f.add_entry("render", dur_ms(5 + (i as u64 % 3) * 2), 1);
                f.add_entry("physics", dur_ms(3), 1);
                f
            })
            .collect()
    }

    #[test]
    fn frame_graph_default() {
        let g = FrameGraph::default();
        assert_eq!(g.viewport_size(), 200);
        assert!((g.target_frame_ms() - 16.667).abs() < 0.01);
        assert!(g.show_gpu());
        assert!(g.show_physics());
        assert!(g.selected_index().is_none());
    }

    #[test]
    fn frame_graph_bars_basic() {
        let frames = make_test_frames(10);
        let g = FrameGraph::new(200);

        let bars = g.bars(&frames);
        assert_eq!(bars.len(), 10);
        assert_eq!(bars[0].frame_number, 1);
        assert!(bars[0].cpu_ms >= 10.0);
        assert!(bars[0].normalized_height >= 0.0 && bars[0].normalized_height <= 1.0);
    }

    #[test]
    fn frame_graph_bars_normalized_height() {
        let mut frames = vec![];
        let mut f1 = FrameProfile::new(1);
        f1.cpu_time = dur_ms(10);
        frames.push(f1);
        let mut f2 = FrameProfile::new(2);
        f2.cpu_time = dur_ms(30);
        frames.push(f2);

        let mut g = FrameGraph::new(200);
        g.set_y_max(Some(30.0));

        let bars = g.bars(&frames);
        assert!((bars[0].normalized_height - 10.0 / 30.0).abs() < 0.01);
        assert!((bars[1].normalized_height - 1.0).abs() < 0.01);
    }

    #[test]
    fn frame_graph_spike_detection() {
        let mut frames = vec![];
        for i in 0..5 {
            let mut f = FrameProfile::new(i + 1);
            f.cpu_time = dur_ms(if i == 2 { 50 } else { 10 });
            frames.push(f);
        }

        let g = FrameGraph::new(200);
        let bars = g.bars(&frames);
        // Only frame 3 (50ms) exceeds target (16.67ms)
        assert!(!bars[0].is_spike);
        assert!(bars[2].is_spike);
    }

    #[test]
    fn frame_graph_set_target_fps() {
        let mut g = FrameGraph::default();
        g.set_target_fps(30.0);
        assert!((g.target_frame_ms() - 33.333).abs() < 0.01);
    }

    #[test]
    fn frame_graph_scroll() {
        let frames = make_test_frames(100);
        let mut g = FrameGraph::new(20);

        assert_eq!(g.viewport_start(), 0);

        g.scroll_right(10, frames.len());
        assert_eq!(g.viewport_start(), 10);

        g.scroll_left(5);
        assert_eq!(g.viewport_start(), 5);

        g.scroll_to_end(frames.len());
        assert_eq!(g.viewport_start(), 80); // 100 - 20

        g.scroll_to_start();
        assert_eq!(g.viewport_start(), 0);
    }

    #[test]
    fn frame_graph_scroll_clamping() {
        let frames = make_test_frames(10);
        let mut g = FrameGraph::new(20);

        g.scroll_right(1000, frames.len());
        // Can't scroll past end — viewport_size (20) > total (10), so max_start = 0
        assert_eq!(g.viewport_start(), 0);

        g.scroll_left(1000);
        assert_eq!(g.viewport_start(), 0);
    }

    #[test]
    fn frame_graph_zoom() {
        let frames = make_test_frames(100);
        let mut g = FrameGraph::new(100);

        g.zoom_in();
        assert_eq!(g.viewport_size(), 50);

        g.zoom_in();
        assert_eq!(g.viewport_size(), 25);

        g.zoom_out(frames.len());
        assert_eq!(g.viewport_size(), 50);

        // Zoom out to max
        for _ in 0..10 {
            g.zoom_out(frames.len());
        }
        assert_eq!(g.viewport_size(), 100); // capped at total frames
    }

    #[test]
    fn frame_graph_zoom_minimum() {
        let mut g = FrameGraph::new(10);
        g.zoom_in(); // 10 is minimum
        assert_eq!(g.viewport_size(), 10);
    }

    #[test]
    fn frame_graph_select() {
        let mut g = FrameGraph::new(50);
        assert!(g.selected_index().is_none());

        g.select(5);
        assert_eq!(g.selected_index(), Some(5));

        g.deselect();
        assert!(g.selected_index().is_none());
    }

    #[test]
    fn frame_graph_select_out_of_bounds() {
        let mut g = FrameGraph::new(10);
        g.select(100); // beyond viewport size
        assert!(g.selected_index().is_none());
    }

    #[test]
    fn frame_graph_tooltip() {
        let frames = make_test_frames(5);
        let g = FrameGraph::new(200);

        let tt = g.tooltip(&frames, 0).unwrap();
        assert_eq!(tt.frame_number, 1);
        assert!(tt.cpu_ms >= 10.0);
        assert!(tt.fps > 0.0);
        assert!(!tt.top_functions.is_empty());
    }

    #[test]
    fn frame_graph_tooltip_out_of_bounds() {
        let frames = make_test_frames(5);
        let g = FrameGraph::new(200);

        assert!(g.tooltip(&frames, 100).is_none());
    }

    #[test]
    fn frame_graph_selected_tooltip() {
        let frames = make_test_frames(5);
        let mut g = FrameGraph::new(200);

        assert!(g.selected_tooltip(&frames).is_none());

        g.select(2);
        let tt = g.selected_tooltip(&frames).unwrap();
        assert_eq!(tt.frame_number, 3);
    }

    #[test]
    fn frame_graph_tooltip_top_functions_sorted() {
        let mut frame = FrameProfile::new(1);
        frame.cpu_time = dur_ms(20);
        frame.add_entry("slow", dur_ms(10), 1);
        frame.add_entry("fast", dur_ms(1), 1);
        frame.add_entry("medium", dur_ms(5), 1);

        let g = FrameGraph::new(200);
        let tt = g.tooltip(&[frame], 0).unwrap();
        assert_eq!(tt.top_functions[0].0, "slow");
        assert_eq!(tt.top_functions[1].0, "medium");
        assert_eq!(tt.top_functions[2].0, "fast");
    }

    #[test]
    fn frame_graph_target_line_height() {
        let mut frames = vec![];
        let mut f = FrameProfile::new(1);
        f.cpu_time = dur_ms(33); // exactly 2x target
        frames.push(f);

        let mut g = FrameGraph::new(200);
        g.set_target_fps(60.0); // target = 16.67ms

        let h = g.target_line_height(&frames);
        // y_max ≈ 33 * 1.1 = 36.3, target line ≈ 16.67/36.3 ≈ 0.459
        assert!(h > 0.3 && h < 0.6, "h={}", h);
    }

    #[test]
    fn frame_graph_spike_count() {
        let mut frames = vec![];
        for i in 0..10 {
            let mut f = FrameProfile::new(i + 1);
            f.cpu_time = dur_ms(if i % 3 == 0 { 50 } else { 10 });
            frames.push(f);
        }

        let g = FrameGraph::new(200);
        let count = g.spike_count(&frames);
        assert_eq!(count, 4); // frames 0, 3, 6, 9
    }

    #[test]
    fn frame_graph_toggle_gpu_physics() {
        let mut g = FrameGraph::default();
        assert!(g.show_gpu());
        g.toggle_gpu();
        assert!(!g.show_gpu());
        g.toggle_gpu();
        assert!(g.show_gpu());

        assert!(g.show_physics());
        g.toggle_physics();
        assert!(!g.show_physics());
    }

    #[test]
    fn frame_graph_bars_empty_frames() {
        let g = FrameGraph::new(200);
        let bars = g.bars(&[]);
        assert!(bars.is_empty());
    }

    #[test]
    fn frame_graph_viewport_clipping() {
        let frames = make_test_frames(5);
        let mut g = FrameGraph::new(3);
        g.scroll_right(2, frames.len());

        let bars = g.bars(&frames);
        assert_eq!(bars.len(), 3);
        assert_eq!(bars[0].frame_number, 3);
        assert_eq!(bars[2].frame_number, 5);
    }
}
