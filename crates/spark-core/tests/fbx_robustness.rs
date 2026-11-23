//! Robustness of the model loaders (FBX binary / ASCII and glTF / GLB) against malformed input: they must return
//! `Err` (or `Ok`) and never panic, hang or allocate without bound.
//!
//! Everything is generated in memory. The mutation loops are deterministic (fixed-seed xorshift); a failure reports the
//! seed and iteration so it can be replayed. For coverage-guided fuzzing see `fuzz/README.md`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::time::{Duration, Instant};

use spark_core::assets::Assets;
use spark_core::model::ModelData;

// ----------------------------------------------------------------------------- tiny RNG + mutator

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const INTERESTING: [u32; 13] =
    [0, 1, 2, 3, 7, 0xFF, 0x100, 0xFFFF, 0x7FFF_FFFF, 0x8000_0000, 0xFFFF_FFFF, 64 * 1024 * 1024, 64 * 1024 * 1024 + 1];

fn put_u32(v: &mut [u8], at: usize, val: u32) {
    if let Some(s) = v.get_mut(at..at + 4) {
        s.copy_from_slice(&val.to_le_bytes());
    }
}

/// Random byte-level damage; `fields` are offsets of structural u32 values (counts, sizes) to attack on purpose.
fn mutate(rng: &mut Rng, base: &[u8], fields: &[usize]) -> Vec<u8> {
    let mut v = base.to_vec();
    for _ in 0..1 + rng.below(4) {
        if v.is_empty() {
            break;
        }
        match rng.below(8) {
            0 => {
                let i = rng.below(v.len());
                v[i] ^= 1 << rng.below(8);
            }
            1 => {
                let i = rng.below(v.len());
                v[i] = rng.next() as u8;
            }
            2 => {
                let i = rng.below(v.len() + 1);
                for _ in 0..1 + rng.below(8) {
                    v.insert(i, rng.next() as u8);
                }
            }
            3 => {
                let i = rng.below(v.len());
                let n = (1 + rng.below(16)).min(v.len() - i);
                v.drain(i..i + n);
            }
            4 => {
                let n = rng.below(v.len());
                v.truncate(n);
            }
            5 | 6 if !fields.is_empty() => {
                let at = fields[rng.below(fields.len())];
                let val = if rng.below(4) == 0 { rng.next() as u32 } else { INTERESTING[rng.below(INTERESTING.len())] };
                put_u32(&mut v, at, val);
            }
            _ => {
                let at = rng.below(v.len());
                put_u32(&mut v, at, INTERESTING[rng.below(INTERESTING.len())]);
            }
        }
    }
    v
}

/// Calls `f` on `iterations` mutants of `base`; any panic is reported with seed + iteration.
fn mutation_loop(label: &str, seed: u64, iterations: usize, base: &[u8], fields: &[usize], f: impl Fn(&[u8]) -> bool) {
    let mut rng = Rng(seed);
    let started = Instant::now();
    let (mut ok, mut err) = (0usize, 0usize);
    let mut panics = Vec::new();
    for it in 0..iterations {
        let input = mutate(&mut rng, base, fields);
        let t = Instant::now();
        match catch_unwind(AssertUnwindSafe(|| f(&input))) {
            Ok(true) => ok += 1,
            Ok(false) => err += 1,
            Err(_) => panics.push(it),
        }
        assert!(t.elapsed() < Duration::from_secs(10), "{label}: seed {seed} iteration {it} took {:?}", t.elapsed());
    }
    let total = started.elapsed();
    println!("{label}: {iterations} mutants (seed {seed}): {ok} ok, {err} err, {} panics, {total:?}", panics.len());
    assert!(panics.is_empty(), "{label}: panics at seed {seed}, iterations {panics:?}");
    assert!(total < Duration::from_secs(120), "{label}: too slow ({total:?})");
}

// ----------------------------------------------------------------------------- binary FBX writer

#[allow(dead_code)]
enum P {
    I(i32),
    L(i64),
    D(f64),
    S(String),
    Df(Vec<f64>),
    Ff(Vec<f32>),
    If(Vec<i32>),
    Lf(Vec<i64>),
    /// zlib-compressed arrays
    DfZ(Vec<f64>),
    IfZ(Vec<i32>),
    /// Hand-made array: type letter, declared count, encoding, payload (the compressed length is its length).
    Raw(u8, u32, u32, Vec<u8>),
}

