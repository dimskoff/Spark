//! Autodesk FBX (binary 7.x) import: node hierarchy, meshes (split per material), diffuse / normal textures,
//! skins and animation stacks. Produces the same [`ModelData`] as the glTF loader, so spawning, skinning and
//! animation playback work unchanged.
//!
//! Supported: binary FBX 7100..7700, `Lcl` transforms with pre / post rotation (pivots are ignored), unit scale and
//! up axis (converted to meters, Y up), polygon meshes (triangulated as fans), normals / UV0 / material layers
//! (by polygon vertex, vertex, polygon, all-same), skin clusters (4 influences per vertex), animation curves of
//! translation / rotation / scale per animation stack. Not supported: ASCII FBX, blend shapes, cameras, lights.

use std::collections::HashMap;
use std::path::Path;

use glam::{DMat4, Mat4, Quat, Vec3, Vec4};

use crate::assets::{Assets, ImageData, MeshId, TextureId};
use crate::color::Color;
use crate::material::{BlendMode, Material};
use crate::mesh::{Aabb, MeshData, Vertex};
use crate::model::{Channel, Clip, Interpolation, ModelData, ModelNode, Primitive, Property, Skin, SkinWeights};

// ----------------------------------------------------------------------------- binary parser

#[derive(Debug, Clone)]
enum Prop {
    Int(i64),
    Float(f64),
    Str(String),
    Raw,
    Ints(Vec<i64>),
    Floats(Vec<f64>),
}

#[derive(Debug, Default)]
struct Node {
    name: String,
    props: Vec<Prop>,
    children: Vec<Node>,
}

impl Node {
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
    fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.name == name)
    }
    fn int(&self, i: usize) -> Option<i64> {
        match self.props.get(i)? {
            Prop::Int(v) => Some(*v),
            Prop::Float(v) => Some(*v as i64),
            _ => None,
        }
    }
    fn num(&self, i: usize) -> Option<f64> {
        match self.props.get(i)? {
            Prop::Int(v) => Some(*v as f64),
            Prop::Float(v) => Some(*v),
            _ => None,
        }
    }
    fn text(&self, i: usize) -> Option<&str> {
        match self.props.get(i)? {
            Prop::Str(s) => Some(s),
            _ => None,
        }
    }
    fn floats(&self) -> Vec<f64> {
        match self.props.first() {
            Some(Prop::Floats(v)) => v.clone(),
            Some(Prop::Ints(v)) => v.iter().map(|&x| x as f64).collect(),
            _ => Vec::new(),
        }
    }
    fn ints(&self) -> Vec<i64> {
        match self.props.first() {
            Some(Prop::Ints(v)) => v.clone(),
            Some(Prop::Floats(v)) => v.iter().map(|&x| x as i64).collect(),
            _ => Vec::new(),
        }
    }
    /// `Properties70` entry: values after the 4 header strings.
    fn property(&self, name: &str) -> Option<Vec<f64>> {
        let p70 = self.child("Properties70").or_else(|| self.child("Properties60"))?;
        for p in p70.all("P") {
            if p.text(0) == Some(name) {
                let v: Vec<f64> = (4..p.props.len()).filter_map(|i| p.num(i)).collect();
                return Some(v);
            }
        }
        None
    }
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.p.checked_add(n).filter(|&e| e <= self.d.len()).ok_or("unexpected end of file")?;
        let s = &self.d[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
}

fn read_node(r: &mut Reader, wide: bool, depth: usize) -> Result<Option<Node>, String> {
    if depth > 64 {
        return Err("nodes are nested too deeply".into());
    }
    let (end, nprops, plen) = if wide { (r.u64()? as usize, r.u64()? as usize, r.u64()? as usize) } else { (r.u32()? as usize, r.u32()? as usize, r.u32()? as usize) };
    let name_len = r.u8()? as usize;
    if end == 0 && nprops == 0 && plen == 0 && name_len == 0 {
        return Ok(None);
    }
    let name = String::from_utf8_lossy(r.take(name_len)?).into_owned();
    let props_end = r.p + plen;
    let mut props = Vec::with_capacity(nprops.min(1024));
    for _ in 0..nprops {
        let t = r.u8()?;
        let prop = match t {
            b'Y' => Prop::Int(i16::from_le_bytes(r.take(2)?.try_into().unwrap()) as i64),
            b'C' => Prop::Int(r.u8()? as i64),
            b'I' => Prop::Int(r.u32()? as i32 as i64),
            b'L' => Prop::Int(r.u64()? as i64),
            b'F' => Prop::Float(f32::from_le_bytes(r.take(4)?.try_into().unwrap()) as f64),
            b'D' => Prop::Float(f64::from_le_bytes(r.take(8)?.try_into().unwrap())),
            b'S' => {
                let n = r.u32()? as usize;
                Prop::Str(String::from_utf8_lossy(r.take(n)?).into_owned())
            }
            b'R' => {
                let n = r.u32()? as usize;
                r.take(n)?;
                Prop::Raw
            }
            b'f' | b'd' | b'l' | b'i' | b'b' => {
                let count = r.u32()? as usize;
                let enc = r.u32()?;
                let clen = r.u32()? as usize;
                let size = match t {
                    b'f' | b'i' => 4,
                    b'b' => 1,
                    _ => 8,
                };
                let raw = r.take(clen)?;
                let data: Vec<u8> = if enc == 1 {
                    miniz_oxide::inflate::decompress_to_vec_zlib(raw).map_err(|e| format!("bad compressed array: {e:?}"))?
                } else {
                    raw.to_vec()
                };
                if data.len() < count * size {
                    return Err("array is shorter than its count".into());
                }
                match t {
                    b'f' => Prop::Floats(data.chunks_exact(4).take(count).map(|c| f32::from_le_bytes(c.try_into().unwrap()) as f64).collect()),
                    b'd' => Prop::Floats(data.chunks_exact(8).take(count).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()),
                    b'l' => Prop::Ints(data.chunks_exact(8).take(count).map(|c| i64::from_le_bytes(c.try_into().unwrap())).collect()),
                    b'i' => Prop::Ints(data.chunks_exact(4).take(count).map(|c| i32::from_le_bytes(c.try_into().unwrap()) as i64).collect()),
                    _ => Prop::Ints(data.iter().take(count).map(|&b| b as i64).collect()),
                }
            }
            other => return Err(format!("unknown property type '{}'", other as char)),
        };
        props.push(prop);
    }
    r.p = props_end.max(r.p);
    let mut children = Vec::new();
    while r.p < end {
        match read_node(r, wide, depth + 1)? {
            Some(c) => children.push(c),
            None => break,
        }
    }
    r.p = end.max(r.p);
    Ok(Some(Node { name, props, children }))
}

