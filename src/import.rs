use crate::squarion::*;
use crate::svo::*;
use parry3d_f64::math::{Point, Vector};
use serde_json::{Value};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;

pub struct JSONImporter;

impl JSONImporter {
    fn set_at_all_lods<F>(
        &mut self,
        svo: &mut Svo<Option<VoxelCellData>>,
        global_position: Point<i32>,
        current_depth: usize,
        scale_factor: i32,
        set_fn: F,
    ) where
        F: Fn(&mut VoxelCellData, Point<i32>, i32),
    {
        fn traverse_svo<F>(
            node: &mut SvoNode<Option<VoxelCellData>>,
            range: &RangeZYX,
            global_position: Point<i32>,
            current_depth: usize,
            scale_factor: i32,
            set_fn: &F,
        ) where
            F: Fn(&mut VoxelCellData, Point<i32>, i32),
        {
            let padding = scale_factor;
            let padded_range = RangeZYX {
                origin: (range.origin - Point::new(padding, padding, padding)).into(),
                size: range.size + Vector::new(2 * padding, 2 * padding, 2 * padding),
            };

            if !padded_range.contains_point(global_position) {
                return;
            }

            let within_lod = global_position
                .coords
                .iter()
                .all(|&coord| coord % scale_factor == 0);

            if within_lod {
                match node {
                    SvoNode::Leaf(Some(cell_data)) => {
                        set_fn(cell_data, global_position, scale_factor);
                    }
                    SvoNode::Internal(Some(cell_data), _) => {
                        set_fn(cell_data, global_position, scale_factor);
                    }
                    _ => {}
                }
            }

            if let SvoNode::Internal(_, children) = node {
                let next_scale_factor = scale_factor / 2;
                let octants = range.split_at_center();
                for (i, child_range) in octants.iter().enumerate() {
                    traverse_svo(
                        &mut children[i],
                        child_range,
                        global_position,
                        current_depth + 1,
                        next_scale_factor,
                        set_fn,
                    );
                }
            }
        }

        traverse_svo(
            &mut svo.root,
            &svo.range,
            global_position,
            current_depth,
            scale_factor,
            &set_fn,
        );
    }

    pub fn set_material_at_all_lods(
        &mut self,
        svo: &mut Svo<Option<VoxelCellData>>,
        global_position: Point<i32>,
        material: u8,
        height: usize,
    ) {
        let initial_scale_factor = 1 << (height - 3);

        self.set_at_all_lods(svo, global_position, 0, initial_scale_factor, |cell_data, pos, scale| {
            cell_data.set_material_at_position(pos, material);

            for dx in 0..=1 {
                for dy in 0..=1 {
                    for dz in 0..=1 {
                        let corner_position = Point::new(
                            pos.x - dx,
                            pos.y - dy,
                            pos.z - dz,
                        );
                        cell_data.set_vertex_offset_at_position(corner_position, [126, 126, 126]);
                    }
                }
            }
        });
    }

    pub fn set_vertex_offset_at_all_lods(
        &mut self,
        svo: &mut Svo<Option<VoxelCellData>>,
        global_position: Point<i32>,
        offset: Point<u8>,
        height: usize,
    ) {
        let initial_scale_factor = 1 << (height - 3);

        self.set_at_all_lods(svo, global_position, 0, initial_scale_factor, |cell_data, pos, scale| {
            cell_data.set_vertex_offset_at_position(pos, offset.into());
        });
    }

    pub fn process_json_and_create_svo(
        &mut self,
        json_data: &Value,
        height: usize,
    ) -> Svo<Option<VoxelCellData>> {
        let origin = Point::new(0, 0, 0);

        // Extract materials mapping
        let materials_json = json_data["materials"].as_object().expect("Invalid 'materials' mapping");

        // Collect material IDs
        let material_ids: Vec<u64> = materials_json
            .keys()
            .map(|k| k.parse::<u64>().expect("Invalid material ID"))
            .collect();

        // Build mapping from material IDs to indices
        let mut material_id_to_index: HashMap<u64, u8> = HashMap::new();

        // Build MaterialMapper
        let mut material_mapper = MaterialMapper::default();

        // Insert the debug material with index 1
        material_mapper.insert(
            1,
            MaterialId {
                id: 157903047,
                short_name: "Debug1\0\0".into(),
            },
        );

        // Start material indices from 2 to avoid conflict with debug material
        let mut material_index = 2;

        for material_id in &material_ids {
            let short_name = format!("Mat{:05}", material_index); 
            material_mapper.insert(
                material_index,
                MaterialId {
                    id: *material_id,
                    short_name: short_name.into(),
                },
            );
            material_id_to_index.insert(*material_id, material_index);
            material_index += 1;
        }

        // Create empty SVO with the material mapper
        let mut svo = self.create_empty_lods(origin, height, &material_mapper);

        // Process positions for each material
        for (material_id_str, positions_json) in materials_json.iter() {
            let material_id = material_id_str.parse::<u64>().expect("Invalid material ID");
            let positions = positions_json.as_array().expect("Invalid positions array");
            let material_index = *material_id_to_index.get(&material_id).expect("Material ID not found in mapping");

            // Create a progress bar for positions
            let position_bar = ProgressBar::new(positions.len() as u64);
            position_bar.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} ({eta})")
                    .expect("Failed to set progress bar template")
                    .progress_chars("#>-"),
            );