struct N {
    name: &'static str,
    props: Vec<P>,
    kids: Vec<N>,
}

fn n(name: &'static str, props: Vec<P>, kids: Vec<N>) -> N {
    N { name, props, kids }
}

fn s(x: &str) -> P {
    P::S(x.to_owned())
}

fn zlib(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 1)
}

fn array(out: &mut Vec<u8>, fields: &mut Vec<usize>, ty: u8, count: u32, enc: u32, data: &[u8]) {
    out.push(ty);
    fields.push(out.len());
    out.extend_from_slice(&count.to_le_bytes());
    fields.push(out.len());
    out.extend_from_slice(&enc.to_le_bytes());
    fields.push(out.len());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
}

fn emit(node: &N, out: &mut Vec<u8>, fields: &mut Vec<usize>) {
    let head = out.len();
    fields.extend([head, head + 4, head + 8]);
    out.extend_from_slice(&[0; 12]);
    out.push(node.name.len() as u8);
    out.extend_from_slice(node.name.as_bytes());
    let props_start = out.len();
    for p in &node.props {
        match p {
            P::I(v) => {
                out.push(b'I');
                out.extend_from_slice(&v.to_le_bytes());
            }
            P::L(v) => {
                out.push(b'L');
                out.extend_from_slice(&v.to_le_bytes());
            }
            P::D(v) => {
                out.push(b'D');
                out.extend_from_slice(&v.to_le_bytes());
            }
            P::S(v) => {
                out.push(b'S');
                fields.push(out.len());
                out.extend_from_slice(&(v.len() as u32).to_le_bytes());
                out.extend_from_slice(v.as_bytes());
            }
            P::Df(v) => array(out, fields, b'd', v.len() as u32, 0, &v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<_>>()),
            P::Ff(v) => array(out, fields, b'f', v.len() as u32, 0, &v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<_>>()),
            P::If(v) => array(out, fields, b'i', v.len() as u32, 0, &v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<_>>()),
            P::Lf(v) => array(out, fields, b'l', v.len() as u32, 0, &v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<_>>()),
            P::DfZ(v) => {
                let raw: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
                array(out, fields, b'd', v.len() as u32, 1, &zlib(&raw));
            }
            P::IfZ(v) => {
                let raw: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
                array(out, fields, b'i', v.len() as u32, 1, &zlib(&raw));
            }
            P::Raw(ty, count, enc, data) => array(out, fields, *ty, *count, *enc, data),
        }
    }
    let props_len = (out.len() - props_start) as u32;
    put_u32(out, head + 4, node.props.len() as u32);
    put_u32(out, head + 8, props_len);
    for k in &node.kids {
        emit(k, out, fields);
    }
    if !node.kids.is_empty() {
        out.extend_from_slice(&[0; 13]);
    }
    let end = out.len() as u32;
    put_u32(out, head, end);
}

/// A binary FBX 7.4 file; returns the bytes and the offsets of structural u32 fields.
fn fbx_file(top: &[N]) -> (Vec<u8>, Vec<usize>) {
    let mut out = b"Kaydara FBX Binary  \0\x1a\0".to_vec();
    out.extend_from_slice(&7400u32.to_le_bytes());
    let mut fields = Vec::new();
    for node in top {
        emit(node, &mut out, &mut fields);
    }
    out.extend_from_slice(&[0; 13]);
    (out, fields)
}

#[derive(Clone)]
struct Scene {
    pvi: Vec<i32>,
    cluster_indexes: Vec<i32>,
    cluster_weights: Vec<f64>,
    compress: bool,
    skin: bool,
    anim: bool,
    /// Two models that are each other's parent, one that is its own parent.
    parent_cycle: bool,
}

impl Default for Scene {
    fn default() -> Self {
        Scene {
            pvi: vec![0, 1, 2, !3],
            cluster_indexes: vec![0, 1, 2, 3],
            cluster_weights: vec![1.0, 0.5, 0.5, 1.0],
            compress: false,
            skin: true,
            anim: true,
            parent_cycle: false,
        }
    }
}

fn d(v: Vec<f64>, z: bool) -> P {
    if z { P::DfZ(v) } else { P::Df(v) }
}