fn parse(bytes: &[u8]) -> Result<(u32, Vec<Node>), String> {
    const MAGIC: &[u8] = b"Kaydara FBX Binary  \0";
    if !bytes.starts_with(MAGIC) {
        if bytes.starts_with(b"; FBX") || bytes.iter().take(2000).any(|&b| b == b':') {
            let mut a = Ascii { s: bytes, p: 0 };
            return Ok((7400, a.nodes(0)?));
        }
        return Err("not an FBX file".into());
    }
    let mut r = Reader { d: bytes, p: 23 };
    let version = r.u32()?;
    let wide = version >= 7500;
    let mut roots = Vec::new();
    while r.p + 13 <= bytes.len() {
        match read_node(&mut r, wide, 0)? {
            Some(n) => roots.push(n),
            None => break,
        }
    }
    Ok((version, roots))
}


// ----------------------------------------------------------------------------- ASCII parser

struct Ascii<'a> {
    s: &'a [u8],
    p: usize,
}

impl Ascii<'_> {
    fn skip(&mut self) {
        while self.p < self.s.len() {
            match self.s[self.p] {
                b' ' | b'\t' | b'\r' | b'\n' => self.p += 1,
                b';' => {
                    while self.p < self.s.len() && self.s[self.p] != b'\n' {
                        self.p += 1;
                    }
                }
                _ => break,
            }
        }
    }

    fn nodes(&mut self, depth: usize) -> Result<Vec<Node>, String> {
        if depth > 64 {
            return Err("nodes are nested too deeply".into());
        }
        let mut out = Vec::new();
        loop {
            self.skip();
            if self.p >= self.s.len() {
                break;
            }
            if self.s[self.p] == b'}' {
                self.p += 1;
                break;
            }
            let start = self.p;
            while self.p < self.s.len() && self.s[self.p] != b':' && self.s[self.p] != b'\n' {
                self.p += 1;
            }
            if self.p >= self.s.len() || self.s[self.p] != b':' {
                return Err(format!("bad ASCII FBX near byte {start}"));
            }
            let name = String::from_utf8_lossy(&self.s[start..self.p]).trim().to_owned();
            self.p += 1;
            let mut node = Node { name: name.clone(), ..Default::default() };
            if name == "a" {
                let start = self.p;
                while self.p < self.s.len() && self.s[self.p] != b'}' {
                    self.p += 1;
                }
                let text = String::from_utf8_lossy(&self.s[start..self.p]);
                let v: Vec<f64> = text.split(',').filter_map(|x| x.trim().parse::<f64>().ok()).collect();
                node.props.push(Prop::Floats(v));
                out.push(node);
                continue;
            }
            // values until end of line or '{'
            loop {
                while self.p < self.s.len() && (self.s[self.p] == b' ' || self.s[self.p] == b'\t' || self.s[self.p] == b',') {
                    self.p += 1;
                }
                if self.p >= self.s.len() {
                    break;
                }
                match self.s[self.p] {
                    b'\n' | b'\r' => break,
                    b'{' => {
                        self.p += 1;
                        let kids = self.nodes(depth + 1)?;
                        // `Vertices: *N { a: ... }` keeps its array as the node's property
                        if kids.len() == 1 && kids[0].name == "a" {
                            node.props.extend(kids.into_iter().next().unwrap().props);
                        } else {
                            node.children = kids;
                        }
                        break;
                    }
                    b'"' => {
                        self.p += 1;
                        let st = self.p;
                        while self.p < self.s.len() && self.s[self.p] != b'"' {
                            self.p += 1;
                        }
                        node.props.push(Prop::Str(String::from_utf8_lossy(&self.s[st..self.p]).into_owned()));
                        self.p += 1;
                    }
                    b'*' => {
                        self.p += 1;
                        while self.p < self.s.len() && self.s[self.p].is_ascii_digit() {
                            self.p += 1;
                        }
                    }
                    _ => {
                        let st = self.p;
                        while self.p < self.s.len() && !matches!(self.s[self.p], b',' | b'\n' | b'\r' | b'{' | b' ' | b'\t') {
                            self.p += 1;
                        }
                        let w = String::from_utf8_lossy(&self.s[st..self.p]).into_owned();
                        match w.parse::<i64>() {
                            Ok(i) => node.props.push(Prop::Int(i)),
                            Err(_) => match w.parse::<f64>() {
                                Ok(f) => node.props.push(Prop::Float(f)),
                                Err(_) => node.props.push(Prop::Str(w)),
                            },
                        }
                    }
                }
            }
            out.push(node);
        }
        Ok(out)
    }
}
// ----------------------------------------------------------------------------- scene helpers

fn clean_name(s: &str) -> String {
    s.split('\0').next().unwrap_or("").to_owned()
}

fn euler_quat(deg: [f64; 3], order: i64) -> Quat {
    let (x, y, z) = (deg[0].to_radians() as f32, deg[1].to_radians() as f32, deg[2].to_radians() as f32);
    let rx = Quat::from_rotation_x(x);
    let ry = Quat::from_rotation_y(y);
    let rz = Quat::from_rotation_z(z);
    // FBX order names the axis applied first, e.g. XYZ = Rz * Ry * Rx.
    match order {
        1 => ry * rz * rx,
        2 => rx * rz * ry,
        3 => rz * rx * ry,
        4 => ry * rx * rz,
        5 => rx * ry * rz,
        _ => rz * ry * rx,
    }
}

fn vec3(v: Option<Vec<f64>>, default: [f64; 3]) -> [f64; 3] {
    match v {
        Some(v) if v.len() >= 3 => [v[0], v[1], v[2]],
        _ => default,
    }
}

struct Transform {
    t: [f64; 3],
    pre: [f64; 3],
    r: [f64; 3],
    post: [f64; 3],
    s: [f64; 3],
    order: i64,
}

impl Transform {
    fn of(node: &Node) -> Self {
        Transform {
            t: vec3(node.property("Lcl Translation"), [0.0; 3]),
            pre: vec3(node.property("PreRotation"), [0.0; 3]),
            r: vec3(node.property("Lcl Rotation"), [0.0; 3]),
            post: vec3(node.property("PostRotation"), [0.0; 3]),
            s: vec3(node.property("Lcl Scaling"), [1.0; 3]),
            order: node.property("RotationOrder").and_then(|v| v.first().copied()).unwrap_or(0.0) as i64,
        }
    }

