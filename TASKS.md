# Offline Blueprint Renderer Implementation Plan

This plan is derived from the current Rust code in `du-blueprint`. It outlines the concrete work needed to ship a fast offline `.blueprint` renderer packaged with Tauri and using a Three.js frontend. Geometry is rendered in neutral grey (no material tinting) and the UI exposes a slider for switching LODs.

## 1. Blueprint decoding core (Rust)
- [x] Add a new Rust module (`src/renderer/loader.rs`) that owns blueprint ingestion. Reuse the existing `serde_json` dependency to read the JSON emitted by `Blueprint::to_construct_json` in `src/blueprint.rs`.
- [x] For each entry under `VoxelData`, decode `records.voxel.data` and `records.meta.data` with `BASE64_STANDARD` and feed the bytes into `VoxelCellData::decompress` / `AggregateMetadata::decompress` from `src/squarion.rs`. Persist both results alongside `height` and `coords`.
- [x] Reconstruct the spatial tree that `make_voxel_data` generated: compute the cell extent as `1 << height`, recover the octant origin via `Point::from(coords) * extent`, and rebuild an `SvoNode<Option<VoxelCellData>>` hierarchy by inserting each decoded chunk into the correct branch using `RangeZYX::split_at_center()` and the existing `Svo` helpers in `src/svo.rs`.
- [x] While rebuilding, track per-level collections keyed by `height` so the renderer can retrieve all chunks that belong to a single LOD in O(1). Cache the corresponding `AggregateMetadata::heavy_current.bounding_box` (converted back into world units by dividing by `4.0`, matching `Blueprint::to_construct_json`).
- [x] Add Rust unit tests that round-trip: generate a blueprint with `Commands::Generate` in `src/main.rs`, load it with the new module, and assert that every decoded chunk’s `VertexGrid::is_empty()` matches the original SVO occupancy and that the recomputed bounding box matches `Blueprint::to_construct_json`.

## 2. Geometry extraction for rendering
- [x] Implement a helper (`build_lod_geometry`) that, given a decoded chunk, iterates its `VertexGrid::inner_range` using `RangeZYX::for_each_index_range` to find occupied cells in `sparse_materials`. Use the stored vertex offsets to stitch adjacent voxels into a single watertight triangle mesh per LOD instead of instancing cubes, deduplicating shared faces across chunks.
- [x] Because everything is grey, normalize all voxels to a single `material_id` before sending them to the UI. Optionally keep counts for HUD display by reusing the `MaterialMapper` already attached to each `VoxelCellData`.
- [x] When extracting geometry for a requested LOD, rely exclusively on the decoded chunks for that `height`. Do not synthesize parent data—if a blueprint omits a level, surface the gap to the UI so the renderer reflects the blueprint exactly as stored.

## 3. Tauri backend wiring
- [x] Promote the crate to a workspace with a new Tauri binary (e.g., `crates/renderer-app`). Expose the shared decoding/geometry code above through a library crate so both the CLI and Tauri command reuse it.
- [x] Define a `#[tauri::command] fn load_blueprint(path: String)` that spawns the decode work on a background thread, calls the helpers above, and returns a JSON payload shaped as `{ model: {...}, lods: [{level, voxelSize, voxels: [...]}, ...] }`. Populate `model` with fields already exported by `Blueprint::to_construct_json` (`Model.Name`, `Model.Size`, `Model.Bounds`).
- [x] Make sure repeated slider changes do not recompute decoding: maintain an `Arc<RwLock<HashMap<(u8, (i32,i32,i32)), Arc<ChunkGeometry>>>>` cache keyed by LOD level and chunk coordinates so geometry is built once per session.
- [x] Rework the Clap parser in `src/main.rs` so `du-blueprint --render <INPUT BLUEPRINT FILE>` becomes the entrypoint for the renderer: add a top-level `#[arg(long)] render: Option<PathBuf>` (or equivalent) that, when set, launches the Tauri binary with the provided path and skips the existing `Commands` dispatch.

## 4. Three.js frontend implementation
- [x] Scaffold the Tauri webview with Vite (`pnpm create vite renderer-ui --template react-ts`) and add Three.js plus `three/examples/jsm/controls/OrbitControls.js`.
- [x] Build the scene graph: create a `PerspectiveCamera`, `OrbitControls`, hemispheric + directional lights, and a reusable `MeshStandardMaterial` set to a neutral grey (`#9e9e9e`).
- [x] Implement a store (e.g., Zustand or vanilla React context) that holds the `lods` array returned by the backend. When the user selects an LOD, instantiate a `THREE.InstancedMesh` sized to that LOD’s voxel count and fill transforms from the `{position, size}` data.
- [x] Add a UI panel with the LOD slider, voxel counts, blueprint metadata, and a toggle to show/hide the `Model.Bounds` as a wireframe box using `LineSegments`.
- [x] Ensure that switching LODs disposes of old geometries/materials (`geometry.dispose()`, `mesh.removeFromParent()`) to avoid GPU leaks when sweeping the slider.

## 5. Testing and validation
- [ ] Write Rust integration tests that create a temporary blueprint via `Commands::Generate`, pass it through the loader, and assert that aggregating the returned voxel cubes reproduces the original occupied cell count at each LOD.
- [ ] Add a Playwright (or Vitest + Playwright) test in the frontend workspace that boots the Tauri app against a fixture blueprint, drives the LOD slider, and asserts that the WebGL canvas stays alive (no console errors, meshes exist for each level).
- [ ] Update `README.md` with end-to-end setup instructions: Rust + Tauri prerequisites, CLI command for generating fixture blueprints, how to start the Tauri dev server, and how to run the automated tests.

