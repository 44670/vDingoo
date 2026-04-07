/// SAI skeletal animation parser.
///
/// SAI binary format:
///   i32 total_frames
///   i32 loop_frame          (frame to loop back to, or total duration)
///   i32 flags               (lower byte = rest_pose_count, bit 0x100 = has_vertex_anim)
///   [RestPose; rest_pose_count]   -- 24 bytes each (2 × vec3 in 16.16 fp)
///   u32 bone_track_count
///   [BoneTrack; bone_track_count]
///     i32 keyframe_count
///     [BoneKeyframe; keyframe_count]  -- 36 bytes each
///   (if has_vertex_anim)
///     u16 verts_per_frame
///     i16 vertex_frame_count
///     [VertexTrack; vertex_frame_count]
///       i32 keyframe_count
///       [VertexKeyframe; keyframe_count × verts_per_frame]  -- 12 bytes each (vec3 fp)
///
/// BoneKeyframe (36 bytes on disk):
///   i32  frame_time
///   u16  sign_flags       (9 bits, one per rotation matrix element)
///   u16[9] rotation       (unsigned magnitudes, 16.16 fp upper half)
///   i32[3] translation    (16.16 fp)
///
/// In-memory each keyframe expands to a 4×4 homogeneous matrix (16 f32s) + frame_time.

fn read_i16_le(d: &[u8], o: usize) -> i16 {
    i16::from_le_bytes([d[o], d[o + 1]])
}

fn read_u16_le(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}