    fn rotation(&self, r: [f64; 3]) -> Quat {
        euler_quat(self.pre, 0) * euler_quat(r, self.order) * euler_quat(self.post, 0).inverse()
    }
}

fn mat_from(v: &[f64]) -> Option<Mat4> {
    if v.len() < 16 {
        return None;
    }
    let mut a = [0.0f64; 16];
    a.copy_from_slice(&v[..16]);
    Some(DMat4::from_cols_array(&a).as_mat4())
}

struct Connection {
    kind: String,
    src: i64,
    dst: i64,
    prop: Option<String>,
}

fn to_srgb_passthrough(c: f64) -> f32 {
    (c as f32).clamp(0.0, 1.0)
}

fn smooth_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut n = vec![Vec3::ZERO; positions.len()];
    for t in indices.chunks_exact(3) {
        let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let (pa, pb, pc) = (Vec3::from(positions[a]), Vec3::from(positions[b]), Vec3::from(positions[c]));
        let f = (pb - pa).cross(pc - pa);
        n[a] += f;
        n[b] += f;
        n[c] += f;
    }
    n.into_iter().map(|v| v.normalize_or(Vec3::Y).to_array()).collect()
}

#[derive(Clone, Copy)]
enum Mapping {
    PolygonVertex,
    Vertex,
    Polygon,
    AllSame,
}

struct Layer {
    mapping: Mapping,
    indexed: bool,
    values: Vec<f64>,
    index: Vec<i64>,
}

impl Layer {
    fn read(node: &Node, data: &str, index: &str) -> Option<Layer> {
        let mapping = match node.child("MappingInformationType").and_then(|n| n.text(0)) {
            Some("ByPolygonVertex") => Mapping::PolygonVertex,
            Some("ByVertice") | Some("ByVertex") | Some("ByControlPoint") => Mapping::Vertex,
            Some("ByPolygon") => Mapping::Polygon,
            Some("AllSame") => Mapping::AllSame,
            _ => Mapping::PolygonVertex,
        };
        let indexed = node.child("ReferenceInformationType").and_then(|n| n.text(0)) == Some("IndexToDirect");
        let values = node.child(data).map(|n| n.floats()).unwrap_or_default();
        let idx = node.child(index).map(|n| n.ints()).unwrap_or_default();
        Some(Layer { mapping, indexed: indexed || (!idx.is_empty() && data != "Materials"), values, index: idx })
    }

    /// Element index for a polygon vertex.
    fn slot(&self, pv: usize, vert: usize, poly: usize) -> usize {
        let i = match self.mapping {
            Mapping::PolygonVertex => pv,
            Mapping::Vertex => vert,
            Mapping::Polygon => poly,
            Mapping::AllSame => 0,
        };
        if self.indexed && !self.index.is_empty() { self.index.get(i).copied().unwrap_or(0).max(0) as usize } else { i }
    }
}


// ----------------------------------------------------------------------------- textures

#[derive(Clone, Default)]
struct TextureSet {
    color: Option<TextureId>,
    normal: Option<TextureId>,
    orm: Option<TextureId>,
    has_metal: bool,
    has_alpha: bool,
}

fn dir_files<'a>(dir: &Path, cache: &'a mut HashMap<std::path::PathBuf, Vec<(String, std::path::PathBuf)>>) -> &'a [(String, std::path::PathBuf)] {
    cache.entry(dir.to_path_buf()).or_insert_with(|| {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_file() {
                    out.push((p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default(), p));
                }
            }
        }
        out
    })
}

fn norm_name(s: &str) -> String {
    s.to_lowercase().replace([' ', '-'], "_")
}

/// Finds `file` (as written in the FBX, any directory) next to the model: same name ignoring case, spaces vs
/// underscores and the image extension.
fn locate_texture(dir: &Path, model: &Path, file: &str, cache: &mut HashMap<std::path::PathBuf, Vec<(String, std::path::PathBuf)>>) -> Option<std::path::PathBuf> {
    let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
    let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(base);
    let want = norm_name(stem);
    let model_stem = model.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let dirs = [dir.to_path_buf(), dir.join("textures"), dir.join("Textures"), dir.join("tex"), dir.join(format!("{model_stem}.fbm"))];
    for d in dirs {
        // exact name first (also works for packed games, where directories cannot be listed)
        let exact = d.join(base);
        if crate::vfs::read(&exact).is_ok() {
            return Some(exact);
        }
        let files = dir_files(&d, cache);
        let image_ext = |n: &str| matches!(n.rsplit('.').next(), Some("png" | "jpg" | "jpeg"));
        if let Some((_, p)) = files.iter().find(|(n, _)| image_ext(n) && norm_name(n.rsplit_once('.').map(|(s, _)| s).unwrap_or(n)) == want) {
            return Some(p.clone());
        }
    }
    None
}

const COLOR_SUFFIXES: &[&str] = &["_basecolor", "_base_color", "_albedo", "_diffusemap", "_diffuse", "_color_1k", "_color", "_rgba", "_col", "_d"];

fn find_sibling(dir: &Path, prefix: &str, suffixes: &[&str], cache: &mut HashMap<std::path::PathBuf, Vec<(String, std::path::PathBuf)>>) -> Option<std::path::PathBuf> {
    let files = dir_files(dir, cache);
    for s in suffixes {
        let want = format!("{prefix}{s}");
        if let Some((_, p)) = files.iter().find(|(n, _)| {
            matches!(n.rsplit('.').next(), Some("png" | "jpg" | "jpeg")) && norm_name(n.rsplit_once('.').map(|(a, _)| a).unwrap_or(n)) == want
        }) {
            return Some(p.clone());
        }
    }
    None
}

