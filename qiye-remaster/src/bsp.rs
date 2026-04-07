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

/// BSP node for collision tree traversal.
#[derive(Debug, Clone)]
pub struct BspNode {
    pub plane_idx: u32,
    pub children: [i32; 2],  // negative = -(leaf_index), positive = node_index
    pub mins: [i16; 3],
    pub maxs: [i16; 3],
    pub first_face: u16,
    pub num_faces: u16,
}

/// BSP leaf — terminal node of BSP tree.
#[derive(Debug, Clone)]
pub struct BspLeaf {
    pub contents: i16,       // bit 0 = solid
    pub cluster: i16,
    pub mins: [i16; 3],
    pub maxs: [i16; 3],
    pub first_leaf_brush: u16,
    pub num_leaf_brushes: u16,
}

/// A convex brush defined by a set of planes.
#[derive(Debug, Clone)]
pub struct BspBrush {
    pub first_side: u16,
    pub num_sides: u16,
}

/// A brush side — references a plane.
#[derive(Debug, Clone)]
pub struct BspBrushSide {
    pub plane_index: u16,
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
    // Collision data
    pub nodes: Vec<BspNode>,
    pub leaves: Vec<BspLeaf>,
    pub brushes: Vec<BspBrush>,
    pub brush_sides: Vec<BspBrushSide>,
    pub leaf_brushes: Vec<u32>,  // indices into brushes[]
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
        // Collision lumps
        let brush_lump = read_lump_dir(data, 0x50);
        let brush_side_lump = read_lump_dir(data, 0x58);
        let leaf_brush_lump = read_lump_dir(data, 0xb8);
        let leaf_lump = read_lump_dir(data, 0xd8);
        let node_lump = read_lump_dir(data, 0xe0);

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

        // Parse collision data: brushes (4 bytes each: first_side i16, num_sides i16)
        let brush_count = brush_lump.size / 4;
        let mut brushes = Vec::with_capacity(brush_count);
        for i in 0..brush_count {
            let off = brush_lump.offset + i * 4;
            brushes.push(BspBrush {
                first_side: read_i16_le(data, off) as u16,
                num_sides: read_i16_le(data, off + 2) as u16,
            });
        }

        // Brush sides (4 bytes each: plane_index u16, surface_flags u16)
        let brush_side_count = brush_side_lump.size / 4;
        let mut brush_sides = Vec::with_capacity(brush_side_count);
        for i in 0..brush_side_count {
            let off = brush_side_lump.offset + i * 4;
            brush_sides.push(BspBrushSide {
                plane_index: read_u16_le(data, off),
            });
        }

        // Leaf brush list (4 bytes each: i32 brush index)
        let leaf_brush_count = leaf_brush_lump.size / 4;
        let mut leaf_brushes = Vec::with_capacity(leaf_brush_count);
        for i in 0..leaf_brush_count {
            leaf_brushes.push(read_u32_le(data, leaf_brush_lump.offset + i * 4));
        }

        // Leaves (0x20 = 32 bytes each on disk)
        let leaf_count = leaf_lump.size / 0x20;
        let mut leaves = Vec::with_capacity(leaf_count);
        for i in 0..leaf_count {
            let off = leaf_lump.offset + i * 0x20;
            leaves.push(BspLeaf {
                contents: read_i16_le(data, off + 0x0C),
                cluster: read_i16_le(data, off + 0x0E),
                mins: [
                    read_i16_le(data, off),
                    read_i16_le(data, off + 2),
                    read_i16_le(data, off + 4),
                ],
                maxs: [
                    read_i16_le(data, off + 6),
                    read_i16_le(data, off + 8),
                    read_i16_le(data, off + 10),
                ],
                first_leaf_brush: read_u16_le(data, off + 0x10),
                num_leaf_brushes: read_u16_le(data, off + 0x12),
            });
        }

        // Nodes (0x18 = 24 bytes each on disk)
        let node_count = node_lump.size / 0x18;
        let mut nodes = Vec::with_capacity(node_count);
        for i in 0..node_count {
            let off = node_lump.offset + i * 0x18;
            nodes.push(BspNode {
                plane_idx: read_u32_le(data, off),
                children: [
                    read_i16_le(data, off + 4) as i32,
                    read_i16_le(data, off + 6) as i32,
                ],
                mins: [
                    read_i16_le(data, off + 8),
                    read_i16_le(data, off + 10),
                    read_i16_le(data, off + 12),
                ],
                maxs: [
                    read_i16_le(data, off + 14),
                    read_i16_le(data, off + 16),
                    read_i16_le(data, off + 18),
                ],
                first_face: read_u16_le(data, off + 0x14),
                num_faces: read_u16_le(data, off + 0x16),
            });
        }