fn i(v: Vec<i32>, z: bool) -> P {
    if z { P::IfZ(v) } else { P::If(v) }
}

fn ident() -> Vec<f64> {
    vec![1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]
}

fn conn(kind: &str, src: i64, dst: i64, prop: Option<&str>) -> N {
    let mut props = vec![s(kind), P::L(src), P::L(dst)];
    if let Some(p) = prop {
        props.push(s(p));
    }
    n("C", props, vec![])
}

fn scene_nodes(sc: &Scene) -> Vec<N> {
    let z = sc.compress;
    let mut kids = vec![
        n("Vertices", vec![d(vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0], z)], vec![]),
        n("PolygonVertexIndex", vec![i(sc.pvi.clone(), z)], vec![]),
        n(
            "LayerElementNormal",
            vec![I(0)],
            vec![
                n("MappingInformationType", vec![s("ByPolygonVertex")], vec![]),
                n("ReferenceInformationType", vec![s("Direct")], vec![]),
                n("Normals", vec![d([0.0, 0.0, 1.0].repeat(4), z)], vec![]),
            ],
        ),
        n(
            "LayerElementUV",
            vec![I(0)],
            vec![
                n("MappingInformationType", vec![s("ByPolygonVertex")], vec![]),
                n("ReferenceInformationType", vec![s("IndexToDirect")], vec![]),
                n("UV", vec![d(vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0], z)], vec![]),
                n("UVIndex", vec![i(vec![0, 1, 2, 3], z)], vec![]),
            ],
        ),
        n(
            "LayerElementMaterial",
            vec![I(0)],
            vec![
                n("MappingInformationType", vec![s("AllSame")], vec![]),
                n("ReferenceInformationType", vec![s("IndexToDirect")], vec![]),
                n("Materials", vec![i(vec![0], z)], vec![]),
            ],
        ),
    ];
    let geometry = n("Geometry", vec![P::L(200), s("Geometry::\0\x01Geometry"), s("Mesh")], std::mem::take(&mut kids));
    let model = |id: i64, name: &str| n("Model", vec![P::L(id), s(name), s("Mesh")], vec![]);
    let mut objects =
        vec![model(100, "Model::Mesh\0\x01Model"), geometry, n("Material", vec![P::L(400), s("Material::M\0\x01Material"), s("")], vec![])];
    let mut conns = vec![conn("OO", 200, 100, None), conn("OO", 100, 0, None), conn("OO", 400, 100, None)];
    if sc.skin {
        objects.push(model(101, "Model::Bone\0\x01Model"));
        objects.push(n("Deformer", vec![P::L(300), s("Deformer::Skin\0\x01Deformer"), s("Skin")], vec![]));
        objects.push(n(
            "Deformer",
            vec![P::L(301), s("SubDeformer::\0\x01SubDeformer"), s("Cluster")],
            vec![
                n("Indexes", vec![i(sc.cluster_indexes.clone(), z)], vec![]),
                n("Weights", vec![d(sc.cluster_weights.clone(), z)], vec![]),
                n("Transform", vec![d(ident(), z)], vec![]),
                n("TransformLink", vec![d(ident(), z)], vec![]),
            ],
        ));
        conns.extend([conn("OO", 101, 0, None), conn("OO", 300, 200, None), conn("OO", 301, 300, None), conn("OO", 101, 301, None)]);
    }
    if sc.anim {
        objects.push(n("AnimationStack", vec![P::L(500), s("AnimStack::take\0\x01AnimStack"), s("")], vec![]));
        objects.push(n("AnimationLayer", vec![P::L(501), s("AnimLayer::base\0\x01AnimLayer"), s("")], vec![]));
        objects.push(n("AnimationCurveNode", vec![P::L(502), s("AnimCurveNode::T\0\x01AnimCurveNode"), s("")], vec![]));
        objects.push(n(
            "AnimationCurve",
            vec![P::L(503), s("AnimCurve::\0\x01AnimCurve"), s("")],
            vec![
                n("KeyTime", vec![P::Lf(vec![0, 46_186_158_000, 92_372_316_000])], vec![]),
                n("KeyValueFloat", vec![P::Ff(vec![0.0, 1.0, 0.5])], vec![]),
            ],
        ));
        conns.extend([
            conn("OO", 501, 500, None),
            conn("OO", 502, 501, None),
            conn("OP", 502, 100, Some("Lcl Translation")),
            conn("OP", 503, 502, Some("d|X")),
        ]);
    }
    if sc.parent_cycle {
        objects.push(model(110, "Model::A\0\x01Model"));
        objects.push(model(111, "Model::B\0\x01Model"));
        objects.push(model(112, "Model::Self\0\x01Model"));
        conns.extend([conn("OO", 110, 111, None), conn("OO", 111, 110, None), conn("OO", 112, 112, None)]);
    }
    vec![n("Objects", vec![], objects), n("Connections", vec![], conns)]
}

