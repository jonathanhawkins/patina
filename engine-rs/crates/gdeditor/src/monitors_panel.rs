//! The performance **Monitors** panel.
//!
//! While the game runs, the bottom-panel Monitors view samples runtime metrics
//! — frames per second, memory use, draw calls, and physics time — and plots the
//! currently selected monitor as a time series. Sampling only happens during a
//! play session; selecting a different monitor re-plots the graph against that
//! monitor's history.

/// A runtime metric the Monitors panel can graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Monitor {
    /// Frames per second.
    Fps,
    /// Total memory in use (bytes).
    Memory,
    /// Draw calls issued per frame.
    DrawCalls,
    /// Physics step time (milliseconds).
    Physics,
}

/// One time step's worth of sampled metrics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorSample {
    /// Frames per second this step.
    pub fps: f32,
    /// Memory in use this step.
    pub memory: f32,
    /// Draw calls this step.
    pub draw_calls: f32,
    /// Physics time this step.
    pub physics: f32,
}

/// The Monitors panel: per-monitor history plus the current selection.
#[derive(Debug, Clone, Default)]
pub struct MonitorsPanel {
    fps: Vec<f32>,
    memory: Vec<f32>,
    draw_calls: Vec<f32>,
    physics: Vec<f32>,
    selected: Option<Monitor>,
    playing: bool,
}

impl MonitorsPanel {
    /// Creates an empty panel with the FPS monitor selected by default.
    pub fn new() -> Self {
        Self {
            selected: Some(Monitor::Fps),
            ..Default::default()
        }
    }

    /// The monitors the panel can graph, in display order.
    pub fn monitors() -> [Monitor; 4] {
        [
            Monitor::Fps,
            Monitor::Memory,
            Monitor::DrawCalls,
            Monitor::Physics,
        ]
    }

    /// Begins a play session: sampling is enabled.
    pub fn start_play(&mut self) {
        self.playing = true;
    }

    /// Ends the play session: sampling stops (history is retained).
    pub fn stop_play(&mut self) {
        self.playing = false;
    }

    /// Whether a play session is currently sampling.
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Records one time step of metrics. Samples are only retained while a play
    /// session is active; outside play this is a no-op. Returns whether the
    /// sample was recorded.
    pub fn record(&mut self, sample: MonitorSample) -> bool {
        if !self.playing {
            return false;
        }
        self.fps.push(sample.fps);
        self.memory.push(sample.memory);
        self.draw_calls.push(sample.draw_calls);
        self.physics.push(sample.physics);
        true
    }

    /// Selects which monitor the graph plots.
    pub fn select(&mut self, monitor: Monitor) {
        self.selected = Some(monitor);
    }

    /// The currently selected monitor, if any.
    pub fn selected(&self) -> Option<Monitor> {
        self.selected
    }

    /// The recorded history for a specific monitor, oldest sample first.
    pub fn series(&self, monitor: Monitor) -> &[f32] {
        match monitor {
            Monitor::Fps => &self.fps,
            Monitor::Memory => &self.memory,
            Monitor::DrawCalls => &self.draw_calls,
            Monitor::Physics => &self.physics,
        }
    }

    /// The points plotted on the graph: the selected monitor's time series.
    /// Empty when nothing is selected.
    pub fn graph_points(&self) -> &[f32] {
        match self.selected {
            Some(monitor) => self.series(monitor),
            None => &[],
        }
    }

    /// The number of time steps sampled so far.
    pub fn sample_count(&self) -> usize {
        self.fps.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(fps: f32, memory: f32, draw_calls: f32, physics: f32) -> MonitorSample {
        MonitorSample {
            fps,
            memory,
            draw_calls,
            physics,
        }
    }

    /// Acceptance (pat-sx8y8): during play the Monitors panel plots the selected
    /// metrics over time, and selecting a monitor updates the graph.
    #[test]
    fn bottom_monitors_plot_metrics() {
        let mut panel = MonitorsPanel::new();

        // All four monitors are available, in order.
        assert_eq!(
            MonitorsPanel::monitors(),
            [
                Monitor::Fps,
                Monitor::Memory,
                Monitor::DrawCalls,
                Monitor::Physics,
            ]
        );
        // FPS is selected by default; nothing plotted before play.
        assert_eq!(panel.selected(), Some(Monitor::Fps));
        assert!(panel.graph_points().is_empty());

        // Sampling does nothing until a play session starts.
        assert!(!panel.record(sample(60.0, 100.0, 10.0, 2.0)));
        assert_eq!(panel.sample_count(), 0);

        // During play, each step plots its metrics over time.
        panel.start_play();
        assert!(panel.is_playing());
        assert!(panel.record(sample(60.0, 100.0, 10.0, 2.0)));
        assert!(panel.record(sample(58.0, 110.0, 12.0, 2.5)));
        assert!(panel.record(sample(59.0, 115.0, 11.0, 2.2)));
        assert_eq!(panel.sample_count(), 3);

        // The graph plots the selected (FPS) monitor's time series.
        assert_eq!(panel.graph_points(), &[60.0, 58.0, 59.0]);

        // Selecting a different monitor updates the graph to that metric.
        panel.select(Monitor::Memory);
        assert_eq!(panel.selected(), Some(Monitor::Memory));
        assert_eq!(panel.graph_points(), &[100.0, 110.0, 115.0]);

        panel.select(Monitor::DrawCalls);
        assert_eq!(panel.graph_points(), &[10.0, 12.0, 11.0]);

        panel.select(Monitor::Physics);
        assert_eq!(panel.graph_points(), &[2.0, 2.5, 2.2]);

        // Every monitor accumulated the same number of points over time.
        for m in MonitorsPanel::monitors() {
            assert_eq!(panel.series(m).len(), 3, "monitor {m:?} plotted over time");
        }

        // Stopping play keeps the history but stops new sampling.
        panel.stop_play();
        assert!(!panel.is_playing());
        assert!(!panel.record(sample(50.0, 120.0, 9.0, 3.0)));
        assert_eq!(panel.sample_count(), 3, "no new samples after play ends");
        // The previously plotted history is still graphable.
        assert_eq!(panel.graph_points(), &[2.0, 2.5, 2.2]);
    }
}