            // Iterate over positions with progress bar
            for pos in positions {
                let global_position = Point::new(
                    (pos[0].as_f64().unwrap() + 0.5).round() as i32,
                    (pos[1].as_f64().unwrap() + 0.5).round() as i32,
                    (pos[2].as_f64().unwrap() + 0.5).round() as i32,
                );
                self.set_material_at_all_lods(&mut svo, global_position, material_index, height);
                position_bar.inc(1);
            }
            position_bar.finish_with_message(format!("Positions for material {} processed", material_id));
        }

        // Process vertices
        let vertices = json_data["vertices"].as_array().expect("Invalid 'vertices' array");

        // Create a progress bar for vertices
        let vertex_bar = ProgressBar::new(vertices.len() as u64);
        vertex_bar.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.magenta/red}] {pos}/{len} ({eta})")
                .expect("Failed to set progress bar template")
                .progress_chars("#>-"),
        );

        // Iterate over vertices with progress bar
        for vert in vertices {
            let x = vert[0].as_f64().unwrap_or_else(|| vert[0].as_i64().unwrap() as f64);
            let y = vert[1].as_f64().unwrap_or_else(|| vert[1].as_i64().unwrap() as f64);
            let z = vert[2].as_f64().unwrap_or_else(|| vert[2].as_i64().unwrap() as f64);
            let global_position = Point::new(x as i32, y as i32, z as i32);

            let offset_x = vert[3].as_f64().unwrap_or_else(|| vert[3].as_i64().unwrap() as f64) as u8;
            let offset_y = vert[4].as_f64().unwrap_or_else(|| vert[4].as_i64().unwrap() as f64) as u8;
            let offset_z = vert[5].as_f64().unwrap_or_else(|| vert[5].as_i64().unwrap() as f64) as u8;
            let offset = Point::new(offset_x, offset_y, offset_z);

            self.set_vertex_offset_at_all_lods(&mut svo, global_position, offset, height);
            vertex_bar.inc(1);
        }
        vertex_bar.finish_with_message("Vertices processed");

        // Adjust the root range
        let scale_factor = 32;
        svo.range = RangeZYX {
            origin: svo.range.origin / scale_factor,
            size: Vector::new(
                svo.range.size.x / scale_factor,
                svo.range.size.y / scale_factor,
                svo.range.size.z / scale_factor,
            ),
        };

        let pruned_svo = svo.prune_empty_grids();
        pruned_svo
    }

    pub fn create_empty_lods(
        &self,
        origin: Point<i32>,
        height: usize,
        material_mapper: &MaterialMapper,
    ) -> Svo<Option<VoxelCellData>> {
        let core_size = 128 * (1 << (height - 5));
        let leaf_size = 32;
        println!("Creating empty LODs with core size: {} and leaf size: {}", core_size, leaf_size);

        // Recursive function to build the SVO nodes
        fn build_svo_node(
            range: &RangeZYX,
            leaf_size: i32,
            depth: usize,
            max_depth: usize,
            material_mapper: &MaterialMapper,
        ) -> SvoNode<Option<VoxelCellData>> {

            if range.size.x <= leaf_size || depth >= max_depth {
                let outer_range = RangeZYX::with_extent(range.origin - Vector::repeat(1), 35);
                let inner_range = RangeZYX::with_extent(range.origin, leaf_size);
                let grid = VertexGrid::new(outer_range.clone(), inner_range.clone());

                println!(
                    "Creating leaf node at depth {} with range origin = {:?}, size = {:?}",
                    depth, range.origin, range.size
                );

                let voxel_cell_data = VoxelCellData::new(grid, material_mapper.clone());
                SvoNode::Leaf(Some(voxel_cell_data))
            } else {
                println!(
                    "Creating internal node at depth {} with range origin = {:?}, size = {:?}",
                    depth, range.origin, range.size
                );

                let outer_range = RangeZYX::with_extent(range.origin - Vector::repeat(1), 35);
                let inner_range = RangeZYX::with_extent(range.origin, leaf_size);
                let grid = VertexGrid::new(outer_range.clone(), inner_range.clone());

                let voxel_cell_data = VoxelCellData::new(grid, material_mapper.clone());

                let children = Box::new(range.split_at_center().map(|sub_range| {
                    build_svo_node(&sub_range, leaf_size, depth + 1, max_depth, material_mapper)
                }));

                SvoNode::Internal(Some(voxel_cell_data), children)
            }
        }

        let root_range = RangeZYX::with_extent(origin, core_size as i32);
        let root_node = build_svo_node(&root_range, leaf_size, 0, height - 3, material_mapper);
        println!("Created root node at depth 0 with range origin = {:?}, size = {:?}", root_range.origin, root_range.size);
        Svo { root: root_node, range: root_range }
    }
}