use P::I;

fn fbx_bytes(sc: &Scene) -> (Vec<u8>, Vec<usize>) {
    fbx_file(&scene_nodes(sc))
}

fn load_fbx(bytes: &[u8]) -> Result<ModelData, String> {
    let mut assets = Assets::new();
    ModelData::load_bytes(Path::new("t.fbx"), bytes, &mut assets)
}

fn expect_err(bytes: &[u8], what: &str) -> String {
    let started = Instant::now();
    let r = catch_unwind(AssertUnwindSafe(|| load_fbx(bytes))).unwrap_or_else(|_| panic!("{what}: panicked"));
    assert!(started.elapsed() < Duration::from_secs(10), "{what}: too slow ({:?})", started.elapsed());
    match r {
        Err(e) => e,
        Ok(_) => panic!("{what}: expected an error, got a model"),
    }
}

// ----------------------------------------------------------------------------- FBX tests

#[test]
fn fbx_valid_files_load() {
    for compress in [false, true] {
        let (bytes, _) = fbx_bytes(&Scene { compress, ..Scene::default() });
        let m = load_fbx(&bytes).unwrap_or_else(|e| panic!("valid FBX (compress={compress}): {e}"));
        assert_eq!(m.meshes.len(), 1);
        let prim = &m.meshes[0][0];
        assert!(prim.skin.is_some(), "skin weights");
        assert_eq!(m.clips.len(), 1, "animation clip");
    }
    // a file without a skin
    let (bytes, _) = fbx_bytes(&Scene { skin: false, anim: false, ..Scene::default() });
    assert!(load_fbx(&bytes).unwrap().meshes[0][0].skin.is_none());
}

#[test]
fn fbx_ascii_loads() {
    let m = load_fbx(ASCII_FBX.as_bytes()).expect("ASCII FBX");
    assert_eq!(m.meshes.len(), 1);
}

const ASCII_FBX: &str = "; FBX 7.4.0 project file
Objects:  {
	Geometry: 200, \"Geometry::\", \"Mesh\" {
		Vertices: *12 {
			a: 0,0,0,1,0,0,1,1,0,0,1,0
		}
		PolygonVertexIndex: *4 {
			a: 0,1,2,-4
		}
	}
	Model: 100, \"Model::M\", \"Mesh\" {
	}
}
Connections:  {
	C: \"OO\",200,100
	C: \"OO\",100,0
}
";

#[test]
fn fbx_mutations_binary() {
    let (base, fields) = fbx_bytes(&Scene::default());
    mutation_loop("fbx binary", 0xFB_0001, 2000, &base, &fields, |b| load_fbx(b).is_ok());
    let (base, fields) = fbx_bytes(&Scene { compress: true, parent_cycle: true, ..Scene::default() });
    mutation_loop("fbx binary (zlib)", 0xFB_0002, 1000, &base, &fields, |b| load_fbx(b).is_ok());
}

#[test]
fn fbx_mutations_ascii() {
    mutation_loop("fbx ascii", 0xFB_0003, 1000, ASCII_FBX.as_bytes(), &[], |b| load_fbx(b).is_ok());
}

#[test]
fn fbx_every_truncation_and_single_byte_damage() {
    let (base, _) = fbx_bytes(&Scene::default());
    for len in 0..base.len() {
        let r = catch_unwind(AssertUnwindSafe(|| load_fbx(&base[..len])));
        assert!(r.is_ok(), "panic on a file truncated to {len} bytes");
    }
    // every byte set to 0x00 / 0xFF / 0x7F / 0x80
    for at in 0..base.len() {
        for v in [0x00u8, 0xFF, 0x7F, 0x80] {
            let mut b = base.clone();
            b[at] = v;
            assert!(catch_unwind(AssertUnwindSafe(|| load_fbx(&b))).is_ok(), "panic with byte {at} set to {v:#x}");
        }
    }
}

