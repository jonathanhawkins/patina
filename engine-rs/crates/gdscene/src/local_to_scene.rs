//! `resource_local_to_scene` — per-instance resource copies and the
//! `_setup_local_to_scene` callback (pat-s3rrv.157).
//!
//! # What this is for
//!
//! A Godot resource with `resource_local_to_scene = true` is **duplicated for
//! every scene instance** rather than shared, and the engine calls
//! `_setup_local_to_scene()` on each copy immediately afterwards so it can bind
//! to its owning scene. It is how a ViewportTexture finds its SubViewport and
//! how per-instance ShaderMaterials get unique parameter state.
//!
//! # Why a registry rather than a copy
//!
//! ⚠️ Patina has **no shared resource object** in scene instancing to begin
//! with: `packed_scene` never constructs a `Variant::Resource`, an inline
//! resource is flattened onto its node as companion properties, and
//! `ResourceRef` is a value type carrying no id. So each instance already holds
//! independent data, and "duplicate it" is not the missing piece — *identity*
//! is. Without an id there is nothing for `get_instance_id()` to return, and
//! the two copies a script stamps are indistinguishable.
//!
//! This mints an `ObjectId` per local-to-scene copy, the same tagged-handle
//! shape `gdobject::weak_ref` and `gdobject::refcounted_handle` use, so a copy
//! can answer for itself.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use gdcore::id::ObjectId;
use gdvariant::Variant;

/// Tags an `ObjectId` minted by [`new_local_resource`].
///
/// 🔑 A handle tag byte is a **workspace-wide** namespace — every
/// `is_*_handle` predicate tests these same 8 bits and the first match wins the
/// dispatch, so a duplicate does not fail loudly, it routes this family's calls
/// into another one's handler. `0x78` was chosen by sweeping every tag constant
/// under `crates/*/src`; `0x77` is weakref, `0x71` and below are taken.
pub const LOCAL_RESOURCE_HANDLE_TAG: u64 = 0x7800_0000_0000_0000;

/// Mask selecting the tag byte out of a raw id.
const TAG_MASK: u64 = 0xFF00_0000_0000_0000;

/// One local-to-scene resource copy.
#[derive(Debug, Clone, Default)]
pub struct LocalResource {
    /// The resource's class (`ShaderMaterial`, or a script's own class).
    pub class_name: String,
    /// Path of the script attached to the resource, if any. This is what
    /// carries `_setup_local_to_scene`.
    pub script_path: Option<String>,
    /// The resource's own properties, including anything its callback writes.
    pub properties: HashMap<String, Variant>,
}

fn registry() -> &'static Mutex<HashMap<u64, LocalResource>> {
    static REGISTRY: OnceLock<Mutex<HashMap<u64, LocalResource>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_seq() -> u64 {
    static NEXT: OnceLock<Mutex<u64>> = OnceLock::new();
    let cell = NEXT.get_or_init(|| Mutex::new(1));
    let mut n = cell.lock().unwrap();
    let v = *n;
    *n += 1;
    v
}

/// Whether `id` names a local-to-scene resource copy.
pub fn is_local_resource_handle(id: ObjectId) -> bool {
    id.raw() & TAG_MASK == LOCAL_RESOURCE_HANDLE_TAG
}

/// Mints a fresh copy and returns its id.
///
/// Every call yields a DISTINCT id even for identical input, which is the whole
/// point: two instances of the same sub-scene must end up with resources that
/// can be told apart, and a content-derived id would make them equal again.
pub fn new_local_resource(resource: LocalResource) -> ObjectId {
    let id = ObjectId::from_raw(LOCAL_RESOURCE_HANDLE_TAG | next_seq());
    registry().lock().unwrap().insert(id.raw(), resource);
    id
}

/// Reads a copy back.
pub fn local_resource(id: ObjectId) -> Option<LocalResource> {
    registry().lock().unwrap().get(&id.raw()).cloned()
}

/// A single property of a copy — what a test or the inspector reads to observe
/// what `_setup_local_to_scene` wrote.
pub fn local_resource_property(id: ObjectId, name: &str) -> Variant {
    registry()
        .lock()
        .unwrap()
        .get(&id.raw())
        .and_then(|r| r.properties.get(name).cloned())
        .unwrap_or(Variant::Nil)
}

/// Writes a property onto a copy — used to fold a callback's results back in.
pub fn set_local_resource_property(id: ObjectId, name: &str, value: Variant) -> bool {
    match registry().lock().unwrap().get_mut(&id.raw()) {
        Some(r) => {
            r.properties.insert(name.to_string(), value);
            true
        }
        None => false,
    }
}