fn read_i32_le(d: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

fn fp_to_f32(v: i32) -> f32 {
    v as f32 / 65536.0
}

/// A 4×4 transform matrix stored as 16 f32s (row-major).
/// Layout: [rot00, rot01, rot02, 0, rot10, rot11, rot12, 0, rot20, rot21, rot22, 0, tx, ty, tz, 1.0]
pub type BoneMatrix = [f32; 16];

fn identity_matrix() -> BoneMatrix {
    [
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ]
}

/// A single bone keyframe: a transform at a specific frame time.
#[derive(Debug, Clone)]
pub struct BoneKeyframe {
    pub frame_time: i32,
    pub matrix: BoneMatrix,
}

/// A bone animation track: sequence of keyframes for one bone.
#[derive(Debug, Clone)]
pub struct BoneTrack {
    pub keyframes: Vec<BoneKeyframe>,
}

/// A vertex animation keyframe: position delta per vertex.
#[derive(Debug, Clone)]
pub struct VertexKeyframe {
    pub frame_time: i32,
    pub positions: Vec<[f32; 3]>, // one vec3 per vertex
}

/// A vertex animation track.
#[derive(Debug, Clone)]
pub struct VertexTrack {
    pub keyframes: Vec<VertexKeyframe>,
}

/// Parsed SAI animation.
#[derive(Debug)]
pub struct SaiAnimation {
    pub total_frames: i32,
    pub loop_frame: i32,
    pub bone_tracks: Vec<BoneTrack>,
    pub vertex_tracks: Vec<VertexTrack>,
    pub verts_per_frame: u16,
}

impl SaiAnimation {
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }

        let mut off = 0;

        let total_frames = read_i32_le(data, off);
        let loop_frame = read_i32_le(data, off + 4);
        let flags = read_i32_le(data, off + 8);
        off += 12;

        let rest_pose_count = (flags & 0xFF) as usize;
        let has_vertex_anim = (flags & 0x100) != 0;

        // Skip rest pose data (24 bytes each = 2 × vec3 in 16.16 fp)
        let rest_pose_size = rest_pose_count * 24;
        if off + rest_pose_size > data.len() {
            eprintln!("SAI: rest pose overflow (count={rest_pose_count})");
            return None;
        }
        off += rest_pose_size;

        // Bone keyframe tracks
        if off + 4 > data.len() {
            return None;
        }
        let bone_track_count = read_u16_le(data, off) as usize;
        off += 4; // u32 but only lower u16 used

        if bone_track_count > 256 {
            eprintln!("SAI: bad bone_track_count {bone_track_count}");
            return None;
        }

        let mut bone_tracks = Vec::with_capacity(bone_track_count);
        for _ in 0..bone_track_count {
            if off + 4 > data.len() {
                break;
            }
            let kf_count = read_i32_le(data, off) as usize;
            off += 4;

            if kf_count > 10000 || off + kf_count * 36 > data.len() {
                eprintln!("SAI: keyframe overflow (count={kf_count})");
                return None;
            }

            let mut keyframes = Vec::with_capacity(kf_count);
            for _ in 0..kf_count {
                let frame_time = read_i32_le(data, off);
                let sign_flags = read_u16_le(data, off + 4);

                // Read 9 rotation matrix elements (u16 magnitudes)
                let mut rot = [0.0f32; 9];
                for i in 0..9 {
                    let magnitude = read_u16_le(data, off + 6 + i * 2) as i32;
                    let signed = if (sign_flags & (1 << i)) != 0 {
                        -magnitude
                    } else {
                        magnitude
                    };
                    // These are 16.16 fp values stored as u16 (upper portion)
                    // The original code does: zx.d(u16) then stores as i32 in matrix
                    // Treating as 0.16 fixed-point (values in [-1, 1] range for rotation)
                    rot[i] = signed as f32 / 65536.0;
                }

                // Read translation (3 × i32, 16.16 fp)
                let tx = fp_to_f32(read_i32_le(data, off + 24));
                let ty = fp_to_f32(read_i32_le(data, off + 28));
                let tz = fp_to_f32(read_i32_le(data, off + 32));

                // Build 4×4 matrix (matching original layout):
                // [rot00, rot01, rot02, 0]
                // [rot10, rot11, rot12, 0]
                // [rot20, rot21, rot22, 0]
                // [tx,    ty,    tz,    1]
                let matrix: BoneMatrix = [
                    rot[0], rot[1], rot[2], 0.0,
                    rot[3], rot[4], rot[5], 0.0,
                    rot[6], rot[7], rot[8], 0.0,
                    tx,     ty,     tz,     1.0,
                ];

                keyframes.push(BoneKeyframe { frame_time, matrix });
                off += 36;
            }

            bone_tracks.push(BoneTrack { keyframes });
        }

        // Vertex animation
        let mut vertex_tracks = Vec::new();
        let mut verts_per_frame = 0u16;

        if has_vertex_anim && off + 4 <= data.len() {
            verts_per_frame = read_u16_le(data, off);
            let vertex_frame_count = read_i16_le(data, off + 2) as usize;
            off += 4;

            if verts_per_frame == 2 && vertex_frame_count > 0 && vertex_frame_count < 10000 {
                for _ in 0..vertex_frame_count {
                    if off + 4 > data.len() {
                        break;
                    }
                    let kf_count = read_i32_le(data, off) as usize;
                    off += 4;

                    let entries = kf_count * verts_per_frame as usize;
                    if off + entries * 12 > data.len() {
                        break;
                    }

                    // TODO: Parse vertex keyframes properly
                    // For now, skip the data
                    let mut keyframes = Vec::new();
                    for _ in 0..kf_count {
                        let mut positions = Vec::with_capacity(verts_per_frame as usize);
                        for _ in 0..verts_per_frame {
                            let x = fp_to_f32(read_i32_le(data, off));
                            let y = fp_to_f32(read_i32_le(data, off + 4));
                            let z = fp_to_f32(read_i32_le(data, off + 8));
                            positions.push([x, y, z]);
                            off += 12;
                        }
                        keyframes.push(VertexKeyframe {
                            frame_time: 0, // vertex anim doesn't have per-keyframe times in same way
                            positions,
                        });
                    }
                    vertex_tracks.push(VertexTrack { keyframes });
                }
            }
        }

        let total_bone_kf: usize = bone_tracks.iter().map(|t| t.keyframes.len()).sum();
        println!(
            "SAI: {} frames, {} bone tracks, {} total keyframes, {} vertex tracks, loop={}",
            total_frames,
            bone_tracks.len(),
            total_bone_kf,
            vertex_tracks.len(),
            loop_frame,
        );

        Some(SaiAnimation {
            total_frames,
            loop_frame,
            bone_tracks,
            vertex_tracks,
            verts_per_frame,
        })
    }

    /// Sample a bone track at a given frame time, returning an interpolated 4×4 matrix.
    pub fn sample_bone(&self, track_idx: usize, frame: f32) -> BoneMatrix {
        let track = match self.bone_tracks.get(track_idx) {
            Some(t) => t,
            None => return identity_matrix(),
        };

        if track.keyframes.is_empty() {
            return identity_matrix();
        }

        // Single keyframe — return it directly
        if track.keyframes.len() == 1 {
            return track.keyframes[0].matrix;
        }

        let frame_i = frame as i32;

        // Find the two keyframes to interpolate between
        let mut next_idx = track.keyframes.len();
        for (i, kf) in track.keyframes.iter().enumerate() {
            if kf.frame_time >= frame_i {
                if kf.frame_time == frame_i {
                    return kf.matrix;
                }
                next_idx = i;
                break;
            }
        }

        // Clamp to first/last
        if next_idx == 0 {
            return track.keyframes[0].matrix;
        }
        if next_idx >= track.keyframes.len() {
            return track.keyframes.last().unwrap().matrix;
        }

        let prev = &track.keyframes[next_idx - 1];
        let next = &track.keyframes[next_idx];

        let dt = (next.frame_time - prev.frame_time) as f32;
        let t = if dt > 0.0 {
            ((frame - prev.frame_time as f32) / dt).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Linear interpolation of all 16 matrix elements
        let mut result = [0.0f32; 16];
        for i in 0..16 {
            result[i] = prev.matrix[i] + (next.matrix[i] - prev.matrix[i]) * t;
        }
        result
    }

    pub fn duration_frames(&self) -> i32 {
        self.total_frames
    }

    pub fn bone_track_count(&self) -> usize {
        self.bone_tracks.len()
    }
}

/// Animation playback controller.
pub struct AnimController {
    pub current_frame: f32,
    pub speed: f32,
    pub looping: bool,
    pub playing: bool,
    total_frames: i32,
    loop_frame: i32,
}

impl AnimController {
    pub fn new(anim: &SaiAnimation) -> Self {
        AnimController {
            current_frame: 0.0,
            speed: 1.0,
            looping: true,
            playing: true,
            total_frames: anim.total_frames,
            loop_frame: anim.loop_frame,
        }
    }

    pub fn total_frames(&self) -> i32 {
        self.total_frames
    }

    pub fn update(&mut self, dt: f32) {
        if !self.playing || self.total_frames <= 0 {
            return;
        }

        // Advance frame (original runs at ~30fps equivalent)
        self.current_frame += self.speed * dt * 30.0;

        if self.current_frame >= self.total_frames as f32 {
            if self.looping {
                let loop_start = self.loop_frame.max(0) as f32;
                let loop_len = self.total_frames as f32 - loop_start;
                if loop_len > 0.0 {
                    self.current_frame = loop_start
                        + (self.current_frame - loop_start) % loop_len;
                } else {
                    self.current_frame = 0.0;
                }
            } else {
                self.current_frame = (self.total_frames - 1) as f32;
                self.playing = false;
            }
        }
    }
}