#[test]
fn fbx_huge_declared_array_count() {
    let top = |count, data: Vec<u8>| {
        fbx_file(&[n(
            "Objects",
            vec![],
            vec![n("Geometry", vec![P::L(1), s("g"), s("Mesh")], vec![n("Vertices", vec![P::Raw(b'd', count, 0, data)], vec![])])],
        )])
        .0
    };
    for count in [u32::MAX, 0x7FFF_FFFF, 64 * 1024 * 1024 + 1, 64 * 1024 * 1024, 1000] {
        expect_err(&top(count, vec![0; 16]), &format!("count {count}"));
    }
    // compressed, declaring far more than the stream holds
    expect_err(
        &fbx_file(&[n("Objects", vec![], vec![n("G", vec![P::Raw(b'd', 64 * 1024 * 1024, 1, zlib(&[0; 64]))], vec![])])]).0,
        "short stream",
    );
}

#[test]
fn fbx_arrays_beyond_the_total_budget() {
    // 20 arrays of 64M declared doubles (each over the budget once the previous ones are charged); none has data
    let props: Vec<P> = (0..20).map(|_| P::Raw(b'd', 64 * 1024 * 1024, 0, vec![])).collect();
    expect_err(&fbx_file(&[n("Objects", props, vec![])]).0, "budget");
}

#[test]
fn fbx_zlib_bomb_is_cut_off() {
    // ~64 MiB of zeros compress to a few dozen KiB; the array declares 8 doubles
    let bomb = zlib(&vec![0u8; 64 * 1024 * 1024]);
    assert!(bomb.len() < 1024 * 1024, "bomb is small ({})", bomb.len());
    let file = fbx_file(&[n("Objects", vec![P::Raw(b'd', 8, 1, bomb)], vec![])]).0;
    let e = expect_err(&file, "zlib bomb");
    assert!(e.contains("compressed"), "{e}");
}

#[test]
fn fbx_unknown_array_encoding_and_property_type() {
    expect_err(&fbx_file(&[n("Objects", vec![P::Raw(b'd', 1, 7, vec![0; 8])], vec![])]).0, "encoding 7");
    expect_err(&fbx_file(&[n("Objects", vec![P::Raw(b'Q', 1, 0, vec![0; 8])], vec![])]).0, "type Q");
}

#[test]
fn fbx_deeply_nested_nodes() {
    let mut node = n("X", vec![], vec![]);
    for _ in 0..5000 {
        node = n("X", vec![], vec![node]);
    }
    let e = expect_err(&fbx_file(&[node]).0, "deep nesting");
    assert!(e.contains("nested"), "{e}");
    // and ASCII
    let mut text = String::from("; FBX\n");
    for _ in 0..100_000 {
        text.push_str("A: {\n");
    }
    assert!(expect_err(text.as_bytes(), "deep ASCII nesting").contains("nested"));
}

#[test]
fn fbx_polygon_index_out_of_range() {
    for pvi in [
        vec![0, 1, 2, !4],
        vec![0, 1, 2, 4],
        vec![0, 1, 2, 1_000_000],
        vec![0, 1, 2, i32::MAX],
        vec![0, 1, 2, !1_000_000],
        vec![0, 1, 2, i32::MIN],
    ] {
        let (bytes, _) = fbx_bytes(&Scene { pvi: pvi.clone(), ..Scene::default() });
        let e = expect_err(&bytes, &format!("pvi {pvi:?}"));
        assert!(e.contains("out of range"), "{e}");
    }
}

#[test]
fn fbx_polygon_without_terminator_or_tiny() {
    // missing the closing negative index, 2-corner polygon: no triangles, but no crash either
    for pvi in [vec![0, 1, 2], vec![0, !1, 2, !3], vec![!0, !1, !2]] {
        let (bytes, _) = fbx_bytes(&Scene { pvi, skin: false, anim: false, ..Scene::default() });
        let r = catch_unwind(AssertUnwindSafe(|| load_fbx(&bytes)));
        assert!(r.is_ok());
    }
}

