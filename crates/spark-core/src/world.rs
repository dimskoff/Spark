use crate::animation::Animator;
use crate::assets::{Assets, MeshId};
use crate::audio::Audio;
use crate::branding::Branding;
use crate::canvas::Canvas;
use crate::input::Input;
use crate::material::Material;
use crate::mesh::MeshData;
use crate::physics::Physics;
use crate::render::RenderSettings;
use crate::scene::{Object, ObjectId, Scene};
use crate::time::Time;

/// Window state a game can change at runtime. The app applies changes every frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowSettings {
    /// Borderless fullscreen on the current monitor (F11 / Alt+Enter toggle it too).
    pub fullscreen: bool,
    /// Window title (`None` = the app's title).
    pub title: Option<String>,
}

/// An object script that failed (ADR 0005, section 4). The game keeps running; the script host reports it here so
/// that devtools and tools can show it next to the node.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptFailure {
    /// Node id of the object (`None` when it was already destroyed).
    pub node_id: Option<crate::node_id::NodeId>,
    /// Name of the object (empty = unnamed).
    pub object: String,
    /// Stable error code (`SPK-1000`, ...).
    pub code: String,
    /// Full message: script path, callback, object and the error.
    pub message: String,
}

/// Failures kept for the next [`World::take_script_failures`]; more are dropped (the host logs every one).
const MAX_PENDING_FAILURES: usize = 256;

/// Everything the game can touch. Pure data: renderer, window and scripts read/write it.
#[derive(Debug, Default)]
pub struct World {
    pub scene: Scene,
    pub assets: Assets,
    pub input: Input,
    pub time: Time,
    /// Resolution, texture sampling, custom shaders and post passes.
    pub render: RenderSettings,
    pub physics: Physics,
    pub audio: Audio,
    /// Immediate-mode 2D drawing, cleared every frame.
    pub canvas: Canvas,
    /// Spark splash / badge state (see [`crate::branding`]).
    pub branding: Branding,
    /// Spawned glTF models and their animations (see [`crate::animation`]).
    pub animator: Animator,
    /// Fullscreen / title requests for the window (ignored in headless runs).
    pub window: WindowSettings,
    /// Debug overlay (FPS, GPU, scene stats). Toggle with F1 in the runner.
    pub debug: crate::debug::DebugOverlay,
    /// Immediate-mode UI state (buttons, sliders, panels).
    pub ui: crate::ui::Ui,
    /// `NodeId -> ObjectId` table (see [`crate::node_id`]).
    pub(crate) nodes: crate::node_id::NodeIndex,
    /// Live prefab instances (see [`crate::doc::PrefabInstance`]).
    pub(crate) prefabs: crate::doc::PrefabTable,
    script_failures: Vec<ScriptFailure>,
    quit: bool,
    restart: bool,
}

impl World {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an object with a mesh and material. Returns its handle.
    pub fn spawn(&mut self, mesh: MeshId, material: Material) -> ObjectId {
        let mut object = Object::empty();
        object.mesh = Some(mesh);
        object.material = material;
        object.local_bounds = self.assets.mesh_bounds(mesh);
        self.scene.add(object)
    }

    /// Adds an empty object (group / pivot) to parent other objects to.
    pub fn spawn_empty(&mut self) -> ObjectId {
        self.scene.add(Object::empty())
    }

    /// Shortcut for `assets.add_mesh`.
    pub fn add_mesh(&mut self, mesh: MeshData) -> MeshId {
        self.assets.add_mesh(mesh)
    }

    /// Changes an object's mesh (updates bounds).
    pub fn set_mesh(&mut self, id: ObjectId, mesh: MeshId) {
        let bounds = self.assets.mesh_bounds(mesh);
        if let Some(o) = self.scene.get_mut(id) {
            o.mesh = Some(mesh);
            o.local_bounds = bounds;
        }
    }

    /// Records a failed object script (called by the script host).
    pub fn report_script_failure(&mut self, failure: ScriptFailure) {
        if self.script_failures.len() < MAX_PENDING_FAILURES {
            self.script_failures.push(failure);
        }
    }

    /// Object script failures since the last call (devtools sends them to the editor).
    pub fn take_script_failures(&mut self) -> Vec<ScriptFailure> {
        std::mem::take(&mut self.script_failures)
    }

    /// Object script failures not taken yet.
    pub fn script_failures(&self) -> &[ScriptFailure] {
        &self.script_failures
    }

    /// Ask the app to close after this frame.
    pub fn quit(&mut self) {
        self.quit = true;
    }

    pub fn quit_requested(&self) -> bool {
        self.quit
    }

    /// Ask the script host to restart the game (scene cleared, `start()` runs again) at the next frame.
    pub fn request_restart(&mut self) {
        self.restart = true;
    }

    /// True once after `request_restart` (the script host consumes it).
    pub fn take_restart(&mut self) -> bool {
        std::mem::take(&mut self.restart)
    }

    /// Clears scene, assets, render settings, bodies and sounds (keeps input, time, backends and asset root).
    /// Used by hot reload.
    pub fn reset(&mut self) {
        self.scene = Scene::new();
        self.nodes.clear();
        self.prefabs.clear();
        self.script_failures.clear();
        self.physics.clear();
        self.physics.gravity = Physics::default().gravity;
        self.physics.enabled = true;
        self.audio.stop_all(0.0);
        self.audio.clear_cache();
        self.assets.clear();
        self.animator.clear();
        self.render = RenderSettings::default();
        self.canvas.reset();
        self.debug = crate::debug::DebugOverlay::default();
        self.ui = crate::ui::Ui::default();
    }
}