/// Last resort for generic material names ("bark_mat", "cluster_mat_spring_ex"): a color texture whose name contains
/// every meaningful word of the material name ("summer" preferred when several seasons match).
fn find_by_tokens(dir: &Path, mat_name: &str, cache: &mut HashMap<std::path::PathBuf, Vec<(String, std::path::PathBuf)>>) -> Option<std::path::PathBuf> {
    const SKIP: &[&str] = &["mat", "material", "ex", "tex", "default", "lod", "mtl", "shader", "m"];
    let tokens: Vec<&str> = mat_name.split('_').filter(|w| w.len() > 1 && !SKIP.contains(w) && !w.chars().all(|c| c.is_ascii_digit())).collect();
    if tokens.is_empty() {
        return None;
    }
    let files = dir_files(dir, cache);
    let mut best: Option<(i32, &std::path::PathBuf)> = None;
    for (n, p) in files {
        if !matches!(n.rsplit('.').next(), Some("png" | "jpg" | "jpeg")) {
            continue;
        }
        let stem = norm_name(n.rsplit_once('.').map(|(a, _)| a).unwrap_or(n));
        // the color suffix is optional here ("Cluster_Bare.png"); the file must share at least one word with the material
        let matched = tokens.iter().filter(|w| stem.contains(**w)).count() as i32;
        if matched == 0 || stem.ends_with("_opacity") || stem.ends_with("_normal") || stem.ends_with("_ao") || stem.ends_with("_roughness") {
            continue;
        }
        let score = matched * 10 + if COLOR_SUFFIXES.iter().any(|s| stem.ends_with(s)) { 3 } else { 0 } + if stem.contains("summer") { 2 } else { 0 } - (stem.len() as i32 / 64);
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, p));
        }
    }
    best.map(|(_, p)| p.clone())
}

fn read_image(path: &Path) -> Option<image::RgbaImage> {
    let data = crate::vfs::read(path).ok()?;
    image::load_from_memory(&data).ok().map(|i| i.to_rgba8())
}

fn load_plain_texture(path: &Path, assets: &mut Assets, cache: &mut HashMap<std::path::PathBuf, Option<TextureId>>) -> Option<TextureId> {
    if let Some(t) = cache.get(path) {
        return *t;
    }
    let id = read_image(path).map(|img| assets.add_texture(ImageData::new(img.width(), img.height(), img.into_raw())));
    cache.insert(path.to_path_buf(), id);
    id
}