#[test]
fn fbx_cluster_data_is_validated() {
    for idx in [vec![0, 1, 2, 4], vec![0, 1, 2, -1], vec![0, 1, 2, i32::MAX]] {
        let (bytes, _) = fbx_bytes(&Scene { cluster_indexes: idx.clone(), ..Scene::default() });
        let e = expect_err(&bytes, &format!("cluster {idx:?}"));
        assert!(e.contains("control point"), "{e}");
    }
    let (bytes, _) = fbx_bytes(&Scene { cluster_weights: vec![1.0], ..Scene::default() });
    assert!(expect_err(&bytes, "weights shorter than indexes").contains("weights"));
}

#[test]
fn fbx_parent_cycles_do_not_hang() {
    let (bytes, _) = fbx_bytes(&Scene { parent_cycle: true, ..Scene::default() });
    let m = load_fbx(&bytes).expect("cyclic parents are repaired");
    for i in 1..m.nodes.len() {
        // every node reaches the root
        let (mut cur, mut steps) = (i, 0);
        while cur != 0 {
            cur = m.nodes[cur].parent.expect("parent");
            steps += 1;
            assert!(steps <= m.nodes.len(), "cycle left in the hierarchy");
        }
    }
}

#[test]
fn fbx_empty_and_garbage() {
    for b in [&b""[..], b"Kaydara FBX Binary  \0", b"Kaydara FBX Binary  \0\x1a\0", b"\0\0\0\0", b"not an fbx", &[0xFF; 4096]] {
        assert!(catch_unwind(AssertUnwindSafe(|| load_fbx(b))).is_ok());
    }
}

// ----------------------------------------------------------------------------- glTF / GLB

/// One skinned, animated triangle. Binary layout: positions 0..36, joints (u8 x4) 36..48, weights 48..96,
/// indices (u16) 96..102 (+2 padding), animation times 104..112, translations 112..136.
fn gltf_bin() -> Vec<u8> {
    let mut b = Vec::new();
    for p in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        for c in p {
            b.extend_from_slice(&c.to_le_bytes());
        }
    }
    b.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    for _ in 0..3 {
        for w in [1.0f32, 0.0, 0.0, 0.0] {
            b.extend_from_slice(&w.to_le_bytes());
        }
    }
    for i in [0u16, 1, 2, 0] {
        b.extend_from_slice(&i.to_le_bytes());
    }
    for t in [0.0f32, 1.0] {
        b.extend_from_slice(&t.to_le_bytes());
    }
    for v in [0.0f32, 0.0, 0.0, 0.0, 1.0, 0.0] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    assert_eq!(b.len(), 136);
    b
}

const GLTF_JSON: &str = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0,1]}],
"nodes":[{"mesh":0,"skin":0},{"name":"joint"}],
"meshes":[{"primitives":[{"attributes":{"POSITION":0,"JOINTS_0":1,"WEIGHTS_0":2},"indices":3}]}],
"skins":[{"joints":[1]}],
"animations":[{"samplers":[{"input":4,"output":5}],"channels":[{"sampler":0,"target":{"node":1,"path":"translation"}}]}],
"buffers":[{"byteLength":136}],
"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":12},{"buffer":0,"byteOffset":48,"byteLength":48},{"buffer":0,"byteOffset":96,"byteLength":6},{"buffer":0,"byteOffset":104,"byteLength":8},{"buffer":0,"byteOffset":112,"byteLength":24}],
"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]},
{"bufferView":1,"componentType":5121,"count":3,"type":"VEC4"},
{"bufferView":2,"componentType":5126,"count":3,"type":"VEC4"},
{"bufferView":3,"componentType":5123,"count":3,"type":"SCALAR"},
{"bufferView":4,"componentType":5126,"count":2,"type":"SCALAR","min":[0],"max":[1]},
{"bufferView":5,"componentType":5126,"count":2,"type":"VEC3"}]}"#;

fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut j = json.as_bytes().to_vec();
    while j.len() % 4 != 0 {
        j.push(b' ');
    }
    let mut bin = bin.to_vec();
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let total = 12 + 8 + j.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(j.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&j);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

fn load_glb(bytes: &[u8]) -> Result<ModelData, String> {
    let mut assets = Assets::new();
    ModelData::load_bytes(Path::new("t.glb"), bytes, &mut assets)
}

