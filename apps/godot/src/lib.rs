use godot::prelude::*;

mod property_probe;
mod scene_probe;
mod signal_probe;

#[derive(GodotClass)]
#[class(base = Node)]
struct PatinaSmokeProbe {
    #[base]
    base: Base<Node>,
    #[var]
    probe_label: GString,
    #[var]
    probe_count: i32,
    signal_events: Vec<String>,
}

#[godot_api]
impl INode for PatinaSmokeProbe {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            probe_label: GString::from("smoke"),
            probe_count: 1,
            signal_events: Vec::new(),
        }
    }
}

#[godot_api]
impl PatinaSmokeProbe {
    #[signal]
    fn probe_signal(stage: GString);

    #[func]
    fn run_smoke_probe(&mut self) {
        let mut base = self.base().clone();
        scene_probe::emit(&base);
        property_probe::emit(&base, &self.probe_label, self.probe_count);
        signal_probe::emit(self, &mut base);
    }

    #[func]
    fn record_probe_signal(&mut self, stage: GString) {
        self.signal_events.push(stage.to_string());
    }

    fn signal_events(&self) -> &[String] {
        &self.signal_events
    }

    fn push_signal_event(&mut self, stage: impl Into<String>) {
        self.signal_events.push(stage.into());
    }
}

struct PatinaGodotLab;

#[gdextension]
unsafe impl ExtensionLibrary for PatinaGodotLab {}
