//! Script-backed `EditorScenePostImport` — `@tool extends EditorScenePostImport`
//! with `_post_import(scene)` (pat-fk17e).
//!
//! # What was already here, and what was actually missing
//!
//! The Rust side of this hook was already complete: [`EditorScenePostImport`]
//! is a real trait, [`SceneFormatImporterRegistry`] keeps a `post_importers`
//! list, and `import_scene` threads the scene through every registered hook
//! (`import.rs`, the `for post in &self.post_importers` loop). What did not
//! exist was any way for a **GDScript** to be one of those hooks — which is
//! this epic's gap, not the hook itself.
//!
//! # ⚠️ Why the script gets a real node, not a Dictionary
//!
//! Godot's signature is `_post_import(scene: Node) -> Node`, so a script
//! written for Godot does `scene.some_property = x` and `return scene`. Handing
//! it a Dictionary would compile, run, and silently fail every real script —
//! the "wired but not correct" shape. So the imported tree is materialized into
//! a throwaway [`SceneTree`], the script receives the root as a
//! `Variant::ObjectId`, and the mutated tree is read back out.
//!
//! That is the same mechanism `char_fx` already uses for `_process_custom_fx`:
//! property writes through an `ObjectId` land in the node's property bag, which
//! is what makes the round trip work without inventing a new Variant type.
//!
//! # ⚠️ Why the script source is stored instead of a live interpreter
//!
//! [`EditorScenePostImport`] requires `Send + Sync` and takes `&self`, while
//! dispatching a virtual needs `&mut` on both the interpreter and the instance.
//! Holding an `Interpreter` here would therefore need interior mutability *and*
//! a guarantee that `Interpreter: Send`, which is not asserted anywhere in the
//! workspace. Storing the source — a `String`, trivially `Send + Sync` — and
//! building the interpreter inside the call sidesteps both questions. Imports
//! are editor-time and rare, so re-parsing per import costs nothing that
//! matters.

use gdcore::error::{EngineError, EngineResult};
use gdscene::node::NodeId;
use gdscene::scripting::GDScriptNodeInstance;
use gdscene::{Node, SceneTree};
use gdvariant::Variant;

use crate::import::{EditorScenePostImport, ImportedScene, ImportedSceneNode};

/// The virtual Godot calls on an `EditorScenePostImport` script.
const POST_IMPORT: &str = "_post_import";

/// The property name a script writes to rename a node, mirroring Godot's
/// `scene.name = "..."`. Read back as the node's name rather than as a
/// property, so it does not also appear in the property map.
const NAME_PROPERTY: &str = "name";

/// A GDScript registered as a scene post-import hook.
///
/// Register it like any other hook:
///
/// ```ignore
/// registry.register_post_import(Box::new(
///     ScriptScenePostImport::from_source("strip_cameras", source),
/// ));
/// ```
#[derive(Debug, Clone)]
pub struct ScriptScenePostImport {
    name: String,
    source: String,
}

impl ScriptScenePostImport {
    /// Builds a hook from script source text.
    pub fn from_source(name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: source.into(),
        }
    }

    /// Loads a hook from a script on disk, named after its path.
    ///
    /// `None` when the file cannot be read. A script that does not parse is
    /// *not* rejected here — that surfaces at import time, where there is an
    /// `EngineResult` to carry the reason, rather than being flattened into a
    /// missing-file `None`.
    pub fn from_script(script_path: &str) -> Option<Self> {
        let source = gdcore::file_access::FileAccess::read_file_string(script_path).ok()?;
        Some(Self::from_source(script_path, source))
    }

    /// The script source this hook runs.
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Materializes `described` and its subtree under `parent`, returning the new
/// node's id.
fn materialize(
    tree: &mut SceneTree,
    parent: NodeId,
    described: &ImportedSceneNode,
) -> EngineResult<NodeId> {
    let mut node = Node::new(described.name.clone(), described.node_type.clone());
    for (key, value) in &described.properties {
        node.set_property(key, value.clone());
    }
    let id = tree.add_child(parent, node)?;
    for child in &described.children {
        materialize(tree, id, child)?;
    }
    Ok(id)
}

