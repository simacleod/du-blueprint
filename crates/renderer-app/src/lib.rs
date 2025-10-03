use std::path::PathBuf;

use anyhow::Context;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use bytemuck::cast_slice;
use du_blueprint::renderer::loader::{
    LodGeometry, MeshBuffers, ModelSection, RenderableBlueprint, Renderer,
};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::Serialize;
use tauri::State;

static RENDERER: Lazy<Renderer> = Lazy::new(Renderer::new);

#[derive(Default)]
struct RendererAppState {
    initial_blueprint: RwLock<Option<PathBuf>>,
}

impl RendererAppState {
    fn set_initial(&self, path: Option<PathBuf>) {
        *self.initial_blueprint.write() = path;
    }

    fn initial(&self) -> Option<String> {
        self.initial_blueprint
            .read()
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
    }
}

#[derive(Serialize)]
struct BlueprintResponse {
    model: ModelResponse,
    lods: Vec<LodResponse>,
}

#[derive(Serialize)]
struct ModelResponse {
    name: String,
    size: usize,
    bounds: BoundsResponse,
}

#[derive(Serialize)]
struct BoundsResponse {
    min: [f64; 3],
    max: [f64; 3],
}

#[derive(Serialize)]
struct LodResponse {
    level: u8,
    #[serde(rename = "voxelSize")]
    voxel_size: f32,
    #[serde(rename = "voxelCount")]
    voxel_count: usize,
    mesh: MeshResponse,
}

#[derive(Serialize)]
struct MeshResponse {
    positions: String,
    normals: String,
    colors: String,
    indices: String,
}

impl From<RenderableBlueprint> for BlueprintResponse {
    fn from(value: RenderableBlueprint) -> Self {
        BlueprintResponse {
            model: ModelResponse::from(value.model),
            lods: value.lods.into_iter().map(LodResponse::from).collect(),
        }
    }
}

impl From<ModelSection> for ModelResponse {
    fn from(value: ModelSection) -> Self {
        ModelResponse {
            name: value.name,
            size: value.size,
            bounds: BoundsResponse {
                min: value.bounds.min,
                max: value.bounds.max,
            },
        }
    }
}

impl From<LodGeometry> for LodResponse {
    fn from(value: LodGeometry) -> Self {
        LodResponse {
            level: value.level,
            voxel_size: value.voxel_size,
            voxel_count: value.voxel_count,
            mesh: MeshResponse::from(value.mesh),
        }
    }
}

impl From<MeshBuffers> for MeshResponse {
    fn from(value: MeshBuffers) -> Self {
        let MeshBuffers {
            positions,
            normals,
            colors,
            indices,
        } = value;
        MeshResponse {
            positions: encode_f32_buffer(&positions),
            normals: encode_f32_buffer(&normals),
            colors: encode_f32_buffer(&colors),
            indices: encode_u32_buffer(&indices),
        }
    }
}

#[tauri::command]
async fn load_blueprint(
    path: String,
    state: State<'_, RendererAppState>,
) -> Result<BlueprintResponse, String> {
    let renderer = RENDERER.clone();
    let blueprint_path = PathBuf::from(path.clone());

    let result = tauri::async_runtime::spawn_blocking(move || {
        renderer
            .load_blueprint_from_path(&blueprint_path)
            .map(BlueprintResponse::from)
    })
    .await
    .map_err(|err| format!("failed to join renderer task: {}", err))?;

    let response = result.map_err(|err| err.to_string())?;
    state.set_initial(Some(PathBuf::from(path)));

    Ok(response)
}

#[tauri::command]
fn initial_blueprint(state: State<'_, RendererAppState>) -> Option<String> {
    state.initial()
}

pub fn run(initial_path: Option<PathBuf>) -> anyhow::Result<()> {
    let mut state = RendererAppState::default();
    state.set_initial(initial_path);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![load_blueprint, initial_blueprint])
        .run(tauri::generate_context!())
        .with_context(|| "failed to run renderer app")
}

fn encode_f32_buffer(values: &[f32]) -> String {
    BASE64_STANDARD.encode(cast_slice(values))
}

fn encode_u32_buffer(values: &[u32]) -> String {
    BASE64_STANDARD.encode(cast_slice(values))
}