/// Whether a property bag asks to be local to its scene.
///
/// ⚠️ Godot omits a property at its default, so an ABSENT flag means `false`
/// and the resource is shared — reading absence as "true" would duplicate every
/// resource in every scene and give each instance its own silently divergent
/// copy of data the author meant to share.
pub fn wants_local_to_scene(properties: &HashMap<String, Variant>) -> bool {
    matches!(
        properties.get("resource_local_to_scene"),
        Some(Variant::Bool(true))
    )
}

/// Duplicates every local-to-scene resource in `tree` and runs its
/// `_setup_local_to_scene` callback. Returns how many copies were made.
///
/// # Why this is a pass over the tree, not a step inside instancing
///
/// The copy has to be **per instance**, and `packed_scene` builds nodes from
/// shared templates at eleven separate call sites — minting inside the template
/// pass would produce ONE id shared by every instance, which is precisely the
/// bug this feature exists to prevent. Walking the finished tree gives one copy
/// per node for free, and needs no change to any of those sites.
///
/// The sub-resource bodies are not on the node (only a `__resref__<prop>`
/// companion naming `class::id`), so the originating [`PackedScene`] is needed
/// to resolve the id back to the resource's own properties.
pub fn setup_local_to_scene(
    tree: &mut crate::scene_tree::SceneTree,
    scene: &crate::packed_scene::PackedScene,
) -> usize {
    let root = tree.root_id();
    setup_local_to_scene_in(tree, scene, root)
}

/// The same pass, restricted to `subtree_root` and its descendants
/// (pat-mgixk.71).
///
/// 🔑 **Scoping is a correctness requirement, not an optimisation.** A
/// `__resref__` companion names its sub-resource by a **scene-local** id
/// (`SubResource("1")`), so resolving one instance's refs against a DIFFERENT
/// scene's `sub_resource_props` can silently match an unrelated resource. Each
/// instancing therefore gets a pass bounded to the subtree it just built, with
/// the `PackedScene` those ids actually belong to.
///
/// Also idempotent: a property that already carries its `__local_res__` copy is
/// skipped, so a node reached twice (two entry points firing for one load, or a
/// re-entrant instancing) keeps its first copy instead of minting a second and
/// orphaning the id the callback already stamped.
pub fn setup_local_to_scene_in(
    tree: &mut crate::scene_tree::SceneTree,
    scene: &crate::packed_scene::PackedScene,
    subtree_root: crate::node::NodeId,
) -> usize {
    // `SceneTree::collect_subtree_top_down` is the engine's own subtree walk —
    // `all_nodes_in_tree_order` covers the WHOLE tree, which is the one thing
    // this pass must not do.
    let mut ids = Vec::new();
    tree.collect_subtree_top_down(subtree_root, &mut ids);
    setup_local_to_scene_for(tree, scene, &ids)
}

/// [`setup_local_to_scene_in`] over an explicit node list — the nodes ONE scene
/// level created. A scene with instanced sub-scenes holds nodes whose
/// `__resref__` ids belong to DIFFERENT scenes, so even the subtree is too wide
/// a scope there; each level passes the nodes it built with its own table.
pub fn setup_local_to_scene_for(
    tree: &mut crate::scene_tree::SceneTree,
    scene: &crate::packed_scene::PackedScene,
    ids: &[crate::node::NodeId],
) -> usize {
    let mut made = 0usize;
    for &node_id in ids {
        // Collect first, mutate second: the walk borrows the node immutably and
        // the write below needs it mutably.
        let refs: Vec<(String, String, String)> = match tree.get_node(node_id) {
            Some(node) => node
                .properties()
                .filter_map(|(key, value)| {
                    let prop = key.strip_prefix("__resref__")?;
                    let Variant::String(meta) = value else {
                        return None;
                    };
                    // `__resref__<prop>` holds `"<class>::<sub id>"`.
                    let (class, sub_id) = meta.rsplit_once("::")?;
                    Some((prop.to_string(), class.to_string(), sub_id.to_string()))
                })
                .collect(),
            None => continue,
        };

        for (prop, class_name, sub_id) in refs {
            let Some(props) = scene.sub_resource_props(&sub_id) else {
                continue;
            };
            if !wants_local_to_scene(props) {
                continue;
            }
            // Already has its copy — see the idempotence note on
            // `setup_local_to_scene_in`.
            if tree
                .get_node(node_id)
                .is_some_and(|n| n.has_property(&format!("__local_res__{prop}")))
            {
                continue;
            }
            // `script = ExtResource("id")` survives parsing as the string
            // `"ExtResource:id"`, which the scene resolves back to a res:// path.
            let script_path = match props.get("script") {
                Some(Variant::String(s)) => scene.resolve_ext_resource_path(s).map(str::to_string),
                _ => None,
            };
            let id = new_local_resource(LocalResource {
                class_name,
                script_path: script_path.clone(),
                properties: props.clone(),
            });
            // Minted BEFORE the callback runs, so `get_instance_id()` inside it
            // has an id to report and each copy reports a different one.
            if let Some(path) = script_path {
                run_setup_callback(id, &path);
            }
            if let Some(node) = tree.get_node_mut(node_id) {
                node.set_property(&format!("__local_res__{prop}"), Variant::ObjectId(id));
            }
            made += 1;
        }
    }
    made
}