/// Reads a materialized subtree back out, carrying whatever the script changed.
///
/// A `name` property written by the script becomes the node's name (Godot's
/// `scene.name = "..."`) and is dropped from the property map, so renaming does
/// not leave a stray property behind.
fn read_back(tree: &SceneTree, id: NodeId) -> Option<ImportedSceneNode> {
    let node = tree.get_node(id)?;

    let mut properties = std::collections::HashMap::new();
    for key in node.get_property_list() {
        properties.insert(key.to_string(), node.get_property(key));
    }
    let renamed = match properties.remove(NAME_PROPERTY) {
        Some(Variant::String(s)) => Some(s),
        Some(Variant::StringName(s)) => Some(s.as_str().to_string()),
        // A non-string `name` is not a rename; keep it as an ordinary property
        // rather than silently discarding what the script wrote.
        Some(other) => {
            properties.insert(NAME_PROPERTY.to_string(), other);
            None
        }
        None => None,
    };

    let mut out = ImportedSceneNode::new(
        renamed.unwrap_or_else(|| node.name().to_string()),
        node.class_name(),
    );
    out.properties = properties;

    let children = node.children().to_vec();
    for child in children {
        if let Some(described) = read_back(tree, child) {
            out.children.push(described);
        }
    }
    Some(out)
}

impl EditorScenePostImport for ScriptScenePostImport {
    fn name(&self) -> &str {
        &self.name
    }

