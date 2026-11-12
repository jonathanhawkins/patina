use godot::prelude::*;

mod classdb_probe;
mod property_probe;
mod resource_probe;
mod scene_probe;
mod signal_probe;
mod api_surface_probe;
mod enum_constants_probe;
mod inheritance_probe;
mod method_defaults_probe;
mod node_defaults_probe;
mod resource_roundtrip_probe;
mod resource_subtype_probe;
mod resource_validation_probe;
mod singleton_probe;
mod virtual_methods_probe;

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

    /// Run resource metadata probe for a given path.
    #[func]
    fn run_resource_probe(&self, path: GString) {
        resource_probe::emit_for_path(&path.to_string());
    }

    /// Run ClassDB probe for the 17 core classes.
    #[func]
    fn run_classdb_probe(&self) {
        classdb_probe::emit();
    }

    /// Run `api_surface_probe` (oracle capture; see apps/godot/README.md).
    #[func]
    fn run_api_surface_probe(&self) {
        api_surface_probe::emit();
    }

    /// Run `enum_constants_probe` (oracle capture; see apps/godot/README.md).
    #[func]
    fn run_enum_constants_probe(&self) {
        enum_constants_probe::emit();
    }

    /// Run `inheritance_probe` (oracle capture; see apps/godot/README.md).
    #[func]
    fn run_inheritance_probe(&self) {
        inheritance_probe::emit();
    }

    /// Run `method_defaults_probe` (oracle capture; see apps/godot/README.md).
    #[func]
    fn run_method_defaults_probe(&self) {
        method_defaults_probe::emit();
    }

    /// Run `node_defaults_probe` (oracle capture; see apps/godot/README.md).
    #[func]
    fn run_node_defaults_probe(&self) {
        node_defaults_probe::emit();
    }

    /// Run `resource_subtype_probe` (oracle capture; see apps/godot/README.md).
    #[func]
    fn run_resource_subtype_probe(&self) {
        resource_subtype_probe::emit();
    }

    /// Run `singleton_probe` (oracle capture; see apps/godot/README.md).
    #[func]
    fn run_singleton_probe(&self) {
        singleton_probe::emit();
    }

    /// Run `virtual_methods_probe` (oracle capture; see apps/godot/README.md).
    #[func]
    fn run_virtual_methods_probe(&self) {
        virtual_methods_probe::emit();
    }

    /// Round-trip a resource through save/load and report what changed.
    #[func]
    fn run_resource_roundtrip_probe(&self, path: GString, expected_class: GString) {
        let class = expected_class.to_string();
        resource_roundtrip_probe::emit_roundtrip(
            &path.to_string(),
            if class.is_empty() { None } else { Some(class.as_str()) },
        );
    }

    /// Validate a resource's loaded class and properties.
    #[func]
    fn run_resource_validation_probe(&self, path: GString, expected_class: GString) {
        let class = expected_class.to_string();
        resource_validation_probe::emit_validation(
            &path.to_string(),
            if class.is_empty() { None } else { Some(class.as_str()) },
        );
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
