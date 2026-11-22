//! Shared test helpers. The skinned test model is generated here (self-made, CC0-1.0 / project-owned), so no binary model
//! has to be committed.

use std::path::Path;

use serde_json::{Value, json};

/// Bind-pose joint positions (model space) of the test rig; the order is the order of the skin's joints.
/// root, foot.L, spine, head, hand.R
const JOINTS: [(&str, [f32; 3]); 5] = [
    ("root", [0.0, 0.0, 0.0]),
    ("foot.L", [0.2, 0.0, 0.0]),
    ("spine", [0.0, 1.0, 0.0]),
    ("head", [0.0, 2.1, 0.0]),
    ("hand.R", [-0.4, 1.8, 0.0]),
];
/// Parent of each joint (index into JOINTS).
const PARENT: [Option<usize>; 5] = [None, Some(0), Some(0), Some(2), Some(2)];

#[derive(Default)]
struct Builder {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Builder {
    fn add(&mut self, bytes: &[u8], component: u32, kind: &str, count: usize, bounds: Option<(Vec<f32>, Vec<f32>)>) -> usize {
        while self.bin.len() % 4 != 0 {
            self.bin.push(0);
        }
        self.views.push(json!({ "buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len() }));
        self.bin.extend_from_slice(bytes);
        let mut a = json!({ "bufferView": self.views.len() - 1, "componentType": component, "count": count, "type": kind });
        if let Some((min, max)) = bounds {
            a["min"] = json!(min);
            a["max"] = json!(max);
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }

    fn floats(&mut self, v: &[f32], kind: &str, count: usize, bounds: Option<(Vec<f32>, Vec<f32>)>) -> usize {
        let bytes: Vec<u8> = v.iter().flat_map(|f| f.to_le_bytes()).collect();
        self.add(&bytes, 5126, kind, count, bounds)
    }
}

/// Axis-aligned box as 24 vertices (flat normals), skinned 100% to `joint`.
fn push_box(min: [f32; 3], max: [f32; 3], joint: u16, pos: &mut Vec<f32>, nrm: &mut Vec<f32>, jnt: &mut Vec<u16>, idx: &mut Vec<u16>) {
    // (normal, four corners counter-clockwise seen from outside)
    let (x0, y0, z0, x1, y1, z1) = (min[0], min[1], min[2], max[0], max[1], max[2]);
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        ([0.0, 0.0, 1.0], [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]]),
        ([0.0, 0.0, -1.0], [[x1, y0, z0], [x0, y0, z0], [x0, y1, z0], [x1, y1, z0]]),
        ([1.0, 0.0, 0.0], [[x1, y0, z1], [x1, y0, z0], [x1, y1, z0], [x1, y1, z1]]),
        ([-1.0, 0.0, 0.0], [[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]]),
        ([0.0, 1.0, 0.0], [[x0, y1, z1], [x1, y1, z1], [x1, y1, z0], [x0, y1, z0]]),
        ([0.0, -1.0, 0.0], [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]]),
    ];
    for (n, corners) in faces {
        let base = (pos.len() / 3) as u16;
        for c in corners {
            pos.extend_from_slice(&c);
            nrm.extend_from_slice(&n);
            jnt.extend_from_slice(&[joint, 0, 0, 0]);
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn quat_x(angle: f32) -> [f32; 4] {
    [(angle / 2.0).sin(), 0.0, 0.0, (angle / 2.0).cos()]
}

/// Writes a small skinned humanoid as a binary glTF (GLB): one skinned mesh node `body` (torso, head, right arm, left
/// foot as boxes), five joints (`root`, `foot.L`, `spine`, `head`, `hand.R`), inverse bind matrices and three clips:
/// `idle` (2 s, the spine bobs), `walk` (1.2 s, the left foot swings), `reach` (1 s, the right hand moves 1 unit forward, -Z).
/// The model is about 2.4 units tall; the head joint sits at y = 2.1.
pub fn write_test_rig(path: &Path) {
    let mut b = Builder::default();
    let (mut pos, mut nrm, mut jnt, mut idx) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    push_box([-0.3, 0.9, -0.15], [0.3, 1.9, 0.15], 2, &mut pos, &mut nrm, &mut jnt, &mut idx); // torso -> spine
    push_box([-0.2, 1.9, -0.2], [0.2, 2.4, 0.2], 3, &mut pos, &mut nrm, &mut jnt, &mut idx); // head
    push_box([-0.7, 1.6, -0.1], [-0.3, 1.9, 0.1], 4, &mut pos, &mut nrm, &mut jnt, &mut idx); // right arm -> hand.R
    push_box([0.1, 0.0, -0.1], [0.3, 0.9, 0.1], 1, &mut pos, &mut nrm, &mut jnt, &mut idx); // left leg -> foot.L
    let count = pos.len() / 3;
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in pos.chunks_exact(3) {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let a_pos = b.floats(&pos, "VEC3", count, Some((lo.to_vec(), hi.to_vec())));
    let a_nrm = b.floats(&nrm, "VEC3", count, None);
    let jbytes: Vec<u8> = jnt.iter().flat_map(|j| j.to_le_bytes()).collect();
    let a_jnt = b.add(&jbytes, 5123, "VEC4", count, None);
    let weights: Vec<f32> = (0..count).flat_map(|_| [1.0, 0.0, 0.0, 0.0]).collect();
    let a_wgt = b.floats(&weights, "VEC4", count, None);
    let ibytes: Vec<u8> = idx.iter().flat_map(|i| i.to_le_bytes()).collect();
    let a_idx = b.add(&ibytes, 5123, "SCALAR", idx.len(), None);
    let ibm: Vec<f32> = JOINTS
        .iter()
        .flat_map(|(_, p)| {
            // inverse of a translation, column-major
            [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -p[0], -p[1], -p[2], 1.0]
        })
        .collect();
    let a_ibm = b.floats(&ibm, "MAT4", JOINTS.len(), None);

    // Nodes 0..5 are the joints, node 5 is the skinned mesh holder.
    let mut nodes: Vec<Value> = Vec::new();
    for (i, (name, p)) in JOINTS.iter().enumerate() {
        let local = match PARENT[i] {
            Some(par) => [p[0] - JOINTS[par].1[0], p[1] - JOINTS[par].1[1], p[2] - JOINTS[par].1[2]],
            None => *p,
        };
        let children: Vec<usize> = (0..JOINTS.len()).filter(|&c| PARENT[c] == Some(i)).collect();
        let mut n = json!({ "name": name, "translation": local });
        if !children.is_empty() {
            n["children"] = json!(children);
        }
        nodes.push(n);
    }
    nodes.push(json!({ "name": "body", "mesh": 0, "skin": 0 }));

    // Animations: (name, [(node, path, times, values)]).
    type Chan = (usize, &'static str, Vec<f32>, Vec<f32>);
    let clips: Vec<(&str, Vec<Chan>)> = vec![
        ("idle", vec![(2, "translation", vec![0.0, 1.0, 2.0], vec![0.0, 1.0, 0.0, 0.0, 1.02, 0.0, 0.0, 1.0, 0.0])]),
        (
            "walk",
            vec![
                (1, "rotation", vec![0.0, 0.3, 0.6, 0.9, 1.2], [0.0, 0.5, 0.0, -0.5, 0.0].iter().flat_map(|a| quat_x(*a)).collect()),
                (2, "translation", vec![0.0, 0.6, 1.2], vec![0.0, 1.0, 0.0, 0.0, 1.05, 0.0, 0.0, 1.0, 0.0]),
            ],
        ),
        ("reach", vec![(4, "translation", vec![0.0, 1.0], vec![-0.4, 0.8, 0.0, -0.4, 0.8, -1.0])]),
    ];
    let mut animations = Vec::new();
    for (name, chans) in &clips {
        // Sampler / channel indices are local to the animation.
        let (mut samplers, mut channels) = (Vec::new(), Vec::new());
        for (node, path, times, values) in chans {
            let kind = if *path == "rotation" { "VEC4" } else { "VEC3" };
            let n = times.len();
            let input = b.floats(times, "SCALAR", n, Some((vec![times[0]], vec![*times.last().unwrap()])));
            let output = b.floats(values, kind, n, None);
            channels.push(json!({ "sampler": samplers.len(), "target": { "node": node, "path": path } }));
            samplers.push(json!({ "input": input, "output": output, "interpolation": "LINEAR" }));
        }
        animations.push(json!({ "name": name, "samplers": samplers, "channels": channels }));
    }
    let doc = json!({
        "asset": { "version": "2.0", "generator": "Spark test rig (self-made, CC0-1.0)" },
        "scene": 0,
        "scenes": [{ "nodes": [0, 5] }],
        "nodes": nodes,
        "meshes": [{ "name": "body", "primitives": [{
            "attributes": { "POSITION": a_pos, "NORMAL": a_nrm, "JOINTS_0": a_jnt, "WEIGHTS_0": a_wgt },
            "indices": a_idx,
            "material": 0,
        }] }],
        "materials": [{ "pbrMetallicRoughness": { "baseColorFactor": [0.8, 0.8, 0.8, 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.8 } }],
        "skins": [{ "joints": [0, 1, 2, 3, 4], "inverseBindMatrices": a_ibm, "skeleton": 0 }],
        "animations": animations,
        "buffers": [{ "byteLength": b.bin.len() }],
        "bufferViews": b.views,
        "accessors": b.accessors,
    });

    let mut json_bytes = serde_json::to_vec(&doc).unwrap();
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    let mut bin = b.bin;
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(path, out).unwrap();
}
