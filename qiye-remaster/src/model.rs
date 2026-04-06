/// SOJ 3D model parser.
///
/// SOJ binary format (no magic/version header, flat sequential):
///   i32 submesh_count
///   [SubMeshEntry; submesh_count]  -- 52 bytes each (material slots)
///   i32 material_count
///   i32 field_b8
///   i32 field_bc
///   [MaterialGroup; material_count]  -- variable size (geometry data)
///   i32[6] bbox (min_xyz, max_xyz)  -- 16.16 fixed-point
///
/// Each MaterialGroup contains inline VertexBuffer + IndexBuffer.
/// All numeric data is 16.16 fixed-point (i32), little-endian.

use crate::packed_string;

fn read_i16_le(d: &[u8], o: usize) -> i16 {
    i16::from_le_bytes([d[o], d[o + 1]])
}

fn read_u16_le(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}

fn read_i32_le(d: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

fn read_u32_le(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

fn fp_to_f32(v: i32) -> f32 {
    v as f32 / 65536.0
}

/// Material slot from the SubMeshEntry array.
#[derive(Debug, Clone)]
pub struct SojMaterial {
    pub texture_name: String,
    pub color: [u8; 4], // RGBA
    pub render_mode: u8, // 0=opaque, 3=alpha_blend, 4=additive
    pub draw_order: u16,
    pub scroll_u: f32,
    pub scroll_v: f32,
}

/// A single vertex with position, optional normal, UV, color, bone.
#[derive(Debug, Clone, Copy, Default)]
pub struct SojVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub color: u32,
    pub bone_index: i32,
}

/// Geometry group referencing a material slot.
#[derive(Debug, Clone)]
pub struct SojMesh {
    pub material_idx: i32, // index into SojMaterial array (-1 = none)
    pub primitive_type: i32, // 4=tri_list, 5=tri_strip
    pub vertices: Vec<SojVertex>,
    pub indices: Vec<u16>,
}

/// Parsed SOJ model.
#[derive(Debug)]
pub struct SojModel {
    pub materials: Vec<SojMaterial>,
    pub meshes: Vec<SojMesh>,
    pub bbox_min: [f32; 3],
    pub bbox_max: [f32; 3],
}

impl SojModel {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }

        let mut off = 0;

        // SubMeshEntry array (material slots)
        let submesh_count = read_i32_le(data, off) as usize;
        off += 4;

        if submesh_count > 256 || off + submesh_count * 52 > data.len() {
            eprintln!("SOJ: bad submesh_count {submesh_count}");
            return None;
        }

        let mut materials = Vec::with_capacity(submesh_count);
        for _ in 0..submesh_count {
            let packed = [
                read_u32_le(data, off + 0x14),
                read_u32_le(data, off + 0x18),
                read_u32_le(data, off + 0x1c),
                read_u32_le(data, off + 0x20),
            ];
            let texture_name = packed_string::decode(&packed);

            let color_r = data[off + 0x08];
            let color_g = data[off + 0x09];
            let color_b = data[off + 0x0a];
            let alpha = data[off + 0x0b];

            let draw_order = read_u16_le(data, off + 0x10);
            let scroll_u = fp_to_f32(read_i32_le(data, off + 0x24));
            let scroll_v = fp_to_f32(read_i32_le(data, off + 0x28));

            let has_alpha_blend = read_i32_le(data, off + 0x2c);
            let has_additive = read_i32_le(data, off + 0x30);
            let render_mode = if has_alpha_blend != 0 {
                3
            } else if has_additive != 0 {
                4
            } else {
                0
            };

            materials.push(SojMaterial {
                texture_name,
                color: [color_r, color_g, color_b, alpha],
                render_mode,
                draw_order,
                scroll_u,
                scroll_v,
            });

            off += 52;
        }

        // Material group count + extra fields
        if off + 12 > data.len() {
            return None;
        }
        let material_count = read_i32_le(data, off) as usize;
        let _field_b8 = read_i32_le(data, off + 4);
        let _field_bc = read_i32_le(data, off + 8);
        off += 12;

        if material_count > 256 {
            eprintln!("SOJ: bad material_count {material_count}");
            return None;
        }

        // Parse MaterialGroups (each has inline VertexBuffer + IndexBuffer)
        let mut meshes = Vec::with_capacity(material_count);
        for _ in 0..material_count {
            if off + 12 > data.len() {
                break;
            }

            let submesh_index = read_i32_le(data, off);
            let primitive_type = read_i32_le(data, off + 4);
            let face_count = read_i32_le(data, off + 8) as usize;
            off += 12;

            // Parse VertexBuffer
            if off + 16 > data.len() {
                break;
            }
            let format_flags = read_i32_le(data, off) as u32;
            let stride_bytes = read_i32_le(data, off + 4) as usize;
            let vertex_count = read_i32_le(data, off + 8) as usize;
            let vb_data_size = read_i32_le(data, off + 12) as usize;
            off += 16;

            if off + vb_data_size > data.len() || vertex_count > 100000 {
                break;
            }

            let stride_i32 = stride_bytes / 4;
            let has_bone = format_flags & 0x10 != 0;
            let has_normal = format_flags & 0x02 != 0;
            let has_uv = format_flags & 0x04 != 0;
            let has_color = format_flags & 0x08 != 0;

            let mut vertices = Vec::with_capacity(vertex_count);
            for v in 0..vertex_count {
                let voff = off + v * stride_bytes;
                let mut field = 0usize;
                let mut vert = SojVertex::default();

                if has_bone {
                    vert.bone_index = read_i32_le(data, voff + field * 4);
                    field += 1;
                }

                // Position (always present)
                vert.pos = [
                    fp_to_f32(read_i32_le(data, voff + field * 4)),
                    fp_to_f32(read_i32_le(data, voff + (field + 1) * 4)),
                    fp_to_f32(read_i32_le(data, voff + (field + 2) * 4)),
                ];
                field += 3;

                if has_normal {
                    vert.normal = [
                        fp_to_f32(read_i32_le(data, voff + field * 4)),
                        fp_to_f32(read_i32_le(data, voff + (field + 1) * 4)),
                        fp_to_f32(read_i32_le(data, voff + (field + 2) * 4)),
                    ];
                    field += 3;
                }

                if has_uv {
                    vert.uv = [
                        fp_to_f32(read_i32_le(data, voff + field * 4)),
                        fp_to_f32(read_i32_le(data, voff + (field + 1) * 4)),
                    ];
                    field += 2;
                }

                if has_color {
                    vert.color = read_u32_le(data, voff + field * 4);
                    // field += 1; -- not needed, last field
                }

                vertices.push(vert);
            }
            off += vb_data_size;

            // Parse IndexBuffer
            if off + 8 > data.len() {
                break;
            }
            let _ib_format = read_i32_le(data, off);
            let ib_data_size = read_i32_le(data, off + 4) as usize;
            off += 8;

            if off + ib_data_size > data.len() {
                break;
            }

            // Compute expected index count
            let expected_indices = match primitive_type {
                4 => face_count * 3,         // triangle list
                5 => face_count + 2,         // triangle strip
                _ => ib_data_size / 2,       // fallback
            };
            let index_count = expected_indices.min(ib_data_size / 2);

            let mut indices = Vec::with_capacity(index_count);
            for i in 0..index_count {
                indices.push(read_u16_le(data, off + i * 2));
            }
            off += ib_data_size;

            // Validate stride
            let expected_fields = 3 // position
                + if has_bone { 1 } else { 0 }
                + if has_normal { 3 } else { 0 }
                + if has_uv { 2 } else { 0 }
                + if has_color { 1 } else { 0 };

            if stride_i32 != expected_fields && stride_i32 != 0 {
                eprintln!(
                    "SOJ: stride mismatch: stride_i32={stride_i32} expected={expected_fields} flags=0x{format_flags:x}"
                );
            }

            meshes.push(SojMesh {
                material_idx: submesh_index,
                primitive_type,
                vertices,
                indices,
            });
        }

        // Parse bounding box (6 × i32, 16.16 fixed-point)
        let mut bbox_min = [0.0f32; 3];
        let mut bbox_max = [0.0f32; 3];
        if off + 24 <= data.len() {
            for i in 0..3 {
                bbox_min[i] = fp_to_f32(read_i32_le(data, off + i * 4));
            }
            for i in 0..3 {
                bbox_max[i] = fp_to_f32(read_i32_le(data, off + 12 + i * 4));
            }
        } else {
            // Compute from vertices
            bbox_min = [f32::MAX; 3];
            bbox_max = [f32::MIN; 3];
            for mesh in &meshes {
                for v in &mesh.vertices {
                    for i in 0..3 {
                        bbox_min[i] = bbox_min[i].min(v.pos[i]);
                        bbox_max[i] = bbox_max[i].max(v.pos[i]);
                    }
                }
            }
        }

        let total_verts: usize = meshes.iter().map(|m| m.vertices.len()).sum();
        let total_indices: usize = meshes.iter().map(|m| m.indices.len()).sum();
        println!(
            "SOJ: {} materials, {} meshes, {} verts, {} indices, bbox ({:.1},{:.1},{:.1})-({:.1},{:.1},{:.1})",
            materials.len(), meshes.len(), total_verts, total_indices,
            bbox_min[0], bbox_min[1], bbox_min[2],
            bbox_max[0], bbox_max[1], bbox_max[2],
        );

        Some(SojModel {
            materials,
            meshes,
            bbox_min,
            bbox_max,
        })
    }

    /// Convert triangle strips to triangle lists for easier GL rendering.
    pub fn triangulate(&self) -> Vec<(i32, Vec<SojVertex>, Vec<u16>)> {
        let mut result = Vec::new();
        for mesh in &self.meshes {
            match mesh.primitive_type {
                4 => {
                    // Already triangle list
                    result.push((mesh.material_idx, mesh.vertices.clone(), mesh.indices.clone()));
                }
                5 => {
                    // Convert strip to list
                    if mesh.indices.len() < 3 {
                        continue;
                    }
                    let mut tri_indices = Vec::new();
                    for i in 0..mesh.indices.len() - 2 {
                        let (a, b, c) = if i % 2 == 0 {
                            (mesh.indices[i], mesh.indices[i + 1], mesh.indices[i + 2])
                        } else {
                            (mesh.indices[i + 1], mesh.indices[i], mesh.indices[i + 2])
                        };
                        // Skip degenerate triangles
                        if a != b && b != c && a != c {
                            tri_indices.push(a);
                            tri_indices.push(b);
                            tri_indices.push(c);
                        }
                    }
                    result.push((mesh.material_idx, mesh.vertices.clone(), tri_indices));
                }
                _ => {}
            }
        }
        result
    }
}