    fn post_import(&self, scene: ImportedScene) -> EngineResult<ImportedScene> {
        let mut tree = SceneTree::new();
        let host = tree.root_id();
        let scene_root = materialize(&mut tree, host, &scene.root)?;

        // The script instance needs a node to live on; it is the hook itself,
        // not part of the imported scene, so it is a sibling of the scene root
        // and is torn down before anything is read back.
        let carrier = tree
            .add_child(host, Node::new(self.name.as_str(), "EditorScenePostImport"))
            .map_err(|e| {
                EngineError::InvalidOperation(format!(
                    "post-import script '{}' could not be hosted: {e}",
                    self.name
                ))
            })?;
        let instance = GDScriptNodeInstance::from_source(&self.source, carrier).map_err(|e| {
            EngineError::Parse(format!("post-import script '{}' failed: {e}", self.name))
        })?;
        tree.attach_script(carrier, Box::new(instance));

        let returned = tree.call_script_method(
            carrier,
            POST_IMPORT,
            &[Variant::ObjectId(scene_root.object_id())],
        );

        let root_to_read = match returned {
            // Godot only runs the virtual when the script overrides it, and a
            // script that does not is not an error — the scene passes through.
            None => {
                let _ = tree.remove_node(carrier);
                return Ok(scene);
            }
            // A script may hand back a different node than it was given.
            Some(Variant::ObjectId(id)) => NodeId::from_object_id(id),
            // ⚠️ Godot FAILS the import when `_post_import` returns null, which
            // is what a GDScript with no `return` marshals to. Treating it as
            // "no changes" would silently drop every mutation the script made,
            // so it is an error here too, named so the cause is obvious.
            Some(Variant::Nil) => {
                return Err(EngineError::InvalidOperation(format!(
                    "post-import script '{}' returned null from {POST_IMPORT}; \
                     it must return the scene",
                    self.name
                )));
            }
            Some(other) => {
                return Err(EngineError::TypeError {
                    expected: "Node".to_string(),
                    actual: format!("{other:?}"),
                });
            }
        };

        let _ = tree.remove_node(carrier);
        let root = read_back(&tree, root_to_read).ok_or_else(|| {
            EngineError::InvalidOperation(format!(
                "post-import script '{}' returned a node that is not in the scene",
                self.name
            ))
        })?;

        Ok(ImportedScene { root, ..scene })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::SceneFormatImporterRegistry;

    fn scene_with_child() -> ImportedScene {
        let child = ImportedSceneNode::new("Camera3D", "Camera3D")
            .with_property("fov", Variant::Float(70.0));
        let root = ImportedSceneNode::new("Scene", "Node3D").with_child(child);
        ImportedScene {
            root,
            animations: Vec::new(),
            mesh_count: 0,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn a_script_property_write_survives_the_round_trip() {
        let hook = ScriptScenePostImport::from_source(
            "tagger",
            "extends EditorScenePostImport\nfunc _post_import(scene):\n\tscene.imported_by = \"tagger\"\n\treturn scene\n",
        );
        let out = hook
            .post_import(scene_with_child())
            .expect("post-import ran");
        assert_eq!(
            out.root.properties.get("imported_by"),
            Some(&Variant::String("tagger".to_string())),
            "what the script wrote through the ObjectId must come back out"
        );
    }

    /// ⚠️ The control that proves the script is handed a real TREE and not just
    /// a bag: it has to walk to a child it never saw in source.
    #[test]
    fn the_script_receives_the_whole_tree_not_just_the_root() {
        let hook = ScriptScenePostImport::from_source(
            "counter",
            "extends EditorScenePostImport\nfunc _post_import(scene):\n\tscene.child_total = scene.get_child_count()\n\treturn scene\n",
        );
        let out = hook
            .post_import(scene_with_child())
            .expect("post-import ran");
        assert_eq!(
            out.root.properties.get("child_total"),
            Some(&Variant::Int(1)),
            "the script must see the materialized child"
        );
        assert_eq!(
            out.root.children.len(),
            1,
            "the child must survive readback"
        );
        assert_eq!(out.root.children[0].name, "Camera3D");
    }

    /// Godot's `scene.name = "..."` renames the node; it must not leave a stray
    /// `name` property behind either.
    #[test]
    fn writing_the_name_property_renames_the_node() {
        let hook = ScriptScenePostImport::from_source(
            "renamer",
            "extends EditorScenePostImport\nfunc _post_import(scene):\n\tscene.name = \"Renamed\"\n\treturn scene\n",
        );
        let out = hook
            .post_import(scene_with_child())
            .expect("post-import ran");
        assert_eq!(out.root.name, "Renamed");
        assert!(
            !out.root.properties.contains_key("name"),
            "a rename must not also appear as a property"
        );
    }

    /// ⚠️ Not overriding the virtual is NOT an error in Godot — most hooks are
    /// optional. The scene must pass through untouched, which is also the only
    /// case where `call_script_method` returns `None`.
    #[test]
    fn a_script_without_the_virtual_passes_the_scene_through() {
        let hook = ScriptScenePostImport::from_source("inert", "func unrelated():\n\treturn 1\n");
        let out = hook.post_import(scene_with_child()).expect("pass-through");
        assert_eq!(out.root.name, "Scene");
        assert_eq!(out.root.children.len(), 1);
        assert_eq!(
            out.root.children[0].properties.get("fov"),
            Some(&Variant::Float(70.0)),
            "an untouched scene must keep its imported properties"
        );
    }

    /// ⚠️ The case a forgetful script hits: no `return` marshals to null, and
    /// Godot fails the import rather than silently dropping the mutations.
    #[test]
    fn returning_null_is_an_error_rather_than_a_silent_no_op() {
        let hook = ScriptScenePostImport::from_source(
            "forgetful",
            "extends EditorScenePostImport\nfunc _post_import(scene):\n\tscene.touched = true\n",
        );
        let err = hook
            .post_import(scene_with_child())
            .expect_err("a null return must fail the import");
        let text = err.to_string();
        assert!(
            text.contains("forgetful") && text.contains("null"),
            "the error must name the script and the cause, got: {text}"
        );
    }

    #[test]
    fn a_broken_script_reports_a_parse_error_not_a_pass_through() {
        let hook = ScriptScenePostImport::from_source("broken", "func _post_import(scene\n");
        let err = hook
            .post_import(scene_with_child())
            .expect_err("an unparseable script must not be ignored");
        assert!(
            err.to_string().contains("broken"),
            "the error must name the offending script, got: {err}"
        );
    }

    #[test]
    fn it_registers_as_an_ordinary_post_import_hook() {
        let mut registry = SceneFormatImporterRegistry::new();
        registry.register_post_import(Box::new(ScriptScenePostImport::from_source(
            "tagger",
            "extends EditorScenePostImport\nfunc _post_import(scene):\n\treturn scene\n",
        )));
        assert_eq!(registry.post_import_count(), 1);
        assert!(
            registry.unregister_post_import("tagger"),
            "it must be removable by the name it registered under"
        );
        assert_eq!(registry.post_import_count(), 0);
    }

    /// The carrier node the script lives on is scaffolding, not scene content.
    #[test]
    fn the_script_carrier_node_does_not_leak_into_the_scene() {
        let hook = ScriptScenePostImport::from_source(
            "tagger",
            "extends EditorScenePostImport\nfunc _post_import(scene):\n\treturn scene\n",
        );
        let out = hook
            .post_import(scene_with_child())
            .expect("post-import ran");
        assert_eq!(
            out.root.children.len(),
            1,
            "only the imported child may remain: {:?}",
            out.root
                .children
                .iter()
                .map(|c| &c.name)
                .collect::<Vec<_>>()
        );
    }

    /// Metadata the importer gathered is not the script's business and must
    /// survive a hook that only touches the node tree.
    #[test]
    fn non_node_import_metadata_is_preserved() {
        let mut scene = scene_with_child();
        scene.mesh_count = 7;
        scene.warnings.push("a warning".to_string());
        let hook = ScriptScenePostImport::from_source(
            "tagger",
            "extends EditorScenePostImport\nfunc _post_import(scene):\n\tscene.imported_by = \"tagger\"\n\treturn scene\n",
        );
        let out = hook.post_import(scene).expect("post-import ran");
        assert_eq!(out.mesh_count, 7);
        assert_eq!(out.warnings, vec!["a warning".to_string()]);
    }
}