#[test]
fn gltf_valid_file_loads() {
    let m = load_glb(&glb(GLTF_JSON, &gltf_bin())).expect("valid GLB");
    assert_eq!(m.meshes.len(), 1);
    assert!(m.meshes[0][0].skin.is_some());
    assert_eq!(m.clips.len(), 1);
}

#[test]
fn gltf_index_out_of_range_is_an_error() {
    let mut bin = gltf_bin();
    bin[96..98].copy_from_slice(&9u16.to_le_bytes());
    let e = load_glb(&glb(GLTF_JSON, &bin)).unwrap_err();
    assert!(e.contains("out of range"), "{e}");
}

#[test]
fn gltf_accessors_beyond_the_buffer() {
    // an accessor claiming far more elements than the buffer holds
    for count in ["3", "1000", "4294967295", "18446744073709551615", "99999999999"] {
        let json = GLTF_JSON.replacen("\"count\":3,\"type\":\"VEC3\"", &format!("\"count\":{count},\"type\":\"VEC3\""), 1);
        let r = catch_unwind(AssertUnwindSafe(|| load_glb(&glb(&json, &gltf_bin()))));
        assert!(r.is_ok(), "panic with accessor count {count}");
    }
    // a buffer view past the end of the binary chunk
    let json = GLTF_JSON.replace("\"byteOffset\":112", "\"byteOffset\":4294967295");
    assert!(catch_unwind(AssertUnwindSafe(|| load_glb(&glb(&json, &gltf_bin())))).is_ok());
    // a declared buffer longer than the chunk
    let json = GLTF_JSON.replace("\"byteLength\":136}]", "\"byteLength\":99999999}]");
    assert!(load_glb(&glb(&json, &gltf_bin())).is_err());
}

#[test]
fn gltf_mutations_binary_container() {
    let base = glb(GLTF_JSON, &gltf_bin());
    mutation_loop("glb bytes", 0x61_0001, 2000, &base, &[], |b| load_glb(b).is_ok());
}

#[test]
fn gltf_mutations_json_numbers() {
    // swap a random number of the JSON for an extreme one, keep the container valid
    const VALUES: [&str; 11] =
        ["0", "1", "2", "5", "-1", "4294967295", "4294967296", "18446744073709551615", "99999999999", "5126", "5123"];
    let mut rng = Rng(0x61_0002);
    let jb = GLTF_JSON.as_bytes();
    let spans: Vec<(usize, usize)> = {
        let mut v = Vec::new();
        let mut k = 0;
        while k < jb.len() {
            if jb[k].is_ascii_digit() {
                let st = k;
                while k < jb.len() && jb[k].is_ascii_digit() {
                    k += 1;
                }
                v.push((st, k));
            } else {
                k += 1;
            }
        }
        v
    };
    let bin = gltf_bin();
    let started = Instant::now();
    let iterations = 2000;
    let mut panics = Vec::new();
    let (mut ok, mut err) = (0, 0);
    for it in 0..iterations {
        let mut json = GLTF_JSON.to_owned();
        for _ in 0..1 + rng.below(2) {
            let (a, b) = spans[rng.below(spans.len())];
            // (earlier replacements may have shifted offsets: only apply when the span is still on digits)
            if a < json.len() && b <= json.len() && json.as_bytes()[a..b].iter().all(u8::is_ascii_digit) {
                json.replace_range(a..b, VALUES[rng.below(VALUES.len())]);
            }
        }
        let bytes = glb(&json, &bin);
        match catch_unwind(AssertUnwindSafe(|| load_glb(&bytes).is_ok())) {
            Ok(true) => ok += 1,
            Ok(false) => err += 1,
            Err(_) => panics.push((it, json)),
        }
    }
    println!("glb json numbers: {iterations} mutants: {ok} ok, {err} err, {} panics, {:?}", panics.len(), started.elapsed());
    assert!(panics.is_empty(), "panics (seed 0x610002): {:?}", panics.iter().map(|(i, _)| *i).collect::<Vec<_>>());
    assert!(started.elapsed() < Duration::from_secs(120));
}

#[test]
fn gltf_every_truncation() {
    let base = glb(GLTF_JSON, &gltf_bin());
    for len in 0..base.len() {
        assert!(catch_unwind(AssertUnwindSafe(|| load_glb(&base[..len]))).is_ok(), "panic when truncated to {len} bytes");
    }
}