        println!(
            "BSP collision: {} nodes, {} leaves, {} brushes, {} brush_sides, {} leaf_brushes",
            nodes.len(), leaves.len(), brushes.len(), brush_sides.len(), leaf_brushes.len(),
        );

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
            nodes,
            leaves,
            brushes,
            brush_sides,
            leaf_brushes,
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

// ── BSP collision / trace ──────────────────────────────────────────────────

/// Result of a swept AABB trace through the BSP.
#[derive(Debug, Clone)]
pub struct TraceResult {
    pub fraction: f32,       // 0.0..1.0, how far the sweep got
    pub end_pos: [f32; 3],   // final position
    pub normal: [f32; 3],    // hit surface normal (zero if no hit)
    pub hit: bool,           // true if collided before reaching end
    pub all_solid: bool,     // true if start pos is inside solid
}

impl TraceResult {
    fn no_hit(end: [f32; 3]) -> Self {
        Self { fraction: 1.0, end_pos: end, normal: [0.0; 3], hit: false, all_solid: false }
    }
}

/// Swept AABB trace state.
struct TraceState<'a> {
    bsp: &'a Bsp,
    start: [f32; 3],
    end: [f32; 3],
    mins: [f32; 3],   // AABB min (relative, e.g. [-0.5, 0, -0.5])
    maxs: [f32; 3],   // AABB max (relative, e.g. [0.5, 2.0, 0.5])
    extents: [f32; 3], // half-extents for each axis
    fraction: f32,
    normal: [f32; 3],
    all_solid: bool,
}

impl Bsp {
    /// Swept AABB trace from `start` to `end` with box defined by `mins`/`maxs`.
    /// Mirrors `Bsp_traceBoxSwept` from the original binary.
    pub fn trace_box(&self, start: [f32; 3], end: [f32; 3], mins: [f32; 3], maxs: [f32; 3]) -> TraceResult {
        if self.nodes.is_empty() {
            return TraceResult::no_hit(end);
        }

        let extents = [
            maxs[0].abs().max(mins[0].abs()),
            maxs[1].abs().max(mins[1].abs()),
            maxs[2].abs().max(mins[2].abs()),
        ];

        let mut state = TraceState {
            bsp: self,
            start,
            end,
            mins,
            maxs,
            extents,
            fraction: 1.0,
            normal: [0.0; 3],
            all_solid: false,
        };

        state.trace_node(0, 0.0, 1.0, start, end);

        if state.all_solid {
            return TraceResult {
                fraction: 0.0,
                end_pos: start,
                normal: [0.0; 3],
                hit: true,
                all_solid: true,
            };
        }

        if state.fraction < 1.0 {
            // Lerp position and nudge off surface
            let eps = 0.02;
            let mut pos = [0.0f32; 3];
            for i in 0..3 {
                pos[i] = start[i] + (end[i] - start[i]) * state.fraction + state.normal[i] * eps;
            }
            TraceResult {
                fraction: state.fraction,
                end_pos: pos,
                normal: state.normal,
                hit: true,
                all_solid: false,
            }
        } else {
            TraceResult::no_hit(end)
        }
    }
}

impl<'a> TraceState<'a> {
    /// Recursive BSP node traversal (mirrors Bsp_traceNodeRecursive).
    fn trace_node(&mut self, node_idx: i32, start_frac: f32, end_frac: f32, start: [f32; 3], end: [f32; 3]) {
        if start_frac > self.fraction {
            return; // already found a closer hit
        }

        // Leaf node
        if node_idx < 0 {
            let leaf_idx = (-node_idx) as usize;
            if leaf_idx < self.bsp.leaves.len() {
                self.trace_leaf(leaf_idx);
            }
            return;
        }

        let node_idx = node_idx as usize;
        if node_idx >= self.bsp.nodes.len() {
            return;
        }

        let node = &self.bsp.nodes[node_idx];
        let plane_idx = node.plane_idx as usize;
        if plane_idx >= self.bsp.planes.len() {
            return;
        }
        let plane = &self.bsp.planes[plane_idx];

        // Compute distances from start/end to plane, offset by AABB extent
        let (start_dist, end_dist, offset) = {
            let n = &plane.normal;
            let axis_type = plane.type_flags;
            if axis_type < 3 {
                // Axial plane — just use the relevant component
                let ax = axis_type as usize;
                (start[ax] - plane.dist, end[ax] - plane.dist, self.extents[ax])
            } else {
                let sd = n[0] * start[0] + n[1] * start[1] + n[2] * start[2] - plane.dist;
                let ed = n[0] * end[0] + n[1] * end[1] + n[2] * end[2] - plane.dist;
                let off = self.extents[0] * n[0].abs() + self.extents[1] * n[1].abs() + self.extents[2] * n[2].abs();
                (sd, ed, off)
            }
        };

        // Both on front side
        if start_dist >= offset && end_dist >= offset {
            self.trace_node(node.children[0], start_frac, end_frac, start, end);
            return;
        }
        // Both on back side
        if start_dist < -offset && end_dist < -offset {
            self.trace_node(node.children[1], start_frac, end_frac, start, end);
            return;
        }

        // Crosses the plane — split the trace
        let inv_dist = start_dist - end_dist;
        let (front_child, back_child, t1, t2) = if start_dist < end_dist {
            // Moving from back to front
            let t1 = if inv_dist != 0.0 { (start_dist + offset) / inv_dist } else { 1.0 };
            let t2 = if inv_dist != 0.0 { (start_dist - offset) / inv_dist } else { 0.0 };
            (1i32, 0i32, t1, t2)
        } else if start_dist > end_dist {
            // Moving from front to back
            let t1 = if inv_dist != 0.0 { (start_dist - offset) / inv_dist } else { 1.0 };
            let t2 = if inv_dist != 0.0 { (start_dist + offset) / inv_dist } else { 0.0 };
            (0, 1, t1, t2)
        } else {
            (0, 1, 1.0, 0.0)
        };

        let t1 = t1.clamp(0.0, 1.0);
        let t2 = t2.clamp(0.0, 1.0);

        // Trace near side first
        let mid_frac = start_frac + (end_frac - start_frac) * t1;
        let mut mid = [0.0f32; 3];
        for i in 0..3 {
            mid[i] = start[i] + (end[i] - start[i]) * t1;
        }
        self.trace_node(node.children[front_child as usize], start_frac, mid_frac, start, mid);

        // Trace far side
        let mid_frac2 = start_frac + (end_frac - start_frac) * t2;
        let mut mid2 = [0.0f32; 3];
        for i in 0..3 {
            mid2[i] = start[i] + (end[i] - start[i]) * t2;
        }
        self.trace_node(node.children[back_child as usize], mid_frac2, end_frac, mid2, end);
    }

