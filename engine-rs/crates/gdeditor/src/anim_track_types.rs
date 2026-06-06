//! Core **animation track types** (pat-1jsf7).
//!
//! The animation editor supports several track kinds — property/value,
//! transform, method-call, audio, and (sub-)animation. Each track type can be
//! added to an animation, renders a type-appropriate row label, and stores its
//! keys in the correct typed form: inserting a key whose payload doesn't match
//! the track's type is rejected.

/// The kind of an animation track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackType {
    /// A property/value track.
    Value,
    /// A 3D transform track.
    Transform,
    /// A method-call track.
    MethodCall,
    /// An audio playback track.
    Audio,
    /// A nested-animation track.
    Animation,
}

impl TrackType {
    /// The human-readable type name shown in the track row.
    pub fn label(self) -> &'static str {
        match self {
            TrackType::Value => "Value",
            TrackType::Transform => "Transform",
            TrackType::MethodCall => "MethodCall",
            TrackType::Audio => "Audio",
            TrackType::Animation => "Animation",
        }
    }
}

/// A typed key payload — its variant must match the owning track's type.
#[derive(Debug, Clone, PartialEq)]
pub enum KeyValue {
    /// A property value (a serialized variant).
    Value(String),
    /// A transform: position, rotation (euler), and scale.
    Transform {
        /// Position xyz.
        position: (f32, f32, f32),
        /// Euler rotation xyz.
        rotation: (f32, f32, f32),
        /// Scale xyz.
        scale: (f32, f32, f32),
    },
    /// A method call with arguments.
    MethodCall {
        /// Method name.
        method: String,
        /// Argument list (serialized).
        args: Vec<String>,
    },
    /// An audio stream to play.
    Audio {
        /// Stream resource path.
        stream: String,
    },
    /// A sub-animation to play.
    Animation {
        /// Animation name.
        animation: String,
    },
}

impl KeyValue {
    /// Whether this key payload is valid for `track_type`.
    pub fn matches(&self, track_type: TrackType) -> bool {
        matches!(
            (track_type, self),
            (TrackType::Value, KeyValue::Value(_))
                | (TrackType::Transform, KeyValue::Transform { .. })
                | (TrackType::MethodCall, KeyValue::MethodCall { .. })
                | (TrackType::Audio, KeyValue::Audio { .. })
                | (TrackType::Animation, KeyValue::Animation { .. })
        )
    }
}

/// A single track: a type, its target path, and its keys.
#[derive(Debug, Clone)]
pub struct Track {
    /// The track type.
    pub track_type: TrackType,
    /// The target node/property path.
    pub target: String,
    /// Keys as `(time, value)`, kept sorted by time.
    pub keys: Vec<(f32, KeyValue)>,
}

/// An animation made of typed tracks.
#[derive(Debug, Clone, Default)]
pub struct TrackedAnimation {
    tracks: Vec<Track>,
}

impl TrackedAnimation {
    /// Creates an empty animation.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a track of `track_type` targeting `target`, returning its index.
    pub fn add_track(&mut self, track_type: TrackType, target: impl Into<String>) -> usize {
        self.tracks.push(Track {
            track_type,
            target: target.into(),
            keys: Vec::new(),
        });
        self.tracks.len() - 1
    }

    /// The number of tracks.
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// The track at `index`.
    pub fn track(&self, index: usize) -> Option<&Track> {
        self.tracks.get(index)
    }

    /// The type-appropriate row label for a track (`"Type: target"`).
    pub fn row_label(&self, index: usize) -> Option<String> {
        self.tracks
            .get(index)
            .map(|t| format!("{}: {}", t.track_type.label(), t.target))
    }

    /// Inserts a key at `time` on the track at `index`, but only if the key's
    /// payload matches the track's type. Returns whether the key was stored.
    pub fn insert_key(&mut self, index: usize, time: f32, value: KeyValue) -> bool {
        let track = match self.tracks.get_mut(index) {
            Some(t) => t,
            None => return false,
        };
        if !value.matches(track.track_type) {
            return false;
        }
        track.keys.push((time, value));
        track
            .keys
            .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        true
    }

    /// The keys of the track at `index`.
    pub fn keys(&self, index: usize) -> Option<&[(f32, KeyValue)]> {
        self.tracks.get(index).map(|t| t.keys.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Acceptance (pat-1jsf7): each supported track type can be added, renders
    /// its type-appropriate row, and stores keys in the correct typed form.
    #[test]
    fn anim_track_types() {
        let mut anim = TrackedAnimation::new();

        // Add one of each core track type.
        let v = anim.add_track(TrackType::Value, "Sprite2D:modulate");
        let t = anim.add_track(TrackType::Transform, "Node3D");
        let m = anim.add_track(TrackType::MethodCall, "Player");
        let a = anim.add_track(TrackType::Audio, "AudioStreamPlayer");
        let n = anim.add_track(TrackType::Animation, "AnimationPlayer");
        assert_eq!(anim.track_count(), 5);

        // Each renders a type-appropriate row.
        assert_eq!(anim.row_label(v).unwrap(), "Value: Sprite2D:modulate");
        assert_eq!(anim.row_label(t).unwrap(), "Transform: Node3D");
        assert_eq!(anim.row_label(m).unwrap(), "MethodCall: Player");
        assert_eq!(anim.row_label(a).unwrap(), "Audio: AudioStreamPlayer");
        assert_eq!(anim.row_label(n).unwrap(), "Animation: AnimationPlayer");

        // Each stores keys in its correct typed form.
        assert!(anim.insert_key(v, 0.0, KeyValue::Value("Color(1,1,1,1)".to_string())));
        assert!(anim.insert_key(
            t,
            0.0,
            KeyValue::Transform {
                position: (0.0, 0.0, 0.0),
                rotation: (0.0, 0.0, 0.0),
                scale: (1.0, 1.0, 1.0),
            }
        ));
        assert!(anim.insert_key(
            m,
            0.5,
            KeyValue::MethodCall {
                method: "play".to_string(),
                args: vec!["idle".to_string()],
            }
        ));
        assert!(anim.insert_key(a, 0.0, KeyValue::Audio { stream: "res://sfx.wav".to_string() }));
        assert!(anim.insert_key(n, 0.0, KeyValue::Animation { animation: "jump".to_string() }));

        // A key whose payload doesn't match the track type is rejected.
        assert!(!anim.insert_key(v, 1.0, KeyValue::Audio { stream: "x".to_string() }));
        assert!(!anim.insert_key(t, 1.0, KeyValue::Value("oops".to_string())));
        // Inserting onto a missing track fails.
        assert!(!anim.insert_key(99, 0.0, KeyValue::Value("x".to_string())));

        // The stored keys keep their typed form.
        assert_eq!(anim.keys(v).unwrap().len(), 1);
        match &anim.keys(v).unwrap()[0].1 {
            KeyValue::Value(s) => assert_eq!(s, "Color(1,1,1,1)"),
            other => panic!("expected Value, got {other:?}"),
        }
        match &anim.keys(m).unwrap()[0].1 {
            KeyValue::MethodCall { method, args } => {
                assert_eq!(method, "play");
                assert_eq!(args, &vec!["idle".to_string()]);
            }
            other => panic!("expected MethodCall, got {other:?}"),
        }
        // The mismatched insert didn't add a key.
        assert_eq!(anim.keys(v).unwrap().len(), 1);
    }
}