/// Base color (with an opacity map merged into alpha), normal map and a packed ORM map found next to the color
/// texture by name: `<name>_AO`, `<name>_Roughness`, `<name>_Metallic`, `<name>_Opacity` / `_alfa` ...
fn build_texture_set(color_path: &Path, cache: &mut HashMap<std::path::PathBuf, Vec<(String, std::path::PathBuf)>>, assets: &mut Assets) -> TextureSet {
    let mut set = TextureSet::default();
    let Some(mut color) = read_image(color_path) else { return set };
    let dir = color_path.parent().unwrap_or(Path::new("."));
    let stem = norm_name(&color_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
    let mut prefix = stem.clone();
    for s in COLOR_SUFFIXES {
        if let Some(p) = stem.strip_suffix(s) {
            prefix = p.to_owned();
            break;
        }
    }
    let resize = |img: &image::RgbaImage, w: u32, h: u32| image::imageops::resize(img, w, h, image::imageops::FilterType::Triangle);
    // opacity
    if let Some(p) = find_sibling(dir, &prefix, &["_opacity", "_alfa", "_alpha", "_mask", "_alfa_edge"], cache) {
        if let Some(mut mask) = read_image(&p) {
            if mask.dimensions() != color.dimensions() {
                mask = resize(&mask, color.width(), color.height());
            }
            for (c, m) in color.pixels_mut().zip(mask.pixels()) {
                c.0[3] = ((m.0[0] as u32 + m.0[1] as u32 + m.0[2] as u32) / 3) as u8;
            }
            set.has_alpha = true;
        }
    } else {
        set.has_alpha = color.pixels().any(|p| p.0[3] < 250);
    }
    set.color = Some(assets.add_texture(ImageData::new(color.width(), color.height(), color.clone().into_raw())));
    // normal
    if let Some(p) = find_sibling(dir, &prefix, &["_normalgl", "_normal", "_normalsmap", "_normals", "_n", "_nrm"], cache) {
        if let Some(img) = read_image(&p) {
            set.normal = Some(assets.add_texture(ImageData::new(img.width(), img.height(), img.into_raw())));
        }
    }
    // ORM
    let ao = find_sibling(dir, &prefix, &["_ao", "_ambientocclusion", "_occlusion", "_ao_q"], cache).and_then(|p| read_image(&p));
    let rough = find_sibling(dir, &prefix, &["_roughness", "_rough", "_r"], cache).and_then(|p| read_image(&p));
    let metal = find_sibling(dir, &prefix, &["_metallic", "_metalness", "_metal"], cache).and_then(|p| read_image(&p));
    let packed = find_sibling(dir, &prefix, &["_unitymetallicsmoothness"], cache).and_then(|p| read_image(&p));
    if ao.is_some() || rough.is_some() || metal.is_some() || packed.is_some() {
        let (mut w, mut h) = (1, 1);
        for i in [&ao, &rough, &metal, &packed].into_iter().flatten() {
            w = w.max(i.width());
            h = h.max(i.height());
        }
        let (w, h) = (w.min(2048), h.min(2048));
        let fit = |i: Option<image::RgbaImage>| i.map(|i| if i.dimensions() == (w, h) { i } else { resize(&i, w, h) });
        let (ao, rough, metal, packed) = (fit(ao), fit(rough), fit(metal), fit(packed));
        let mut out = image::RgbaImage::new(w, h);
        for (x, y, px) in out.enumerate_pixels_mut() {
            let get = |i: &Option<image::RgbaImage>, ch: usize| i.as_ref().map(|i| i.get_pixel(x, y).0[ch]);
            let r = get(&ao, 0).unwrap_or(255);
            let g = get(&rough, 0).or_else(|| get(&packed, 3).map(|s| 255 - s)).unwrap_or(255);
            let b = get(&metal, 0).or_else(|| get(&packed, 0)).unwrap_or(255);
            px.0 = [r, g, b, 255];
        }
        set.has_metal = metal.is_some() || packed.is_some();
        set.orm = Some(assets.add_texture(ImageData::new(w, h, out.into_raw())));
    }
    set
}
// ----------------------------------------------------------------------------- loader

pub fn load(path: &Path, bytes: &[u8], assets: &mut Assets) -> Result<ModelData, String> {
    let err = |e: String| format!("cannot load model '{}': {e}", path.display());
    let (_version, top) = parse(bytes).map_err(err)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();

    let objects_node = top.iter().find(|n| n.name == "Objects").ok_or_else(|| err("no Objects section".into()))?;
    let mut objects: HashMap<i64, &Node> = HashMap::new();
    for o in &objects_node.children {
        if let Some(id) = o.int(0) {
            objects.insert(id, o);
        }
    }
    let mut conns: Vec<Connection> = Vec::new();
    if let Some(c) = top.iter().find(|n| n.name == "Connections") {
        for n in c.all("C") {
            let (Some(kind), Some(src), Some(dst)) = (n.text(0), n.int(1), n.int(2)) else { continue };
            conns.push(Connection { kind: kind.to_owned(), src, dst, prop: n.text(3).map(str::to_owned) });
        }
    }
    let children_of = |id: i64, ty: &str| -> Vec<&Connection> {
        conns.iter().filter(|c| c.dst == id && objects.get(&c.src).map(|o| o.name == ty).unwrap_or(false)).collect()
    };

    // Global settings: unit scale (cm per unit) and up axis.
    let mut unit = 1.0f64;
    let mut up_axis = 1i64;
    if let Some(gs) = top.iter().find(|n| n.name == "GlobalSettings") {
        if let Some(v) = gs.property("UnitScaleFactor") {
            unit = v.first().copied().unwrap_or(1.0);
        }
        if let Some(v) = gs.property("UpAxis") {
            up_axis = v.first().copied().unwrap_or(1.0) as i64;
        }
    }
    if std::env::var_os("SPARK_FBX_DEBUG").is_some() {
        if let Some(gs) = top.iter().find(|n| n.name == "GlobalSettings") {
            let g = |k: &str| gs.property(k).and_then(|v| v.first().copied());
            log::warn!("{name}: axes up={:?}/{:?} front={:?}/{:?} coord={:?}/{:?} unit={:?}", g("UpAxis"), g("UpAxisSign"), g("FrontAxis"), g("FrontAxisSign"), g("CoordAxis"), g("CoordAxisSign"), g("UnitScaleFactor"));
        }
    }
    let meters = (unit * 0.01) as f32;
    let mut model = ModelData { name: name.clone(), bounds: Aabb::EMPTY, ..Default::default() };

    // Root node carries the unit scale and the up-axis conversion.
    let root_rot = if up_axis == 2 { Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2) } else { Quat::IDENTITY };
    model.nodes.push(ModelNode {
        name: "__fbx_root".into(),
        parent: None,
        children: Vec::new(),
        translation: Vec3::ZERO,
        rotation: root_rot,
        scale: Vec3::splat(meters),
        mesh: None,
        skin: None,
    });
    model.roots.push(0);

    // Model nodes in file order.
    let mut node_of: HashMap<i64, usize> = HashMap::new();
    let mut model_ids: Vec<i64> = Vec::new();
    for o in &objects_node.children {
        if o.name == "Model" {
            if let Some(id) = o.int(0) {
                node_of.insert(id, model.nodes.len());
                model_ids.push(id);
                let tf = Transform::of(o);
                if std::env::var_os("SPARK_FBX_DEBUG").is_some() && clean_name(o.text(1).unwrap_or("")) == "Bip01" {
                    log::warn!("{name}: Bip01 props: t={:?} pre={:?} r={:?} post={:?} s={:?} order={} all={:?}", tf.t, tf.pre, tf.r, tf.post, tf.s, tf.order, o.child("Properties70").map(|p| p.all("P").map(|n| n.text(0).unwrap_or("").to_owned()).collect::<Vec<_>>()));
                }
                model.nodes.push(ModelNode {
                    name: clean_name(o.text(1).unwrap_or("node")),
                    parent: Some(0),
                    children: Vec::new(),
                    translation: Vec3::new(tf.t[0] as f32, tf.t[1] as f32, tf.t[2] as f32),
                    rotation: tf.rotation(tf.r),
                    scale: Vec3::new(tf.s[0] as f32, tf.s[1] as f32, tf.s[2] as f32),
                    mesh: None,
                    skin: None,
                });
            }
        }
    }
    for &id in &model_ids {
        let ni = node_of[&id];
        let parent = conns.iter().find(|c| c.src == id && c.kind == "OO" && node_of.contains_key(&c.dst)).map(|c| node_of[&c.dst]).unwrap_or(0);
        model.nodes[ni].parent = Some(parent);
    }
    for i in 1..model.nodes.len() {
        if let Some(p) = model.nodes[i].parent {
            model.nodes[p].children.push(i);
        }
    }
    if std::env::var_os("SPARK_FBX_DEBUG").is_some() {
        for &c in &model.nodes[0].children.clone() {
            let n = &model.nodes[c];
            log::warn!("{name}: top node {} t={:?} r={:?} s={:?} children={}", n.name, n.translation, n.rotation.to_euler(glam::EulerRot::XYZ), n.scale, n.children.len());
        }
    }

    // Textures: located next to the model (case / space / underscore tolerant); PBR sets are discovered by name.
    let mut dir_cache: HashMap<std::path::PathBuf, Vec<(String, std::path::PathBuf)>> = HashMap::new();
    let mut tex_cache: HashMap<std::path::PathBuf, Option<TextureId>> = HashMap::new();
    let mut set_cache: HashMap<std::path::PathBuf, TextureSet> = HashMap::new();

    let texture_file = |tex_id: i64| -> Option<String> {
        let t = objects.get(&tex_id)?;
        let f = |n: &Node| n.child("RelativeFilename").and_then(|c| c.text(0)).filter(|s| !s.is_empty()).or_else(|| n.child("FileName").and_then(|c| c.text(0)).filter(|s| !s.is_empty())).map(str::to_owned);
        f(t).or_else(|| {
            conns.iter().filter(|c| c.dst == tex_id).filter_map(|c| objects.get(&c.src)).find(|o| o.name == "Video").and_then(|v| f(v))
        })
    };

    let mut material_cache: HashMap<i64, Material> = HashMap::new();
    let mut build_material = |mat_id: Option<i64>, assets: &mut Assets| -> Material {
        let Some(id) = mat_id else { return Material { pbr: true, roughness: 0.7, ..Default::default() } };
        if let Some(m) = material_cache.get(&id) {
            return *m;
        }
        let mut m = Material { roughness: 0.7, metallic: 0.0, pbr: true, ..Default::default() };
        if let Some(node) = objects.get(&id) {
            let col = node.property("DiffuseColor").or_else(|| node.property("Diffuse")).or_else(|| node.property("base_color"));
            if let Some(c) = col.filter(|c| c.len() >= 3) {
                m.color = Color::rgba(to_srgb_passthrough(c[0]), to_srgb_passthrough(c[1]), to_srgb_passthrough(c[2]), 1.0);
            }
            if let Some(e) = node.property("EmissiveColor").filter(|c| c.len() >= 3) {
                let k = node.property("EmissiveFactor").and_then(|v| v.first().copied()).unwrap_or(1.0);
                m.emissive = Color::rgb(to_srgb_passthrough(e[0] * k), to_srgb_passthrough(e[1] * k), to_srgb_passthrough(e[2] * k));
            }
            if let Some(o) = node.property("Opacity").and_then(|v| v.first().copied()) {
                if o < 0.999 {
                    m.color.a = o as f32;
                    m.blend = BlendMode::Alpha;
                }
            }
            if let Some(s) = node.property("Shininess").or_else(|| node.property("ShininessExponent")).and_then(|v| v.first().copied()) {
                m.roughness = (1.0 - (s / 100.0).clamp(0.0, 1.0).sqrt()).clamp(0.2, 1.0) as f32;
            }
        }
        for c in conns.iter().filter(|c| c.dst == id && c.kind == "OP") {
            let Some(prop) = c.prop.as_deref() else { continue };
            if !objects.get(&c.src).is_some_and(|o| o.name == "Texture") {
                continue;
            }
            let Some(file) = texture_file(c.src) else { continue };
            let Some(path_found) = locate_texture(dir, path, &file, &mut dir_cache) else {
                log::warn!("{name}: texture '{file}' not found next to the model");
                continue;
            };
            let lp = prop.to_lowercase();
            if std::env::var_os("SPARK_FBX_DEBUG").is_some() {
                log::warn!("{name}: material {id} texture prop \"{prop}\" -> {file}");
            }
            if lp.contains("normal") || lp.contains("bump") {
                if m.normal_map.is_none() {
                    m.normal_map = load_plain_texture(&path_found, assets, &mut tex_cache);
                }
            } else if (lp.contains("diffuse") || lp.contains("base") || lp == "color") && m.texture.is_none() {
                let set = set_cache.entry(path_found.clone()).or_insert_with(|| build_texture_set(&path_found, &mut dir_cache, assets)).clone();
                m.texture = set.color;
                if m.texture.is_some() {
                    m.color = Color::rgba(1.0, 1.0, 1.0, m.color.a);
                }
                if m.normal_map.is_none() {
                    m.normal_map = set.normal;
                }
                if set.orm.is_some() {
                    m.orm_map = set.orm;
                    m.roughness = 1.0;
                    if set.has_metal {
                        m.metallic = 1.0;
                    }
                }
                if set.has_alpha && m.blend == BlendMode::Auto {
                    m.double_sided = true;
                }
            }
        }
        if m.texture.is_none() {
            // No texture connected (some exporters drop the link): look for files named after the material.
            let mat_name = objects.get(&id).and_then(|o| o.text(1)).map(|s| norm_name(&clean_name(s))).unwrap_or_default();
            if std::env::var_os("SPARK_FBX_DEBUG").is_some() {
                log::warn!("{name}: material {id} has no color texture; name \"{mat_name}\"");
            }
            if !mat_name.is_empty() {
                let mut suffixes: Vec<&str> = vec![""];
                suffixes.extend_from_slice(COLOR_SUFFIXES);
                let found = find_sibling(dir, &mat_name, &suffixes, &mut dir_cache).or_else(|| find_by_tokens(dir, &mat_name, &mut dir_cache));
                if let Some(p) = found {
                    let set = set_cache.entry(p.clone()).or_insert_with(|| build_texture_set(&p, &mut dir_cache, assets)).clone();
                    m.texture = set.color;
                    if m.texture.is_some() {
                        m.color = Color::rgba(1.0, 1.0, 1.0, m.color.a);
                    }
                    if m.normal_map.is_none() {
                        m.normal_map = set.normal;
                    }
                    if set.orm.is_some() {
                        m.orm_map = set.orm;
                        m.roughness = 1.0;
                    }
                }
            }
        }
        material_cache.insert(id, m);
        m
    };
    // Skins: skin deformer id -> clusters.
    struct ClusterInfo {
        bone_node: usize,
        indexes: Vec<i64>,
        weights: Vec<f64>,
        transform: Mat4,
        link: Mat4,
    }
    let mut geometry_skin: HashMap<i64, Vec<ClusterInfo>> = HashMap::new();
    for (&id, o) in &objects {
        if o.name != "Deformer" || o.text(2) != Some("Skin") {
            continue;
        }
        let mut list = Vec::new();
        for c in children_of(id, "Deformer") {
            let Some(cl) = objects.get(&c.src).filter(|n| n.text(2) == Some("Cluster")) else { continue };
            let bone = conns.iter().find(|k| k.dst == c.src && k.kind == "OO" && node_of.contains_key(&k.src)).map(|k| node_of[&k.src]);
            let Some(bone_node) = bone else { continue };
            list.push(ClusterInfo {
                bone_node,
                indexes: cl.child("Indexes").map(|n| n.ints()).unwrap_or_default(),
                weights: cl.child("Weights").map(|n| n.floats()).unwrap_or_default(),
                transform: cl.child("Transform").and_then(|n| mat_from(&n.floats())).unwrap_or(Mat4::IDENTITY),
                link: cl.child("TransformLink").and_then(|n| mat_from(&n.floats())).unwrap_or(Mat4::IDENTITY),
            });
        }
        for g in conns.iter().filter(|c| c.src == id && objects.get(&c.dst).map(|o| o.name == "Geometry").unwrap_or(false)) {
            // the same skin deformer is only used by one geometry in practice
            geometry_skin.entry(g.dst).or_default().extend(list.iter().map(|c| ClusterInfo {
                bone_node: c.bone_node,
                indexes: c.indexes.clone(),
                weights: c.weights.clone(),
                transform: c.transform,
                link: c.link,
            }));
        }
    }

    // Meshes.
    let mut skin_of_node: HashMap<usize, usize> = HashMap::new();
    for &mid in &model_ids {
        let ni = node_of[&mid];
        let Some(geo_conn) = conns.iter().find(|c| c.dst == mid && c.kind == "OO" && objects.get(&c.src).map(|o| o.name == "Geometry").unwrap_or(false)) else { continue };
        let geo = objects[&geo_conn.src];
        let mats: Vec<i64> = conns.iter().filter(|c| c.dst == mid && c.kind == "OO" && objects.get(&c.src).map(|o| o.name == "Material").unwrap_or(false)).map(|c| c.src).collect();
        let verts = geo.child("Vertices").map(|n| n.floats()).unwrap_or_default();
        let pvi = geo.child("PolygonVertexIndex").map(|n| n.ints()).unwrap_or_default();
        if verts.len() < 9 || pvi.len() < 3 {
            continue;
        }
        let normals = geo.child("LayerElementNormal").and_then(|n| Layer::read(n, "Normals", "NormalsIndex"));
        let uvs = geo.child("LayerElementUV").and_then(|n| Layer::read(n, "UV", "UVIndex"));
        let matl = geo.child("LayerElementMaterial").and_then(|n| Layer::read(n, "Materials", "Materials_none"));

        // Skin data per control point (top 4 influences).
        let clusters = geometry_skin.get(&geo_conn.src);
        let ncp = verts.len() / 3;
        let mut influences: Vec<Vec<(u16, f32)>> = Vec::new();
        let mut bake = Mat4::IDENTITY;
        let mut skin_index = None;
        if let Some(cl) = clusters.filter(|c| !c.is_empty()) {
            influences = vec![Vec::new(); ncp];
            let mut joints = Vec::new();
            let mut inverse_bind = Vec::new();
            for (ji, c) in cl.iter().enumerate() {
                joints.push(c.bone_node);
                inverse_bind.push(c.link.inverse());
                for (k, &ix) in c.indexes.iter().enumerate() {
                    if let (Some(list), Some(&w)) = (influences.get_mut(ix as usize), c.weights.get(k)) {
                        list.push((ji as u16, w as f32));
                    }
                }
            }
            // Vertices are skinned from "bind world" space: bring the mesh-local positions there with the mesh node's own
            // global bind transform (the cluster `Transform` is not reliable across exporters).
            bake = model.node_matrix(0).inverse() * model.node_matrix(ni);
            if std::env::var_os("SPARK_FBX_DEBUG").is_some() {
                let root_inv = model.node_matrix(0).inverse();
                let mut worst = 0.0f32;
                let mut worst_name = String::new();
                for c in cl {
                    let global = root_inv * model.node_matrix(c.bone_node);
                    let d = (global.to_cols_array().iter().zip(c.link.to_cols_array().iter()).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max)) / c.link.col(3).truncate().length().max(1.0);
                    if d > worst {
                        worst = d;
                        worst_name = model.nodes[c.bone_node].name.clone();
                    }
                }
                log::warn!("{name}: skin bind pose deviation {worst:.4} (bone {worst_name}), {} clusters", cl.len());
                if worst > 0.01 {
                    for c in cl.iter().take(3) {
                        let n = &model.nodes[c.bone_node];
                        let mut chain = Vec::new();
                        let mut cur = Some(c.bone_node);
                        while let Some(i) = cur {
                            let m = &model.nodes[i];
                            chain.push(format!("{} t={:?} r={:?} s={:?}", m.name, m.translation, m.rotation.to_euler(glam::EulerRot::XYZ), m.scale));
                            cur = m.parent;
                        }
                        log::warn!("  bone {} link={:?}\n    global={:?}\n    chain: {}", n.name, c.link.to_cols_array(), (root_inv * model.node_matrix(c.bone_node)).to_cols_array(), chain.join(" <- "));
                    }
                }
            }
            model.skins.push(Skin { joints, inverse_bind });
            skin_index = Some(model.skins.len() - 1);
            for list in &mut influences {
                list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                list.truncate(4);
            }
        }
        let normal_bake = bake.inverse().transpose();

        struct Group {
            vertices: Vec<Vertex>,
            cps: Vec<usize>,
            indices: Vec<u32>,
            lookup: HashMap<(usize, usize, usize), u32>,
        }
        let mut groups: HashMap<usize, Group> = HashMap::new();
        let mut poly: Vec<(usize, usize)> = Vec::new(); // (control point, polygon vertex index)
        let mut poly_index = 0usize;
        for (pv, &raw) in pvi.iter().enumerate() {
            let end = raw < 0;
            let cp = if end { (!raw) as usize } else { raw as usize };
            poly.push((cp, pv));
            if end {
                let mi = match &matl {
                    Some(l) => l.values.get(l.slot(poly[0].1, poly[0].0, poly_index)).copied().unwrap_or(0.0).max(0.0) as usize,
                    None => 0,
                };
                let mi = if let Some(l) = &matl {
                    // `Materials` holds the material index directly
                    let i = match l.mapping {
                        Mapping::Polygon => poly_index,
                        _ => 0,
                    };
                    l.values.get(i).copied().unwrap_or(mi as f64).max(0.0) as usize
                } else {
                    mi
                };
                let g = groups.entry(mi).or_insert_with(|| Group { vertices: Vec::new(), cps: Vec::new(), indices: Vec::new(), lookup: HashMap::new() });
                let mut ids = Vec::with_capacity(poly.len());
                for &(cp, pvi_) in &poly {
                    let ni_ = normals.as_ref().map(|l| l.slot(pvi_, cp, poly_index)).unwrap_or(usize::MAX);
                    let ui_ = uvs.as_ref().map(|l| l.slot(pvi_, cp, poly_index)).unwrap_or(usize::MAX);
                    let key = (cp, ni_, ui_);
                    let id = *g.lookup.entry(key).or_insert_with(|| {
                        let p = Vec3::new(verts[cp * 3] as f32, verts[cp * 3 + 1] as f32, verts[cp * 3 + 2] as f32);
                        let nrm = match &normals {
                            Some(l) if l.values.len() >= (ni_ + 1) * 3 => Vec3::new(l.values[ni_ * 3] as f32, l.values[ni_ * 3 + 1] as f32, l.values[ni_ * 3 + 2] as f32),
                            _ => Vec3::ZERO,
                        };
                        let uv = match &uvs {
                            Some(l) if l.values.len() >= (ui_ + 1) * 2 => [l.values[ui_ * 2] as f32, 1.0 - l.values[ui_ * 2 + 1] as f32],
                            _ => [0.0, 0.0],
                        };
                        let (p, nrm) = if skin_index.is_some() { (bake.transform_point3(p), normal_bake.transform_vector3(nrm)) } else { (p, nrm) };
                        g.vertices.push(Vertex { position: p.to_array(), normal: nrm.normalize_or_zero().to_array(), uv });
                        g.cps.push(cp);
                        (g.vertices.len() - 1) as u32
                    });
                    ids.push(id);
                }
                for k in 1..ids.len().saturating_sub(1) {
                    g.indices.extend_from_slice(&[ids[0], ids[k], ids[k + 1]]);
                }
                poly.clear();
                poly_index += 1;
            }
        }

        let mut order: Vec<usize> = groups.keys().copied().collect();
        order.sort_unstable();
        let mut prims = Vec::new();
        for mi in order {
            let mut g = groups.remove(&mi).unwrap();
            if g.indices.is_empty() {
                continue;
            }
            if g.vertices.iter().all(|v| v.normal == [0.0; 3]) {
                let pos: Vec<[f32; 3]> = g.vertices.iter().map(|v| v.position).collect();
                let n = smooth_normals(&pos, &g.indices);
                for (v, n) in g.vertices.iter_mut().zip(n) {
                    v.normal = n;
                }
            }
            let skin = if skin_index.is_some() {
                let mut joints = Vec::with_capacity(g.cps.len());
                let mut weights = Vec::with_capacity(g.cps.len());
                for &cp in &g.cps {
                    let list = &influences[cp];
                    let mut j = [0u16; 4];
                    let mut w = [0f32; 4];
                    for (k, &(ji, wt)) in list.iter().enumerate() {
                        j[k] = ji;
                        w[k] = wt;
                    }
                    let s: f32 = w.iter().sum();
                    if s > 0.0 { w = w.map(|x| x / s) } else { w = [1.0, 0.0, 0.0, 0.0] }
                    joints.push(j);
                    weights.push(w);
                }
                Some(SkinWeights { joints, weights })
            } else {
                None
            };
            let material = build_material(mats.get(mi).copied(), assets);
            if std::env::var_os("SPARK_FBX_DEBUG").is_some() {
                log::warn!("{name}: mesh {} verts={} tex={} normal={} orm={} color={:?}", model.nodes[ni].name, g.vertices.len(), material.texture.is_some(), material.normal_map.is_some(), material.orm_map.is_some(), material.color);
            }
            let mesh: MeshId = assets.add_mesh(MeshData::new(g.vertices, g.indices));
            prims.push(Primitive { mesh, material, skin });
        }
        if prims.is_empty() {
            continue;
        }
        model.meshes.push(prims);
        model.nodes[ni].mesh = Some(model.meshes.len() - 1);
        if let Some(s) = skin_index {
            model.nodes[ni].skin = Some(s);
            skin_of_node.insert(ni, s);
        }
    }

    // Animation stacks -> clips.
    const TICKS: f64 = 46_186_158_000.0;
    for (&stack_id, stack) in &objects {
        if stack.name != "AnimationStack" {
            continue;
        }
        // "skeleton|walk" (Blender / Source exports prefix the armature name) -> "walk"
        let full = clean_name(stack.text(1).unwrap_or("take"));
        let short = full.rsplit('|').next().unwrap_or(&full).to_owned();
        let mut clip = Clip { name: if short.is_empty() { full } else { short }, duration: 0.0, channels: Vec::new() };
        let layers: Vec<i64> = children_of(stack_id, "AnimationLayer").iter().map(|c| c.src).collect();
        for layer in layers {
            for cn in children_of(layer, "AnimationCurveNode") {
                let cn_id = cn.src;
                let Some(target) = conns.iter().find(|c| c.src == cn_id && c.kind == "OP" && node_of.contains_key(&c.dst)) else { continue };
                let node = node_of[&target.dst];
                let prop_name = target.prop.clone().unwrap_or_default();
                let property = match prop_name.as_str() {
                    "Lcl Translation" => Property::Translation,
                    "Lcl Rotation" => Property::Rotation,
                    "Lcl Scaling" => Property::Scale,
                    _ => continue,
                };
                // curves per axis
                let mut axes: [Option<(Vec<f64>, Vec<f64>)>; 3] = [None, None, None];
                for cv in conns.iter().filter(|c| c.dst == cn_id && c.kind == "OP") {
                    let Some(curve) = objects.get(&cv.src).filter(|o| o.name == "AnimationCurve") else { continue };
                    let axis = match cv.prop.as_deref() {
                        Some("d|X") => 0,
                        Some("d|Y") => 1,
                        Some("d|Z") => 2,
                        _ => continue,
                    };
                    let times: Vec<f64> = curve.child("KeyTime").map(|n| n.ints().iter().map(|&t| t as f64 / TICKS).collect()).unwrap_or_default();
                    let vals = curve.child("KeyValueFloat").map(|n| n.floats()).unwrap_or_default();
                    if !times.is_empty() && times.len() == vals.len() {
                        axes[axis] = Some((times, vals));
                    }
                }
                let mut times: Vec<f64> = axes.iter().flatten().flat_map(|(t, _)| t.iter().copied()).collect();
                if times.is_empty() {
                    continue;
                }
                times.sort_by(|a, b| a.partial_cmp(b).unwrap());
                times.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
                let tf = Transform::of(objects_node.children.iter().find(|o| o.int(0) == Some(target.dst)).unwrap());
                let default = match property {
                    Property::Translation => tf.t,
                    Property::Rotation => tf.r,
                    Property::Scale => tf.s,
                };
                let sample = |axis: usize, t: f64| -> f64 {
                    match &axes[axis] {
                        None => default[axis],
                        Some((ts, vs)) => {
                            if t <= ts[0] {
                                return vs[0];
                            }
                            if t >= ts[ts.len() - 1] {
                                return vs[vs.len() - 1];
                            }
                            let i = ts.partition_point(|&k| k <= t).saturating_sub(1).min(ts.len() - 2);
                            let k = (t - ts[i]) / (ts[i + 1] - ts[i]).max(1e-9);
                            vs[i] + (vs[i + 1] - vs[i]) * k
                        }
                    }
                };
                let mut values = Vec::with_capacity(times.len());
                let mut prev: Option<Quat> = None;
                for &t in &times {
                    let v = [sample(0, t), sample(1, t), sample(2, t)];
                    values.push(match property {
                        Property::Rotation => {
                            let mut q = tf.rotation(v);
                            if let Some(p) = prev {
                                if p.dot(q) < 0.0 {
                                    q = -q;
                                }
                            }
                            prev = Some(q);
                            Vec4::from(q)
                        }
                        _ => Vec4::new(v[0] as f32, v[1] as f32, v[2] as f32, 0.0),
                    });
                }
                let end = *times.last().unwrap() as f32;
                clip.duration = clip.duration.max(end);
                clip.channels.push(Channel { node, property, interpolation: Interpolation::Linear, times: times.iter().map(|&t| t as f32).collect(), values });
            }
        }
        if !clip.channels.is_empty() {
            model.clips.push(clip);
        }
    }
    model.clips.sort_by(|a, b| a.name.cmp(&b.name));

    // Bounds (skinned meshes: bind positions through the root).
    let root_m = model.node_matrix(0);
    for (i, n) in model.nodes.iter().enumerate() {
        if let Some(m) = n.mesh {
            let matrix = if n.skin.is_some() { root_m } else { model.node_matrix(i) };
            for p in &model.meshes[m] {
                model.bounds = model.bounds.union(&assets.mesh_bounds(p.mesh).transform(&matrix));
            }
        }
    }
    if model.meshes.is_empty() {
        return Err(err("the file has no polygon meshes".into()));
    }
    Ok(model)
}
