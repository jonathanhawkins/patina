//! Measured audio quality — the ear for an agent that cannot hear.
//!
//! "The music crackles" is a claim about samples, and every cause leaves a
//! measurable mark in the stream the device actually played:
//! - an underrun or gap is a run of exact-zero frames inside live audio;
//! - a click is a sample-to-sample jump far larger than the signal's own local
//!   curvature (a discontinuity: a block seam, a cursor that snapped, a loop
//!   point that does not line up);
//! - clipping is a sample outside `[-1, 1]`.
//!
//! [`analyze`] reports all three with timestamps, so a fix can be judged on the
//! same capture the listener heard instead of on a synthetic tone.

/// One detected discontinuity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Click {
    /// Seconds from the start of the analysed buffer.
    pub time: f32,
    /// How many times the local curvature the jump was.
    pub severity: f32,
}

/// One run of silence inside otherwise-live audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gap {
    pub time: f32,
    pub frames: usize,
}

/// What [`analyze`] measured.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioReport {
    pub seconds: f32,
    pub peak: f32,
    pub rms: f32,
    pub clipped_samples: usize,
    pub clicks: Vec<Click>,
    pub gaps: Vec<Gap>,
}

/// Window, in frames, over which "local curvature" is averaged.
const WINDOW: usize = 256;
/// A jump this many times the local curvature is a click.
pub const CLICK_FACTOR: f32 = 8.0;
/// Below this second-difference a jump is inaudible whatever its ratio.
const CLICK_FLOOR: f32 = 0.02;
/// Frames of exact zero before a jump that make it a stream ONSET, not a click.
const ONSET_SILENCE: usize = 32;
/// Shortest all-zero run reported as a gap (~0.7 ms at 48 kHz).
const MIN_GAP_FRAMES: usize = 32;
/// A frame quieter than this counts as silent.
const SILENT: f32 = 1e-6;

/// Measures an interleaved buffer.
pub fn analyze(samples: &[f32], sample_rate: u32, channels: u16) -> AudioReport {
    let ch = channels.max(1) as usize;
    let frames = samples.len() / ch;
    let rate = sample_rate.max(1) as f32;
    let mut report = AudioReport {
        seconds: frames as f32 / rate,
        ..AudioReport::default()
    };
    if frames == 0 {
        return report;
    }
    let mut sum_sq = 0.0f64;
    for &s in samples {
        report.peak = report.peak.max(s.abs());
        if !(-1.0..=1.0).contains(&s) {
            report.clipped_samples += 1;
        }
        sum_sq += f64::from(s) * f64::from(s);
    }
    report.rms = (sum_sq / samples.len() as f64).sqrt() as f32;

    // Gaps: all channels silent for a run, with live audio on both sides.
    let silent = |f: usize| (0..ch).all(|c| samples[f * ch + c].abs() < SILENT);
    let mut f = 0;
    let mut seen_live = false;
    while f < frames {
        if silent(f) {
            let start = f;
            while f < frames && silent(f) {
                f += 1;
            }
            let run = f - start;
            if seen_live && f < frames && run >= MIN_GAP_FRAMES {
                report.gaps.push(Gap {
                    time: start as f32 / rate,
                    frames: run,
                });
            }
        } else {
            seen_live = true;
            f += 1;
        }
    }

    // Clicks: |second difference| against its own running mean, per channel.
    for c in 0..ch {
        let x = |i: usize| samples[i * ch + c];
        let d2: Vec<f32> = (2..frames)
            .map(|i| (x(i) - 2.0 * x(i - 1) + x(i - 2)).abs())
            .collect();
        let mut window_sum: f32 = 0.0;
        for (i, &v) in d2.iter().enumerate() {
            window_sum += v;
            if i >= WINDOW {
                window_sum -= d2[i - WINDOW];
            }
            let n = (i + 1).min(WINDOW) as f32;
            // The mean EXCLUDING this sample, so a click does not raise its
            // own threshold.
            let local = ((window_sum - v) / (n - 1.0).max(1.0)).max(1e-6);
            // A sound STARTING out of exact digital silence is the asset's own
            // first sample (Godot plays it with no fade either — Dodge the
            // Creeps' music opens at 0.017), not a mix discontinuity. A
            // mid-stream dropout that resumes is still reported, as a GAP.
            // `d2[i]` spans samples i..=i+2, so the two values straddling the
            // first non-zero sample both have their OLDEST sample, `x(i)`,
            // still in the silence.
            let from_silence =
                i + 1 >= ONSET_SILENCE && (i + 1 - ONSET_SILENCE..=i).all(|k| x(k) == 0.0);
            if i > WINDOW && v > CLICK_FLOOR && v > local * CLICK_FACTOR && !from_silence {
                let time = (i + 2) as f32 / rate;
                // One click per 5 ms, the loudest kept.
                match report.clicks.last_mut() {
                    Some(last) if time - last.time < 0.005 => {
                        last.severity = last.severity.max(v / local);
                    }
                    _ => report.clicks.push(Click {
                        time,
                        severity: v / local,
                    }),
                }
            }
        }
    }
    report
        .clicks
        .sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32, rate: u32) -> Vec<f32> {
        (0..(secs * rate as f32) as usize)
            .map(|i| (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin() * 0.5)
            .collect()
    }

    #[test]
    fn a_clean_tone_has_no_clicks_gaps_or_clipping() {
        let r = analyze(&tone(1.0, 48_000), 48_000, 1);
        assert!(r.clicks.is_empty(), "{:?}", r.clicks);
        assert!(r.gaps.is_empty());
        assert_eq!(r.clipped_samples, 0);
        assert!((r.peak - 0.5).abs() < 0.01);
    }

    #[test]
    fn a_phase_jump_is_a_click_at_its_time() {
        let mut t = tone(1.0, 48_000);
        // Skip 20 samples at 0.5 s: a discontinuity like a snapped cursor.
        t.drain(24_000..24_020);
        let r = analyze(&t, 48_000, 1);
        assert_eq!(r.clicks.len(), 1, "{:?}", r.clicks);
        assert!((r.clicks[0].time - 0.5).abs() < 0.002);
    }

    #[test]
    fn a_sound_starting_from_silence_is_not_a_click() {
        // Silence, then a tone that opens mid-waveform (like a music file whose
        // first sample is non-zero): an onset, not a mix discontinuity.
        let mut t = vec![0.0f32; 24_000];
        t.extend(tone(0.5, 48_000).into_iter().skip(10));
        let r = analyze(&t, 48_000, 1);
        assert!(r.clicks.is_empty(), "{:?}", r.clicks);
    }

    #[test]
    fn a_jump_after_near_silence_is_still_a_click() {
        // Edge: quiet-but-not-zero audio before the jump is a live signal.
        let mut t = vec![1e-4f32; 24_000];
        t.extend(tone(0.5, 48_000).into_iter().skip(10));
        let r = analyze(&t, 48_000, 1);
        assert_eq!(r.clicks.len(), 1, "{:?}", r.clicks);
    }

    #[test]
    fn a_dropout_is_a_gap_and_clipping_is_counted() {
        let mut t = tone(1.0, 48_000);
        for s in &mut t[10_000..10_500] {
            *s = 0.0;
        }
        t[30_000] = 1.5;
        let r = analyze(&t, 48_000, 1);
        assert_eq!(r.gaps.len(), 1);
        assert_eq!(r.gaps[0].frames, 500);
        assert_eq!(r.clipped_samples, 1);
    }
}