/// Runs one resource's `_setup_local_to_scene` and folds what it wrote back
/// into the registry.
///
/// Silent on every failure path: a resource whose script is missing, fails to
/// parse, or simply does not define the callback is not an error — Godot only
/// calls the virtual on resources that override it, and the copy is still a
/// valid copy without it.
fn run_setup_callback(id: ObjectId, script_path: &str) {
    let Ok(source) = gdcore::file_access::FileAccess::read_file_string(script_path) else {
        return;
    };
    let mut interp = gdscript_interop::interpreter::Interpreter::new();
    let Ok(class_def) = interp.run_class(&source) else {
        return;
    };
    let Ok(mut instance) = interp.instantiate_class(&class_def) else {
        return;
    };
    // Seed the script's vars from the resource so the callback can read the
    // values the .tres declared, not just write new ones.
    if let Some(existing) = local_resource(id) {
        for (key, value) in existing.properties {
            instance.properties.entry(key).or_insert(value);
        }
    }
    // Give the instance its own identity, which is what makes a bare
    // `get_instance_id()` inside `_setup_local_to_scene` answer — and answer
    // DIFFERENTLY for each copy, which is the whole observable of this feature.
    instance.properties.insert(
        gdscript_interop::interpreter::SELF_OBJECT_ID_KEY.to_string(),
        Variant::Int(id.raw() as i64),
    );
    if interp
        .call_instance_method(&mut instance, "_setup_local_to_scene", &[])
        .is_err()
    {
        return;
    }
    for (key, value) in instance.properties {
        // The seeded id is interpreter plumbing, not one of the resource's own
        // properties; folding it back would expose it to the inspector and to
        // anything that saves the resource.
        if key == gdscript_interop::interpreter::SELF_OBJECT_ID_KEY {
            continue;
        }
        set_local_resource_property(id, &key, value);
    }
}

/// Clears the registry. Test isolation only — this is process-global.
pub fn clear_for_testing() {
    registry().lock().unwrap().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bag(pairs: &[(&str, Variant)]) -> HashMap<String, Variant> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    /// The flag is off unless explicitly on: Godot omits it at its default.
    #[test]
    fn an_absent_flag_is_not_local_to_scene() {
        assert!(!wants_local_to_scene(&bag(&[])));
        assert!(!wants_local_to_scene(&bag(&[(
            "resource_local_to_scene",
            Variant::Bool(false)
        )])));
        assert!(wants_local_to_scene(&bag(&[(
            "resource_local_to_scene",
            Variant::Bool(true)
        )])));
    }

    /// Two copies of identical input must be distinguishable — this is the
    /// property the whole feature rests on.
    #[test]
    fn every_copy_gets_its_own_id() {
        clear_for_testing();
        let mk = || LocalResource {
            class_name: "ShaderMaterial".to_string(),
            script_path: None,
            properties: bag(&[("shared", Variant::Int(1))]),
        };
        let a = new_local_resource(mk());
        let b = new_local_resource(mk());

        assert_ne!(a, b, "identical input must still yield distinct copies");
        assert!(is_local_resource_handle(a) && is_local_resource_handle(b));
        // ...and they must not alias: writing one leaves the other alone.
        assert!(set_local_resource_property(a, "stamp", Variant::Int(7)));
        assert_eq!(local_resource_property(a, "stamp"), Variant::Int(7));
        assert_eq!(
            local_resource_property(b, "stamp"),
            Variant::Nil,
            "the copies share storage — a write to one reached the other"
        );
    }

    /// The tag must not collide with the other handle families.
    #[test]
    fn the_tag_is_distinct_from_neighbouring_families() {
        clear_for_testing();
        let id = new_local_resource(LocalResource::default());
        assert!(is_local_resource_handle(id));
        assert!(
            !gdobject::weak_ref::is_weakref_handle(id),
            "a local-resource id must not answer the weakref predicate"
        );
        assert!(
            !gdobject::refcounted_handle::is_refcounted_handle(id),
            "a local-resource id must not answer the refcounted predicate"
        );
    }

    #[test]
    fn an_unknown_id_reads_nil_rather_than_panicking() {
        clear_for_testing();
        let stale = ObjectId::from_raw(LOCAL_RESOURCE_HANDLE_TAG | 999);
        assert_eq!(local_resource_property(stale, "stamp"), Variant::Nil);
        assert!(local_resource(stale).is_none());
        assert!(!set_local_resource_property(
            stale,
            "stamp",
            Variant::Int(1)
        ));
    }
}
