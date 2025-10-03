use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;

use base64::prelude::*;
use parking_lot::RwLock;
use parry3d_f64::math::{Point, Vector};
use serde::de::Error as DeError;
use serde::Deserialize;

use crate::squarion::{
    AggregateMetadata, Deserialize as SquarionDeserialize, MaterialId, RangeZYX, VertexGrid,
    VoxelCellData,
};
use crate::svo::{Svo, SvoNode};

#[derive(Debug, Clone, PartialEq)]
pub struct WorldBounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl WorldBounds {
    fn from_range(range: &RangeZYX) -> Self {
        let mins = range.origin.map(|v| v as f64) / 4.0;
        let size = range.size.map(|v| v as f64) / 4.0;
        WorldBounds {
            min: [mins.x, mins.y, mins.z],
            max: [mins.x + size.x, mins.y + size.y, mins.z + size.z],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelSection {
    pub name: String,
    pub size: usize,
    pub bounds: WorldBounds,
}

#[derive(Debug, Clone)]
pub struct LodChunk {
    pub height: u8,
    pub coords: Point<i32>,
    pub origin: Point<i32>,
    pub extent: i32,
    pub voxels: Arc<VoxelCellData>,
    pub metadata: Arc<AggregateMetadata>,
    pub world_bounds: Option<WorldBounds>,
}

#[derive(Debug, Clone)]
pub struct LodLevel {
    pub height: u8,
    pub chunks: Vec<LodChunk>,
}

pub const VOXEL_SCALE: f32 = 0.25;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MeshBuffers {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub colors: Vec<f32>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LodGeometry {
    pub level: u8,
    pub voxel_size: f32,
    pub voxel_count: usize,
    pub mesh: MeshBuffers,
    pub material_counts: BTreeMap<MaterialId, usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChunkGeometry {
    pub lod_level: u8,
    pub coords: [i32; 3],
    pub voxel_count: usize,
    pub mesh: MeshBuffers,
    pub material_counts: BTreeMap<MaterialId, usize>,
}

pub struct LoadedBlueprint {
    pub model: ModelSection,
    pub tree: Svo<Option<Arc<VoxelCellData>>>,
    pub lods: BTreeMap<u8, LodLevel>,
}

impl LoadedBlueprint {
    pub fn geometry(&self) -> BTreeMap<u8, LodGeometry> {
        self.lods
            .values()
            .map(|level| (level.height, build_lod_geometry(level)))
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct RenderableBlueprint {
    pub model: ModelSection,
    pub lods: Vec<LodGeometry>,
}

pub type GeometryCache = Arc<RwLock<HashMap<(u8, (i32, i32, i32)), Arc<ChunkGeometry>>>>;

#[derive(Debug, Clone)]
pub struct Renderer {
    cache: GeometryCache,
}

impl Renderer {
    pub fn new() -> Self {
        Renderer {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_cache(cache: GeometryCache) -> Self {
        Renderer { cache }
    }

    pub fn load_blueprint_from_path<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<RenderableBlueprint, RendererError> {
        let file = File::open(path.as_ref()).map_err(RendererError::Io)?;
        let loaded = BlueprintLoader::load_from_reader(file)?;
        Ok(self.materialize_renderable(loaded))
    }

    pub fn materialize_renderable(&self, loaded: LoadedBlueprint) -> RenderableBlueprint {
        let LoadedBlueprint { model, lods, .. } = loaded;
        let lods = lods
            .into_values()
            .map(|lod| self.geometry_for_level(&lod))
            .collect();

        RenderableBlueprint { model, lods }
    }

    fn geometry_for_level(&self, level: &LodLevel) -> LodGeometry {
        let occupancy = collect_level_occupancy(level);
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut colors = Vec::new();
        let mut indices = Vec::new();
        let mut material_counts: BTreeMap<MaterialId, usize> = BTreeMap::new();
        let mut voxel_count = 0usize;
        let mut index_offset = 0u32;

        for chunk in &level.chunks {
            let chunk_geometry = self.geometry_for_chunk(chunk, &occupancy);
            let mesh = &chunk_geometry.mesh;
            let vertex_count = (mesh.positions.len() / 3) as u32;

            positions.extend(&mesh.positions);
            normals.extend(&mesh.normals);
            colors.extend(&mesh.colors);
            indices.extend(mesh.indices.iter().map(|idx| idx + index_offset));
            index_offset += vertex_count;

            voxel_count += chunk_geometry.voxel_count;

            for (material, count) in &chunk_geometry.material_counts {
                *material_counts.entry(material.clone()).or_insert(0) += count;
            }
        }

        LodGeometry {
            level: level.height,
            voxel_size: VOXEL_SCALE * (1 << level.height) as f32,
            voxel_count,
            mesh: MeshBuffers {
                positions,
                normals,
                colors,
                indices,
            },
            material_counts,
        }
    }

    fn geometry_for_chunk(
        &self,
        chunk: &LodChunk,
        occupancy: &HashSet<(i32, i32, i32)>,
    ) -> Arc<ChunkGeometry> {
        let key = (
            chunk.height,
            (chunk.coords.x, chunk.coords.y, chunk.coords.z),
        );
        if let Some(cached) = self.cache.read().get(&key).cloned() {
            return cached;
        }

        let mut cache = self.cache.write();
        if let Some(cached) = cache.get(&key) {
            return Arc::clone(cached);
        }

        let geometry = Arc::new(compute_chunk_geometry(chunk, occupancy));
        cache.insert(key, Arc::clone(&geometry));
        geometry
    }

    pub fn cache(&self) -> GeometryCache {
        Arc::clone(&self.cache)
    }
}

pub fn build_lod_geometry(level: &LodLevel) -> LodGeometry {
    let occupancy = collect_level_occupancy(level);
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    let mut material_counts: BTreeMap<MaterialId, usize> = BTreeMap::new();
    let mut voxel_count = 0usize;
    let mut index_offset = 0u32;

    for chunk in &level.chunks {
        let chunk_geometry = compute_chunk_geometry(chunk, &occupancy);
        let mesh = &chunk_geometry.mesh;
        let vertex_count = (mesh.positions.len() / 3) as u32;

        positions.extend(&mesh.positions);
        normals.extend(&mesh.normals);
        colors.extend(&mesh.colors);
        indices.extend(mesh.indices.iter().map(|idx| idx + index_offset));
        index_offset += vertex_count;

        voxel_count += chunk_geometry.voxel_count;

        for (material, count) in chunk_geometry.material_counts {
            *material_counts.entry(material).or_insert(0) += count;
        }
    }

    LodGeometry {
        level: level.height,
        voxel_size: VOXEL_SCALE * (1 << level.height) as f32,
        voxel_count,
        mesh: MeshBuffers {
            positions,
            normals,
            colors,
            indices,
        },
        material_counts,
    }
}

fn collect_level_occupancy(level: &LodLevel) -> HashSet<(i32, i32, i32)> {
    let mut occupancy = HashSet::new();
    for chunk in &level.chunks {
        chunk.voxels.grid.for_each_filled_cell(|position, _| {
            occupancy.insert((position.x, position.y, position.z));
        });
    }
    occupancy
}

fn compute_chunk_geometry(chunk: &LodChunk, occupancy: &HashSet<(i32, i32, i32)>) -> ChunkGeometry {
    let mut filled_cells = Vec::new();
    let mut material_counts: BTreeMap<MaterialId, usize> = BTreeMap::new();

    chunk
        .voxels
        .grid
        .for_each_filled_cell(|position, vertex_material| {
            filled_cells.push(position);
            if let Some(material_id) = chunk
                .voxels
                .material_mapper()
                .resolve(vertex_material.material)
            {
                *material_counts.entry(material_id.clone()).or_insert(0) += 1;
            }
        });

    let mut builder = MeshBuilder::new(chunk);
    for position in &filled_cells {
        builder.emit_cell_faces(*position, occupancy);
    }

    ChunkGeometry {
        lod_level: chunk.height,
        coords: [chunk.coords.x, chunk.coords.y, chunk.coords.z],
        voxel_count: filled_cells.len(),
        mesh: builder.finish(),
        material_counts,
    }
}

struct MeshBuilder<'a> {
    grid: &'a VertexGrid,
    offset_scale: f32,
    positions: Vec<f32>,
    normals: Vec<f32>,
    indices: Vec<u32>,
    vertex_indices: HashMap<(i32, i32, i32), u32>,
}

impl<'a> MeshBuilder<'a> {
    fn new(chunk: &'a LodChunk) -> Self {
        let chunk_scale = VOXEL_SCALE * (1 << chunk.height) as f32;
        MeshBuilder {
            grid: &chunk.voxels.grid,
            offset_scale: chunk_scale / 84.0,
            positions: Vec::new(),
            normals: Vec::new(),
            indices: Vec::new(),
            vertex_indices: HashMap::new(),
        }
    }

    fn emit_cell_faces(&mut self, position: Point<i32>, occupancy: &HashSet<(i32, i32, i32)>) {
        for face in FACE_DEFINITIONS.iter() {
            let neighbor = (
                position.x + face.neighbor_offset.0,
                position.y + face.neighbor_offset.1,
                position.z + face.neighbor_offset.2,
            );
            if occupancy.contains(&neighbor) {
                continue;
            }

            let corners: [Point<i32>; 4] = face
                .corners
                .map(|delta| position + Vector::new(delta[0], delta[1], delta[2]));
            self.emit_face(&corners);
        }
    }

    fn emit_face(&mut self, corners: &[Point<i32>; 4]) {
        let i0 = self.vertex_index(corners[0]);
        let i1 = self.vertex_index(corners[1]);
        let i2 = self.vertex_index(corners[2]);
        let i3 = self.vertex_index(corners[3]);
        self.push_triangle(i0, i1, i2);
        self.push_triangle(i0, i2, i3);
    }

    fn vertex_index(&mut self, point: Point<i32>) -> u32 {
        let key = (point.x, point.y, point.z);
        if let Some(&index) = self.vertex_indices.get(&key) {
            return index;
        }

        let base_position = [
            point.x as f32 * VOXEL_SCALE,
            point.y as f32 * VOXEL_SCALE,
            point.z as f32 * VOXEL_SCALE,
        ];
        let raw_offset = self.grid.vertex_offset(point);
        let offset = [
            (raw_offset[0] as f32 - 126.0) * self.offset_scale,
            (raw_offset[1] as f32 - 126.0) * self.offset_scale,
            (raw_offset[2] as f32 - 126.0) * self.offset_scale,
        ];
        let position = [
            base_position[0] + offset[0],
            base_position[1] + offset[1],
            base_position[2] + offset[2],
        ];

        let index = (self.positions.len() / 3) as u32;
        self.positions.extend_from_slice(&position);
        self.normals.extend_from_slice(&[0.0, 0.0, 0.0]);
        self.vertex_indices.insert(key, index);
        index
    }

    fn push_triangle(&mut self, i0: u32, i1: u32, i2: u32) {
        self.indices.extend_from_slice(&[i0, i1, i2]);
        let normal = triangle_normal(&self.positions, i0, i1, i2);
        self.add_normal(i0, normal);
        self.add_normal(i1, normal);
        self.add_normal(i2, normal);
    }

    fn add_normal(&mut self, index: u32, normal: [f32; 3]) {
        let base = index as usize * 3;
        self.normals[base] += normal[0];
        self.normals[base + 1] += normal[1];
        self.normals[base + 2] += normal[2];
    }

    fn finish(mut self) -> MeshBuffers {
        normalize_normals(&mut self.normals);
        let colors = bake_vertex_colors(&self.normals);
        MeshBuffers {
            positions: self.positions,
            normals: self.normals,
            colors,
            indices: self.indices,
        }
    }
}

#[derive(Copy, Clone)]
struct Light {
    direction: [f32; 3],
    color: [f32; 3],
    intensity: f32,
}

const AMBIENT_INTENSITY: f32 = 0.3;
const BASE_COLOR: [f32; 3] = [0.78, 0.78, 0.78];

const LIGHTS: [Light; 3] = [
    Light {
        direction: [-0.44464687, -0.7877919, -0.40383157],
        color: [1.0, 0.98, 0.95],
        intensity: 0.35,
    },
    Light {
        direction: [0.3053629, 0.72423023, 0.6183005],
        color: [0.9, 0.92, 1.0],
        intensity: 0.45,
    },
    Light {
        direction: [0.0, -1.0, 0.0],
        color: [0.8, 0.82, 0.85],
        intensity: 0.2,
    },
];

fn bake_vertex_colors(normals: &[f32]) -> Vec<f32> {
    let mut colors = Vec::with_capacity(normals.len());
    for normal in normals.chunks_exact(3) {
        let mut r = BASE_COLOR[0] * AMBIENT_INTENSITY;
        let mut g = BASE_COLOR[1] * AMBIENT_INTENSITY;
        let mut b = BASE_COLOR[2] * AMBIENT_INTENSITY;

        for light in LIGHTS.iter() {
            let dot = normal[0] * light.direction[0]
                + normal[1] * light.direction[1]
                + normal[2] * light.direction[2];
            if dot > 0.0 {
                let contribution = dot * light.intensity;
                r += BASE_COLOR[0] * contribution * light.color[0];
                g += BASE_COLOR[1] * contribution * light.color[1];
                b += BASE_COLOR[2] * contribution * light.color[2];
            }
        }

        let r = r.clamp(0.0, 1.0);
        let g = g.clamp(0.0, 1.0);
        let b = b.clamp(0.0, 1.0);
        colors.extend_from_slice(&[r, g, b]);
    }
    colors
}

struct FaceDefinition {
    neighbor_offset: (i32, i32, i32),
    corners: [[i32; 3]; 4],
}

const FACE_DEFINITIONS: [FaceDefinition; 6] = [
    FaceDefinition {
        neighbor_offset: (1, 0, 0),
        corners: [[0, -1, -1], [0, 0, -1], [0, 0, 0], [0, -1, 0]],
    },
    FaceDefinition {
        neighbor_offset: (-1, 0, 0),
        corners: [[-1, -1, -1], [-1, -1, 0], [-1, 0, 0], [-1, 0, -1]],
    },
    FaceDefinition {
        neighbor_offset: (0, 1, 0),
        corners: [[-1, 0, -1], [-1, 0, 0], [0, 0, 0], [0, 0, -1]],
    },
    FaceDefinition {
        neighbor_offset: (0, -1, 0),
        corners: [[-1, -1, -1], [0, -1, -1], [0, -1, 0], [-1, -1, 0]],
    },
    FaceDefinition {
        neighbor_offset: (0, 0, 1),
        corners: [[-1, -1, 0], [0, -1, 0], [0, 0, 0], [-1, 0, 0]],
    },
    FaceDefinition {
        neighbor_offset: (0, 0, -1),
        corners: [[-1, -1, -1], [-1, 0, -1], [0, 0, -1], [0, -1, -1]],
    },
];

fn triangle_normal(positions: &[f32], i0: u32, i1: u32, i2: u32) -> [f32; 3] {
    let v0 = vertex_at(positions, i0);
    let v1 = vertex_at(positions, i1);
    let v2 = vertex_at(positions, i2);
    let a = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
    let b = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn vertex_at(positions: &[f32], index: u32) -> [f32; 3] {
    let base = index as usize * 3;
    [positions[base], positions[base + 1], positions[base + 2]]
}

fn normalize_normals(normals: &mut [f32]) {
    let mut i = 0;
    while i < normals.len() {
        let nx = normals[i];
        let ny = normals[i + 1];
        let nz = normals[i + 2];
        let length = (nx * nx + ny * ny + nz * nz).sqrt();
        if length > f32::EPSILON {
            normals[i] = nx / length;
            normals[i + 1] = ny / length;
            normals[i + 2] = nz / length;
        }
        i += 3;
    }
}

#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Base64(base64::DecodeError),
    Squarion(crate::squarion::DeserializeError),
    InvalidHeight(u32),
    EmptyBlueprint,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io(err) => write!(f, "failed to read blueprint: {}", err),
            LoadError::Json(err) => write!(f, "failed to parse blueprint json: {}", err),
            LoadError::Base64(err) => write!(f, "failed to decode base64 payload: {}", err),
            LoadError::Squarion(err) => {
                write!(f, "failed to decompress blueprint chunk: {:?}", err)
            }
            LoadError::InvalidHeight(h) => {
                write!(f, "blueprint chunk reported invalid height {}", h)
            }
            LoadError::EmptyBlueprint => write!(f, "blueprint contained no voxel chunks"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<serde_json::Error> for LoadError {
    fn from(value: serde_json::Error) -> Self {
        LoadError::Json(value)
    }
}

#[derive(Debug)]
pub enum RendererError {
    Io(std::io::Error),
    Load(LoadError),
}

impl fmt::Display for RendererError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RendererError::Io(err) => write!(f, "failed to open blueprint: {}", err),
            RendererError::Load(err) => write!(f, "failed to decode blueprint: {}", err),
        }
    }
}

impl std::error::Error for RendererError {}

impl From<LoadError> for RendererError {
    fn from(value: LoadError) -> Self {
        RendererError::Load(value)
    }
}

impl From<base64::DecodeError> for LoadError {
    fn from(value: base64::DecodeError) -> Self {
        LoadError::Base64(value)
    }
}

impl From<crate::squarion::DeserializeError> for LoadError {
    fn from(value: crate::squarion::DeserializeError) -> Self {
        LoadError::Squarion(value)
    }
}

impl From<std::io::Error> for LoadError {
    fn from(value: std::io::Error) -> Self {
        LoadError::Io(value)
    }
}

#[derive(Deserialize)]
struct BlueprintManifest {
    #[serde(rename = "Model")]
    model: ManifestModel,
    #[serde(rename = "VoxelData")]
    voxel_data: Vec<ManifestChunk>,
}

#[derive(Deserialize)]
struct ManifestModel {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Size")]
    size: usize,
    #[serde(rename = "Bounds")]
    bounds: ManifestBounds,
}

#[derive(Deserialize)]
struct ManifestBounds {
    min: ManifestVec3,
    max: ManifestVec3,
}

#[derive(Deserialize)]
struct ManifestVec3 {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Deserialize)]
struct ManifestChunk {
    h: u32,
    #[serde(deserialize_with = "deserialize_number_long")]
    x: i64,
    #[serde(deserialize_with = "deserialize_number_long")]
    y: i64,
    #[serde(deserialize_with = "deserialize_number_long")]
    z: i64,
    records: ManifestRecords,
}

#[derive(Deserialize)]
struct ManifestRecords {
    meta: ManifestRecord,
    voxel: ManifestRecord,
}

#[derive(Deserialize)]
struct ManifestRecord {
    data: ManifestBinary,
}

#[derive(Deserialize)]
struct ManifestBinary {
    #[serde(rename = "$binary")]
    payload: String,
    #[serde(rename = "$type")]
    _ty: Option<String>,
}

#[derive(Hash, PartialEq, Eq)]
struct ChunkKey {
    height: u8,
    coords: (i32, i32, i32),
}

impl ChunkKey {
    fn new(height: u8, coords: Point<i32>) -> Self {
        ChunkKey {
            height,
            coords: (coords.x, coords.y, coords.z),
        }
    }
}

fn deserialize_number_long<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumberLongRepr {
        Int(i64),
        UInt(u64),
        Obj {
            #[serde(rename = "$numberLong")]
            value: serde_json::Value,
        },
    }

    match NumberLongRepr::deserialize(deserializer)? {
        NumberLongRepr::Int(v) => Ok(v),
        NumberLongRepr::UInt(v) => Ok(v as i64),
        NumberLongRepr::Obj { value } => match value {
            serde_json::Value::String(s) => s.parse().map_err(DeError::custom),
            serde_json::Value::Number(num) => num
                .as_i64()
                .or_else(|| num.as_u64().map(|v| v as i64))
                .ok_or_else(|| DeError::custom("invalid $numberLong payload")),
            _ => Err(DeError::custom("invalid $numberLong payload")),
        },
    }
}

fn build_node(
    height: u8,
    range: &RangeZYX,
    chunks: &HashMap<ChunkKey, Arc<VoxelCellData>>,
) -> SvoNode<Option<Arc<VoxelCellData>>> {
    let extent = range.size.x;
    let coords = range.origin.map(|v| v / extent);
    let key = ChunkKey::new(height, coords);
    let value = chunks.get(&key).cloned();
    if height == 0 {
        SvoNode::Leaf(value)
    } else {
        let child_height = height - 1;
        let children = range
            .split_at_center()
            .map(|child| build_node(child_height, &child, chunks));
        SvoNode::Internal(value, Box::new(children))
    }
}

pub struct BlueprintLoader;

impl BlueprintLoader {
    pub fn load_from_reader(mut reader: impl Read) -> Result<LoadedBlueprint, LoadError> {
        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer).map_err(LoadError::from)?;
        let manifest: BlueprintManifest = serde_json::from_slice(&buffer)?;

        let mut chunk_voxels = HashMap::new();
        let mut lods: BTreeMap<u8, LodLevel> = BTreeMap::new();
        let mut max_height = None;

        for chunk in manifest.voxel_data.iter() {
            if chunk.h < 3 {
                return Err(LoadError::InvalidHeight(chunk.h));
            }
            let height = (chunk.h - 3) as u8;
            let coords = Point::new(chunk.x as i32, chunk.y as i32, chunk.z as i32);
            let extent = 1i32 << height;
            let origin = Point::new(coords.x * extent, coords.y * extent, coords.z * extent);

            let voxel_bytes = BASE64_STANDARD.decode(&chunk.records.voxel.data.payload)?;
            let meta_bytes = BASE64_STANDARD.decode(&chunk.records.meta.data.payload)?;
            let voxels = Arc::new(<VoxelCellData as SquarionDeserialize>::decompress(
                &voxel_bytes,
            )?);
            let metadata = Arc::new(<AggregateMetadata as SquarionDeserialize>::decompress(
                &meta_bytes,
            )?);

            let lod = lods.entry(height).or_insert_with(|| LodLevel {
                height,
                chunks: Vec::new(),
            });
            let world_bounds = metadata
                .heavy_current
                .bounding_box
                .as_ref()
                .map(WorldBounds::from_range);
            lod.chunks.push(LodChunk {
                height,
                coords,
                origin,
                extent,
                voxels: Arc::clone(&voxels),
                metadata: Arc::clone(&metadata),
                world_bounds,
            });

            chunk_voxels.insert(ChunkKey::new(height, coords), voxels);

            match max_height {
                Some((current, _)) if current > height => {}
                Some((current, _)) if current == height => {}
                _ => {
                    max_height = Some((height, origin));
                }
            }
        }

        let (root_height, root_origin) = match max_height {
            Some(pair) => pair,
            None => return Err(LoadError::EmptyBlueprint),
        };
        let extent = 1i32 << root_height;
        let range = RangeZYX::with_extent(root_origin, extent);
        let tree = Svo {
            root: build_node(root_height, &range, &chunk_voxels),
            range,
        };

        let model = ModelSection {
            name: manifest.model.name,
            size: manifest.model.size,
            bounds: WorldBounds {
                min: [
                    manifest.model.bounds.min.x,
                    manifest.model.bounds.min.y,
                    manifest.model.bounds.min.z,
                ],
                max: [
                    manifest.model.bounds.max.x,
                    manifest.model.bounds.max.y,
                    manifest.model.bounds.max.z,
                ],
            },
        };

        Ok(LoadedBlueprint { model, tree, lods })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blueprint::{Blueprint, CoreInfo, CoreSize, CoreType};
    use crate::voxelization::Voxelizer;
    use parry3d_f64::bounding_volume::Aabb;
    use parry3d_f64::math::{Isometry, Point, Vector};
    use parry3d_f64::shape::{TriMesh, TriMeshFlags};

    fn build_loader_fixture() -> (
        LoadedBlueprint,
        HashMap<(u8, i32, i32, i32), bool>,
        ([f64; 3], [f64; 3]),
    ) {
        let vertices = vec![
            Point::new(0.0, 0.0, 0.0),
            Point::new(1.0, 0.0, 0.0),
            Point::new(0.0, 1.0, 0.0),
            Point::new(0.0, 0.0, 1.0),
        ];
        let indices = vec![
            [0u32, 1u32, 2u32],
            [0u32, 2u32, 3u32],
            [0u32, 3u32, 1u32],
            [1u32, 3u32, 2u32],
        ];
        let mut mesh = TriMesh::new(vertices, indices);
        mesh.set_flags(
            TriMeshFlags::ORIENTED
                | TriMeshFlags::FIX_INTERNAL_EDGES
                | TriMeshFlags::DELETE_DEGENERATE_TRIANGLES,
        )
        .unwrap();

        let isometry = Isometry::identity();
        let size = CoreSize::XS;
        let core_type = CoreType::Static;
        let height = size.height() - 3;
        let aabb = mesh.aabb(&isometry);
        let extents = Vector::repeat(4.0 * (1 << height) as f64);
        let svo_aabb = Aabb::from_half_extents(aabb.center(), extents);
        let voxelizer = Voxelizer::new(isometry, mesh);
        let svo = voxelizer.create_lods(&svo_aabb, Point::origin(), height, 1971262921);

        let mut occupancy = HashMap::new();
        svo.cata(|range, value, _| {
            let extent = range.size.x;
            let lod_height = extent.trailing_zeros() as u8;
            let coords = range.origin.map(|v| v / extent);
            occupancy.insert(
                (lod_height, coords.x, coords.y, coords.z),
                value.as_ref().map(|v| v.grid.is_empty()).unwrap_or(true),
            );
        });

        let blueprint = Blueprint::new(
            "test".to_string(),
            CoreInfo::from(size, core_type),
            1971262921,
            svo,
        );
        let json = blueprint.to_construct_json();
        let json_string = serde_json::to_string(&json).unwrap();
        let loaded = BlueprintLoader::load_from_reader(json_string.as_bytes()).unwrap();
        let expected_bounds = (
            [
                json["Model"]["Bounds"]["min"]["x"].as_f64().unwrap(),
                json["Model"]["Bounds"]["min"]["y"].as_f64().unwrap(),
                json["Model"]["Bounds"]["min"]["z"].as_f64().unwrap(),
            ],
            [
                json["Model"]["Bounds"]["max"]["x"].as_f64().unwrap(),
                json["Model"]["Bounds"]["max"]["y"].as_f64().unwrap(),
                json["Model"]["Bounds"]["max"]["z"].as_f64().unwrap(),
            ],
        );

        (loaded, occupancy, expected_bounds)
    }

    #[test]
    fn round_trip_loader_matches_generated_voxels() {
        let (loaded, occupancy, (min_bounds, max_bounds)) = build_loader_fixture();

        assert_eq!(loaded.model.bounds.min, min_bounds);
        assert_eq!(loaded.model.bounds.max, max_bounds);

        for (height, level) in &loaded.lods {
            for chunk in &level.chunks {
                let key = (*height, chunk.coords.x, chunk.coords.y, chunk.coords.z);
                let expected = occupancy.get(&key).copied().unwrap_or(true);
                assert_eq!(chunk.voxels.grid.is_empty(), expected);
            }
        }
    }

    #[test]
    fn extract_voxel_geometry_matches_grid_positions() {
        let (loaded, _, _) = build_loader_fixture();
        let geometry = loaded.geometry();

        for height in loaded.lods.keys() {
            let level = loaded
                .lods
                .get(height)
                .expect("missing level for geometry comparison");
            let lod_geometry = geometry.get(height).expect("missing geometry for lod");
            assert_eq!(lod_geometry.level, *height);

            let mut expected_positions: Vec<[i32; 3]> = Vec::new();
            let mut expected_counts: BTreeMap<MaterialId, usize> = BTreeMap::new();

            for chunk in &level.chunks {
                chunk
                    .voxels
                    .grid
                    .for_each_filled_cell(|position, vertex_material| {
                        expected_positions.push([position.x, position.y, position.z]);
                        if let Some(material_id) = chunk
                            .voxels
                            .material_mapper()
                            .resolve(vertex_material.material)
                        {
                            *expected_counts.entry(material_id.clone()).or_insert(0) += 1;
                        }
                    });
            }

            let voxel_count = expected_positions.len();
            assert_eq!(lod_geometry.voxel_count, voxel_count);
            assert_eq!(expected_counts, lod_geometry.material_counts);

            let mut occupancy = HashSet::new();
            for position in &expected_positions {
                occupancy.insert((position[0], position[1], position[2]));
            }

            let mut expected_faces = 0usize;
            let neighbors = [
                (1, 0, 0),
                (-1, 0, 0),
                (0, 1, 0),
                (0, -1, 0),
                (0, 0, 1),
                (0, 0, -1),
            ];
            for position in &expected_positions {
                for neighbor in neighbors {
                    let adjacent = (
                        position[0] + neighbor.0,
                        position[1] + neighbor.1,
                        position[2] + neighbor.2,
                    );
                    if !occupancy.contains(&adjacent) {
                        expected_faces += 1;
                    }
                }
            }

            let mesh = &lod_geometry.mesh;
            assert_eq!(mesh.positions.len() % 3, 0);
            assert_eq!(mesh.normals.len(), mesh.positions.len());
            assert_eq!(mesh.colors.len(), mesh.positions.len());
            assert_eq!(mesh.indices.len() % 3, 0);

            let expected_triangles = expected_faces * 2;
            assert_eq!(mesh.indices.len() / 3, expected_triangles);
        }
    }

    #[test]
    fn surface_normals_point_outward() {
        let (loaded, _, _) = build_loader_fixture();
        let geometry = loaded.geometry();

        for height in loaded.lods.keys() {
            let lod_geometry = geometry.get(height).expect("missing geometry for lod");
            let mesh = &lod_geometry.mesh;

            assert_eq!(mesh.normals.len(), mesh.positions.len());
            assert_eq!(mesh.colors.len(), mesh.positions.len());

            let mut min_z = f32::MAX;
            let mut max_z = f32::MIN;
            for position in mesh.positions.chunks_exact(3) {
                min_z = min_z.min(position[2]);
                max_z = max_z.max(position[2]);
            }

            if !max_z.is_finite() || !min_z.is_finite() {
                continue;
            }

            let epsilon = ((max_z - min_z).abs() * 0.001).max(1e-5);

            let mut top_colors = Vec::new();
            let mut bottom_colors = Vec::new();
            for color in &mesh.colors {
                assert!(
                    (0.0..=1.0).contains(color),
                    "vertex color intensity outside expected range: {}",
                    color
                );
            }

            for (vertex_index, (position, normal)) in mesh
                .positions
                .chunks_exact(3)
                .zip(mesh.normals.chunks_exact(3))
                .enumerate()
            {
                let color = &mesh.colors[vertex_index * 3..vertex_index * 3 + 3];
                let average_color = (color[0] + color[1] + color[2]) / 3.0;
                if (position[2] - max_z).abs() <= epsilon {
                    assert!(
                        normal[2] >= -1e-5,
                        "top vertex normal points inward: {}",
                        normal[2]
                    );
                    top_colors.push(average_color);
                }

                if (position[2] - min_z).abs() <= epsilon {
                    assert!(
                        normal[2] <= 1e-5,
                        "bottom vertex normal points inward: {}",
                        normal[2]
                    );
                    bottom_colors.push(average_color);
                }
            }

            if !top_colors.is_empty() && !bottom_colors.is_empty() {
                let top_avg: f32 = top_colors.iter().sum::<f32>() / top_colors.len() as f32;
                let bottom_avg: f32 =
                    bottom_colors.iter().sum::<f32>() / bottom_colors.len() as f32;
                assert!(
                    top_avg >= bottom_avg,
                    "baked lighting should brighten upward-facing surfaces"
                );
            }
        }
    }
}
