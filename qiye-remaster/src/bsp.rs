#![allow(dead_code)]

use crate::packed_string;

fn read_u16_le(data: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([data[off], data[off + 1]])
}

fn read_i16_le(data: &[u8], off: usize) -> i16 {
    i16::from_le_bytes([data[off], data[off + 1]])
}

fn read_u32_le(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn read_i32_le(data: &[u8], off: usize) -> i32 {
    i32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn read_f32_le(data: &[u8], off: usize) -> f32 {
    f32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

#[derive(Debug, Clone)]
pub struct BspVertex {
    pub pos: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct BspEdge {
    pub v: [u16; 2],
}

#[derive(Debug, Clone)]
pub struct BspPlane {
    pub normal: [f32; 3],
    pub dist: f32,
    pub type_flags: u32,
}

#[derive(Debug, Clone)]
pub struct BspTexinfo {
    pub s_vec: [f32; 4], // s axis xyz + offset
    pub t_vec: [f32; 4], // t axis xyz + offset
    pub texture_name: String,
}

#[derive(Debug, Clone)]
pub struct BspFace {
    pub plane_idx: i16,
    pub first_edge: u16,
    pub num_edges: u16,
    pub texinfo_idx: i16,
    pub lightmap_s: i16,
    pub lightmap_t: i16,
    pub lightmap_offset: i32,
    pub styles: [u8; 4],
}

// Placeholder structs for future PVS culling
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct BspNode {
    pub plane_idx: u32,
    pub children: [i32; 2],
    pub mins: [i16; 3],
    pub maxs: [i16; 3],
    pub first_face: u16,
    pub num_faces: u16,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct BspLeaf {
    pub cluster: i16,
    pub area: i16,
    pub mins: [i16; 3],
    pub maxs: [i16; 3],
    pub first_leaf_face: u16,
    pub num_leaf_faces: u16,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct BspModel {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
    pub origin: [f32; 3],
    pub head_node: i32,
    pub first_face: u32,
    pub num_faces: u32,
}

/// A camera spot from the BSP entity data (EPair lump).
#[derive(Debug, Clone)]
pub struct BspCameraSpot {
    pub pos: [f32; 3],
    pub dir: [f32; 3],
    pub dist: f32,
}

/// A raw entity from the BSP Ent lump.
/// 16 bytes per entry: type(i16), model_idx(i16), ebrush_start(u16), ebrush_count(i16),
///                      epair_start(u16), epair_count(i16), elink_idx(i16), reserved(i16)
#[derive(Debug, Clone)]
pub struct BspEntity {
    pub entity_type: i16,
    pub model_idx: i16,
    pub epair_start: u16,
    pub epair_count: u16,
    /// Parsed position from EPairs (if available)
    pub position: Option<[f32; 3]>,
    /// Parsed 3x3 transform matrix rows from EPairs (if available)
    pub transform: Option<[[f32; 3]; 3]>,
    /// Parsed model name from EPairs (if available)
    pub model_name: Option<String>,
    /// Parsed bounding box extents from EPairs (if available)
    pub bbox: Option<[f32; 3]>,
}

pub struct Bsp {
    pub bbox_min: [f32; 3],
    pub bbox_max: [f32; 3],
    pub vertices: Vec<BspVertex>,
    pub edges: Vec<BspEdge>,
    pub face_edges: Vec<i32>,
    pub planes: Vec<BspPlane>,
    pub texinfos: Vec<BspTexinfo>,
    pub faces: Vec<BspFace>,
    pub lightmap_data: Vec<u8>,
    pub camera_spots: Vec<BspCameraSpot>,
    pub entities: Vec<BspEntity>,
    pub epair_strings: Vec<String>,
}

struct LumpDir {
    offset: usize,
    size: usize,
}

/// Parse "(x,y,z)" string from BSP EPair data.
fn parse_vec3(s: &str) -> Option<[f32; 3]> {
    let s = s.trim().strip_prefix('(')?.strip_suffix(')')?;
    let mut parts = s.split(',');
    let x = parts.next()?.trim().parse::<f32>().ok()?;
    let y = parts.next()?.trim().parse::<f32>().ok()?;
    let z = parts.next()?.trim().parse::<f32>().ok()?;
    Some([x, y, z])
}

fn read_lump_dir(data: &[u8], header_off: usize) -> LumpDir {
    LumpDir {
        offset: read_u32_le(data, header_off) as usize,
        size: read_u32_le(data, header_off + 4) as usize,
    }
}

impl Bsp {
    pub fn parse(data: &[u8]) -> Self {
        // Validate version (7.6f = 0x40f33333)
        let version_bits = read_u32_le(data, 4);
        assert_eq!(
            version_bits, 0x40f33333,
            "Bad BSP version: 0x{version_bits:08x} (expected 7.6)"
        );

        // Header bounding box (unreliable — we compute from vertices below)
        let _bbox_min_hdr = [
            read_f32_le(data, 0x08),
            read_f32_le(data, 0x0c),
            read_f32_le(data, 0x10),
        ];
        let _bbox_max_hdr = [
            read_f32_le(data, 0x14),
            read_f32_le(data, 0x18),
            read_f32_le(data, 0x1c),
        ];

        // Lump directories
        let vertex_lump = read_lump_dir(data, 0x20);
        let edge_lump = read_lump_dir(data, 0x28);
        let face_edge_lump = read_lump_dir(data, 0x30);
        let plane_lump = read_lump_dir(data, 0x38);
        let texinfo_lump = read_lump_dir(data, 0x40);
        let face_lump = read_lump_dir(data, 0x48);
        let lightmap_lump = read_lump_dir(data, 0xf0);

        // Parse vertices (12 bytes each: 3 × f32)
        let vertex_count = vertex_lump.size / 12;
        let mut vertices = Vec::with_capacity(vertex_count);
        for i in 0..vertex_count {
            let off = vertex_lump.offset + i * 12;
            vertices.push(BspVertex {
                pos: [
                    read_f32_le(data, off),
                    read_f32_le(data, off + 4),
                    read_f32_le(data, off + 8),
                ],
            });
        }

        // Parse edges (4 bytes each: 2 × u16)
        let edge_count = edge_lump.size / 4;
        let mut edges = Vec::with_capacity(edge_count);
        for i in 0..edge_count {
            let off = edge_lump.offset + i * 4;
            edges.push(BspEdge {
                v: [read_u16_le(data, off), read_u16_le(data, off + 2)],
            });
        }

        // Parse face edges (4 bytes each: i32)
        let face_edge_count = face_edge_lump.size / 4;
        let mut face_edges = Vec::with_capacity(face_edge_count);
        for i in 0..face_edge_count {
            face_edges.push(read_i32_le(data, face_edge_lump.offset + i * 4));
        }

        // Parse planes (20 bytes each: 3×f32 + f32 + u32)
        let plane_count = plane_lump.size / 20;
        let mut planes = Vec::with_capacity(plane_count);
        for i in 0..plane_count {
            let off = plane_lump.offset + i * 20;
            planes.push(BspPlane {
                normal: [
                    read_f32_le(data, off),
                    read_f32_le(data, off + 4),
                    read_f32_le(data, off + 8),
                ],
                dist: read_f32_le(data, off + 12),
                type_flags: read_u32_le(data, off + 16),
            });
        }

        // Parse texinfos (48 bytes each: 8×f32 + 4×u32 packed string)
        let texinfo_count = texinfo_lump.size / 48;
        let mut texinfos = Vec::with_capacity(texinfo_count);
        for i in 0..texinfo_count {
            let off = texinfo_lump.offset + i * 48;
            let packed = [
                read_u32_le(data, off + 32),
                read_u32_le(data, off + 36),
                read_u32_le(data, off + 40),
                read_u32_le(data, off + 44),
            ];
            texinfos.push(BspTexinfo {
                s_vec: [
                    read_f32_le(data, off),
                    read_f32_le(data, off + 4),
                    read_f32_le(data, off + 8),
                    read_f32_le(data, off + 12),
                ],
                t_vec: [
                    read_f32_le(data, off + 16),
                    read_f32_le(data, off + 20),
                    read_f32_le(data, off + 24),
                    read_f32_le(data, off + 28),
                ],
                texture_name: packed_string::decode(&packed),
            });
        }

        // Parse faces (24 bytes each)
        // Layout from decompiled Bsp_LoadFace:
        //   i16[0]: plane_idx
        //   u16[1]: first_edge
        //   u16[2]: (unused)
        //   u16[3]: num_edges
        //   i16[4]: (unused)
        //   i16[5]: texinfo_idx
        //   i16[6]: lightmap_s
        //   i16[7]: lightmap_t
        //   i32 at byte 16: lightmap_offset
        //   u8[4] at byte 20: styles
        let face_count = face_lump.size / 24;
        let mut faces = Vec::with_capacity(face_count);
        for i in 0..face_count {
            let off = face_lump.offset + i * 24;
            faces.push(BspFace {
                plane_idx: read_i16_le(data, off),
                first_edge: read_u16_le(data, off + 2),
                num_edges: read_u16_le(data, off + 6),
                texinfo_idx: read_i16_le(data, off + 8),
                lightmap_s: read_i16_le(data, off + 12),
                lightmap_t: read_i16_le(data, off + 14),
                lightmap_offset: read_i32_le(data, off + 16),
                styles: [data[off + 20], data[off + 21], data[off + 22], data[off + 23]],
            });
        }

        // Lightmap data (raw bytes)
        let lightmap_data = data[lightmap_lump.offset..lightmap_lump.offset + lightmap_lump.size]
            .to_vec();

        // Compute actual bounds from vertices (header bbox is unreliable)
        let mut bbox_min = [f32::MAX; 3];
        let mut bbox_max = [f32::MIN; 3];
        for v in &vertices {
            for i in 0..3 {
                bbox_min[i] = bbox_min[i].min(v.pos[i]);
                bbox_max[i] = bbox_max[i].max(v.pos[i]);
            }
        }

        // Parse EPair lump (32-byte null-terminated strings)
        let epair_lump = read_lump_dir(data, 0x80);
        let ent_lump = read_lump_dir(data, 0x90);
        let mut epair_strings = Vec::new();
        if epair_lump.size > 0 {
            let count = epair_lump.size / 32;
            for i in 0..count {
                let off = epair_lump.offset + i * 32;
                let s = &data[off..off + 32];
                let nul = s.iter().position(|&b| b == 0).unwrap_or(32);
                epair_strings.push(String::from_utf8_lossy(&s[..nul]).into_owned());
            }
        }

        // Parse Ent lump (16 bytes each): type(i16), model_idx(i16),
        //   ebrush_start(u16), ebrush_count(i16), epair_start(u16), epair_count(i16),
        //   elink_idx(i16), reserved(i16)
        let mut camera_spots = Vec::new();
        let mut entities = Vec::new();
        if ent_lump.size > 0 {
            let ent_count = ent_lump.size / 16;
            for i in 0..ent_count {
                let off = ent_lump.offset + i * 16;
                let ent_type = read_i16_le(data, off);
                let model_idx = read_i16_le(data, off + 2);
                let epair_start = read_u16_le(data, off + 8);
                let epair_count = read_u16_le(data, off + 10);

                let ep_start = epair_start as usize;
                let ep_count = epair_count as usize;

                // Extract camera spots (type 10)
                if ent_type == 10 && ep_count >= 4 && ep_start + 3 < epair_strings.len() {
                    if let (Some(pos), Some(dir), Ok(dist)) = (
                        parse_vec3(&epair_strings[ep_start + 1]),
                        parse_vec3(&epair_strings[ep_start + 2]),
                        epair_strings[ep_start + 3].parse::<f32>(),
                    ) {
                        camera_spots.push(BspCameraSpot { pos, dir, dist });
                    }
                }

                // Parse entity fields from EPairs
                let mut position = None;
                let mut transform = None;
                let mut model_name = None;
                let mut bbox = None;

                if ep_count > 0 && ep_start < epair_strings.len() {
                    // Type 0 entities (scene objects): EPair layout is
                    //   [0]=model_name, [1]=null, [2-4]=flags,
                    //   [5-7]=transform 3x3, [8]=position, [9]=bbox, [10]=flag
                    if ent_type == 0 && ep_count >= 9 {
                        let name = &epair_strings[ep_start];
                        if !name.is_empty() && name != "0" {
                            model_name = Some(name.clone());
                        }
                        // Transform rows at EPair[5..8]
                        if ep_start + 7 < epair_strings.len() {
                            if let (Some(r0), Some(r1), Some(r2)) = (
                                parse_vec3(&epair_strings[ep_start + 5]),
                                parse_vec3(&epair_strings[ep_start + 6]),
                                parse_vec3(&epair_strings[ep_start + 7]),
                            ) {
                                transform = Some([r0, r1, r2]);
                            }
                        }
                        // Position at EPair[8]
                        if ep_start + 8 < epair_strings.len() {
                            position = parse_vec3(&epair_strings[ep_start + 8]);
                        }
                        // Bbox at EPair[9]
                        if ep_start + 9 < epair_strings.len() {
                            bbox = parse_vec3(&epair_strings[ep_start + 9]);
                        }
                    } else if ent_type == 10 && ep_count >= 2 {
                        // Camera spot: position is EPair[1]
                        if ep_start + 1 < epair_strings.len() {
                            position = parse_vec3(&epair_strings[ep_start + 1]);
                        }
                    }
                }

                entities.push(BspEntity {
                    entity_type: ent_type,
                    model_idx,
                    epair_start,
                    epair_count,
                    position,
                    transform,
                    model_name,
                    bbox,
                });
            }
        }

        // Print entity type summary
        let mut type_counts: std::collections::HashMap<i16, usize> = std::collections::HashMap::new();
        for ent in &entities {
            *type_counts.entry(ent.entity_type).or_default() += 1;
        }
        let mut type_list: Vec<_> = type_counts.into_iter().collect();
        type_list.sort_by_key(|&(t, _)| t);
        println!("BSP entity types:");
        for (t, c) in &type_list {
            println!("  type {t}: {c} entities");
        }

        println!(
            "BSP: {} verts, {} edges, {} face_edges, {} planes, {} texinfos, {} faces, {} bytes lightmap",
            vertices.len(),
            edges.len(),
            face_edges.len(),
            planes.len(),
            texinfos.len(),
            faces.len(),
            lightmap_data.len(),
        );

        // Print unique texture names
        let mut tex_names: Vec<&str> = texinfos.iter().map(|t| t.texture_name.as_str()).collect();
        tex_names.sort();
        tex_names.dedup();
        println!("BSP textures ({}):", tex_names.len());
        for name in &tex_names {
            println!("  {name}");
        }

        println!(
            "BSP entities: {} epairs, {} ents, {} camera spots",
            epair_strings.len(),
            if ent_lump.size > 0 { ent_lump.size / 16 } else { 0 },
            camera_spots.len(),
        );

        Bsp {
            bbox_min,
            bbox_max,
            vertices,
            edges,
            face_edges,
            planes,
            texinfos,
            faces,
            lightmap_data,
            camera_spots,
            entities,
            epair_strings,
        }
    }

    /// Get vertex indices for a face by walking face_edges.
    pub fn face_vertices(&self, face: &BspFace) -> Vec<u16> {
        let mut verts = Vec::with_capacity(face.num_edges as usize);
        for i in 0..face.num_edges as usize {
            let ei = self.face_edges[face.first_edge as usize + i];
            if ei >= 0 {
                verts.push(self.edges[ei as usize].v[0]);
            } else {
                verts.push(self.edges[(-ei) as usize].v[1]);
            }
        }
        verts
    }
}