    /// Test trace against all brushes in a leaf (mirrors Bsp_traceLeafBrushes).
    fn trace_leaf(&mut self, leaf_idx: usize) {
        let leaf = &self.bsp.leaves[leaf_idx];
        let first = leaf.first_leaf_brush as usize;
        let count = leaf.num_leaf_brushes as usize;

        for i in 0..count {
            let lb_idx = first + i;
            if lb_idx >= self.bsp.leaf_brushes.len() {
                break;
            }
            let brush_idx = self.bsp.leaf_brushes[lb_idx] as usize;
            if brush_idx >= self.bsp.brushes.len() {
                continue;
            }
            self.trace_brush(brush_idx);
        }
    }

    /// Test trace against a single brush (mirrors brush test in Bsp_traceLeafBrushes).
    fn trace_brush(&mut self, brush_idx: usize) {
        let brush = &self.bsp.brushes[brush_idx];
        if brush.num_sides == 0 {
            return;
        }

        let mut enter_frac = -1.0f32;
        let mut leave_frac = 1.0f32;
        let mut hit_normal = [0.0f32; 3];
        let mut starts_out = false;
        let mut ends_out = false;

        for s in 0..brush.num_sides as usize {
            let side_idx = brush.first_side as usize + s;
            if side_idx >= self.bsp.brush_sides.len() {
                return;
            }
            let plane_idx = self.bsp.brush_sides[side_idx].plane_index as usize;
            if plane_idx >= self.bsp.planes.len() {
                return;
            }
            let plane = &self.bsp.planes[plane_idx];

            // Compute distance from start/end, offset by AABB extent along plane normal
            let n = &plane.normal;
            let offset = self.extents[0] * n[0].abs()
                       + self.extents[1] * n[1].abs()
                       + self.extents[2] * n[2].abs();

            let start_dist = n[0] * self.start[0] + n[1] * self.start[1] + n[2] * self.start[2]
                           - plane.dist - offset;
            let end_dist = n[0] * self.end[0] + n[1] * self.end[1] + n[2] * self.end[2]
                         - plane.dist - offset;

            if start_dist > 0.0 { starts_out = true; }
            if end_dist > 0.0 { ends_out = true; }

            // Both outside this plane — not inside brush
            if start_dist > 0.0 && end_dist > 0.0 {
                return;
            }
            // Both inside this plane — skip (still may be inside brush)
            if start_dist <= 0.0 && end_dist <= 0.0 {
                continue;
            }

            let inv = 1.0 / (start_dist - end_dist);
            if start_dist > end_dist {
                // Entering the brush
                let f = (start_dist - 0.02) * inv;
                if f > enter_frac {
                    enter_frac = f;
                    hit_normal = [n[0], n[1], n[2]];
                }
            } else {
                // Leaving the brush
                let f = (start_dist + 0.02) * inv;
                if f < leave_frac {
                    leave_frac = f;
                }
            }
        }

        if !starts_out {
            self.all_solid = !ends_out;
            return;
        }

        if enter_frac < leave_frac && enter_frac > -1.0 && enter_frac < self.fraction {
            let f = enter_frac.max(0.0);
            self.fraction = f;
            self.normal = hit_normal;
        }
    }
}
