use crate::bsp::{Bsp, BspFace, BspTexinfo};
use crate::entity::EntityManager;
use crate::fs_app::AppFs;
use crate::texture;
use crate::trigger::TriggerSystem;
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::ffi::CString;

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 3],
    uv: [f32; 2],
    lm_uv: [f32; 2],
}

struct DrawCall {
    texture_id: u32,
    index_offset: usize,
    index_count: usize,
}

pub struct BspRenderer {
    vao: u32,
    #[allow(dead_code)]
    vbo: u32,
    #[allow(dead_code)]
    ebo: u32,
    shader: u32,
    draw_calls: Vec<DrawCall>,
    lightmap_atlas: u32,
    u_mvp: i32,
    #[allow(dead_code)]
    u_lightmap: i32,
}

fn compile_shader(src: &str, shader_type: u32) -> u32 {
    unsafe {
        let shader = gl::CreateShader(shader_type);
        let c_str = CString::new(src).unwrap();
        gl::ShaderSource(shader, 1, &c_str.as_ptr(), std::ptr::null());
        gl::CompileShader(shader);

        let mut success = 0;
        gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut success);
        if success == 0 {
            let mut len = 0;
            gl::GetShaderiv(shader, gl::INFO_LOG_LENGTH, &mut len);
            let mut buf = vec![0u8; len as usize];
            gl::GetShaderInfoLog(shader, len, std::ptr::null_mut(), buf.as_mut_ptr() as *mut _);
            eprintln!(
                "Shader compile error: {}",
                String::from_utf8_lossy(&buf)
            );
        }
        shader
    }
}

fn link_program(vs: u32, fs: u32) -> u32 {
    unsafe {
        let program = gl::CreateProgram();
        gl::AttachShader(program, vs);
        gl::AttachShader(program, fs);
        gl::LinkProgram(program);

        let mut success = 0;
        gl::GetProgramiv(program, gl::LINK_STATUS, &mut success);
        if success == 0 {
            let mut len = 0;
            gl::GetProgramiv(program, gl::INFO_LOG_LENGTH, &mut len);
            let mut buf = vec![0u8; len as usize];
            gl::GetProgramInfoLog(
                program,
                len,
                std::ptr::null_mut(),
                buf.as_mut_ptr() as *mut _,
            );
            eprintln!("Shader link error: {}", String::from_utf8_lossy(&buf));
        }

        gl::DeleteShader(vs);
        gl::DeleteShader(fs);
        program
    }
}

const VERTEX_SHADER: &str = r#"
#version 330 core
layout (location = 0) in vec3 aPos;
layout (location = 1) in vec2 aUV;
layout (location = 2) in vec2 aLmUV;

uniform mat4 u_mvp;

out vec2 vUV;
out vec2 vLmUV;

void main() {
    gl_Position = u_mvp * vec4(aPos, 1.0);
    vUV = aUV;
    vLmUV = aLmUV;
}
"#;

const FRAGMENT_SHADER: &str = r#"
#version 330 core
in vec2 vUV;
in vec2 vLmUV;
out vec4 FragColor;

uniform sampler2D u_diffuse;
uniform sampler2D u_lightmap;

void main() {
    vec4 diffuse = texture(u_diffuse, vUV);
    float light = texture(u_lightmap, vLmUV).r;
    FragColor = diffuse * vec4(vec3(light * 2.0), 1.0);
}
"#;

/// Compute lightmap dimensions for a face from its vertex texture-space extents.
/// Returns (lm_w, lm_h, min_s_aligned, min_t_aligned).
fn face_lightmap_info(bsp: &Bsp, face: &BspFace, ti: &BspTexinfo) -> (u32, u32, f32, f32) {
    let verts = bsp.face_vertices(face);
    if verts.is_empty() {
        return (1, 1, 0.0, 0.0);
    }

    let mut min_s = f32::MAX;
    let mut max_s = f32::MIN;
    let mut min_t = f32::MAX;
    let mut max_t = f32::MIN;

    for &vi in &verts {
        let pos = bsp.vertices[vi as usize].pos;
        let s = pos[0] * ti.s_vec[0] + pos[1] * ti.s_vec[1] + pos[2] * ti.s_vec[2] + ti.s_vec[3];
        let t = pos[0] * ti.t_vec[0] + pos[1] * ti.t_vec[1] + pos[2] * ti.t_vec[2] + ti.t_vec[3];
        min_s = min_s.min(s);
        max_s = max_s.max(s);
        min_t = min_t.min(t);
        max_t = max_t.max(t);
    }

    let min_s_aligned = (min_s / 16.0).floor() * 16.0;
    let min_t_aligned = (min_t / 16.0).floor() * 16.0;
    // Game uses ceil for max (not floor like Quake)
    let lm_w = ((max_s / 16.0).ceil() - (min_s / 16.0).floor() + 1.0) as u32;
    let lm_h = ((max_t / 16.0).ceil() - (min_t / 16.0).floor() + 1.0) as u32;

    // Clamp to reasonable sizes
    let lm_w = lm_w.max(1).min(256);
    let lm_h = lm_h.max(1).min(256);

    (lm_w, lm_h, min_s_aligned, min_t_aligned)
}

/// Simple lightmap atlas packer using row-based shelf packing.
struct LightmapAtlas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    shelf_x: u32,
    shelf_y: u32,
    shelf_h: u32,
}

impl LightmapAtlas {
    fn new(size: u32) -> Self {
        Self {
            width: size,
            height: size,
            pixels: vec![255; (size * size) as usize], // default white (full bright)
            shelf_x: 0,
            shelf_y: 0,
            shelf_h: 0,
        }
    }

    /// Pack a lightmap patch into the atlas. Returns (u_offset, v_offset) in pixels.
    fn pack(&mut self, w: u32, h: u32, data: &[u8]) -> Option<(u32, u32)> {
        // Move to next shelf if needed
        if self.shelf_x + w > self.width {
            self.shelf_y += self.shelf_h;
            self.shelf_x = 0;
            self.shelf_h = 0;
        }
        if self.shelf_y + h > self.height {
            return None; // atlas full
        }

        let ox = self.shelf_x;
        let oy = self.shelf_y;

        // Copy lightmap data into atlas
        for row in 0..h {
            for col in 0..w {
                let src_idx = (row * w + col) as usize;
                let dst_idx = ((oy + row) * self.width + (ox + col)) as usize;
                if src_idx < data.len() && dst_idx < self.pixels.len() {
                    self.pixels[dst_idx] = data[src_idx];
                }
            }
        }

        self.shelf_x += w;
        self.shelf_h = self.shelf_h.max(h);
        Some((ox, oy))
    }

    /// Upload to OpenGL. Returns texture ID.
    unsafe fn upload(&self) -> u32 {
        let mut id = 0u32;
        gl::GenTextures(1, &mut id);
        gl::BindTexture(gl::TEXTURE_2D, id);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
        gl::TexImage2D(
            gl::TEXTURE_2D,
            0,
            gl::RED as i32,
            self.width as i32,
            self.height as i32,
            0,
            gl::RED,
            gl::UNSIGNED_BYTE,
            self.pixels.as_ptr() as *const _,
        );
        id
    }
}

/// Per-face lightmap placement in the atlas.
struct FaceLightmap {
    atlas_x: u32,
    atlas_y: u32,
    lm_w: u32,
    lm_h: u32,
    min_s_aligned: f32,
    min_t_aligned: f32,
}

impl BspRenderer {
    pub fn new(bsp: &Bsp, fs: &AppFs) -> Self {
        // Load textures
        let mut texture_ids: HashMap<String, u32> = HashMap::new();
        let mut fallback_tex = 0u32;

        // Create a fallback 1x1 white texture
        unsafe {
            gl::GenTextures(1, &mut fallback_tex);
            gl::BindTexture(gl::TEXTURE_2D, fallback_tex);
            let white: [u8; 4] = [200, 200, 200, 255];
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA as i32,
                1,
                1,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                white.as_ptr() as *const _,
            );
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
        }

        // Load unique textures from PAK, cache dimensions
        let mut texture_dims: HashMap<String, (f32, f32)> = HashMap::new();
        for ti in &bsp.texinfos {
            if texture_ids.contains_key(&ti.texture_name) {
                continue;
            }
            let tex_path = format!(".\\common\\{}.stx", ti.texture_name);
            let tex_data = fs.read(&tex_path);
            let tex_id = if let Some(data) = tex_data {
                if let Some(tex) = texture::parse_vid_impl(data) {
                    texture_dims.insert(
                        ti.texture_name.clone(),
                        (tex.width as f32, tex.height as f32),
                    );
                    unsafe { texture::upload_texture(&tex) }
                } else {
                    eprintln!("Failed to parse texture: {}", ti.texture_name);
                    fallback_tex
                }
            } else {
                fallback_tex
            };
            texture_ids.insert(ti.texture_name.clone(), tex_id);
        }

        println!(
            "Loaded {} textures ({} found in PAK)",
            texture_ids.len(),
            texture_dims.len()
        );

        // Build lightmap atlas — precompute all face lightmap placements
        let mut atlas = LightmapAtlas::new(1024);
        let mut face_lightmaps: Vec<Option<FaceLightmap>> = Vec::with_capacity(bsp.faces.len());
        let mut lm_packed = 0usize;

        for face in &bsp.faces {
            if face.texinfo_idx < 0
                || face.texinfo_idx as usize >= bsp.texinfos.len()
                || face.lightmap_offset < 0
            {
                face_lightmaps.push(None);
                continue;
            }

            let ti = &bsp.texinfos[face.texinfo_idx as usize];
            let (lm_w, lm_h, min_s_aligned, min_t_aligned) =
                face_lightmap_info(bsp, face, ti);
            let lm_size = (lm_w * lm_h) as usize;
            let lm_off = face.lightmap_offset as usize;

            // Extract lightmap bytes (style 0 only)
            let lm_data = if lm_off + lm_size <= bsp.lightmap_data.len() {
                &bsp.lightmap_data[lm_off..lm_off + lm_size]
            } else {
                face_lightmaps.push(None);
                continue;
            };

            if let Some((ax, ay)) = atlas.pack(lm_w, lm_h, lm_data) {
                face_lightmaps.push(Some(FaceLightmap {
                    atlas_x: ax,
                    atlas_y: ay,
                    lm_w,
                    lm_h,
                    min_s_aligned,
                    min_t_aligned,
                }));
                lm_packed += 1;
            } else {
                face_lightmaps.push(None);
            }
        }

        // Debug: check lightmap data range
        if !bsp.lightmap_data.is_empty() {
            let lm_min = *bsp.lightmap_data.iter().min().unwrap();
            let lm_max = *bsp.lightmap_data.iter().max().unwrap();
            let lm_avg: f32 = bsp.lightmap_data.iter().map(|&b| b as f32).sum::<f32>() / bsp.lightmap_data.len() as f32;
            println!("Lightmap data range: min={lm_min} max={lm_max} avg={lm_avg:.1}");
        }

        println!(
            "Lightmap atlas: {lm_packed}/{} faces packed into {}x{} atlas",
            bsp.faces.len(),
            atlas.width,
            atlas.height
        );

        let lightmap_atlas = unsafe { atlas.upload() };

        // Build vertex + index buffers, grouped by texture
        let mut all_vertices: Vec<Vertex> = Vec::new();
        let mut all_indices: Vec<u32> = Vec::new();

        // Group faces by texture
        let mut face_groups: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, face) in bsp.faces.iter().enumerate() {
            if face.texinfo_idx < 0 || face.texinfo_idx as usize >= bsp.texinfos.len() {
                continue;
            }
            let tex_name = &bsp.texinfos[face.texinfo_idx as usize].texture_name;
            face_groups
                .entry(tex_name.clone())
                .or_default()
                .push(i);
        }

        let mut draw_calls: Vec<DrawCall> = Vec::new();
        let atlas_w = atlas.width as f32;
        let atlas_h = atlas.height as f32;

        for (tex_name, face_indices) in &face_groups {
            let tex_id = *texture_ids.get(tex_name).unwrap_or(&fallback_tex);
            let index_offset = all_indices.len();

            for &fi in face_indices {
                let face = &bsp.faces[fi];
                let ti = &bsp.texinfos[face.texinfo_idx as usize];
                let verts = bsp.face_vertices(face);

                if verts.len() < 3 {
                    continue;
                }

                let base = all_vertices.len() as u32;
                let flm = &face_lightmaps[fi];

                for &vi in &verts {
                    let pos = bsp.vertices[vi as usize].pos;
                    let s = pos[0] * ti.s_vec[0]
                        + pos[1] * ti.s_vec[1]
                        + pos[2] * ti.s_vec[2]
                        + ti.s_vec[3];
                    let t = pos[0] * ti.t_vec[0]
                        + pos[1] * ti.t_vec[1]
                        + pos[2] * ti.t_vec[2]
                        + ti.t_vec[3];

                    let (tw, th) =
                        texture_dims.get(tex_name).copied().unwrap_or((64.0, 64.0));

                    // Lightmap UV: map face texture-space coords to atlas position
                    let (lm_u, lm_v) = if let Some(lm) = flm {
                        let local_u = (s - lm.min_s_aligned) / (lm.lm_w as f32 * 16.0);
                        let local_v = (t - lm.min_t_aligned) / (lm.lm_h as f32 * 16.0);
                        // Map to atlas coordinates
                        let atlas_u =
                            (lm.atlas_x as f32 + local_u * lm.lm_w as f32) / atlas_w;
                        let atlas_v =
                            (lm.atlas_y as f32 + local_v * lm.lm_h as f32) / atlas_h;
                        (atlas_u, atlas_v)
                    } else {
                        // No lightmap — point to a white texel (atlas defaults to 255)
                        (0.0, 0.0)
                    };

                    all_vertices.push(Vertex {
                        pos,
                        uv: [s / tw, t / th],
                        lm_uv: [lm_u, lm_v],
                    });
                }

                // Fan triangulate
                for j in 1..verts.len() - 1 {
                    all_indices.push(base);
                    all_indices.push(base + j as u32);
                    all_indices.push(base + j as u32 + 1);
                }
            }

            let index_count = all_indices.len() - index_offset;
            if index_count > 0 {
                draw_calls.push(DrawCall {
                    texture_id: tex_id,
                    index_offset,
                    index_count,
                });
            }
        }

        println!(
            "Renderer: {} vertices, {} indices, {} draw calls",
            all_vertices.len(),
            all_indices.len(),
            draw_calls.len(),
        );

        // Create shader program
        let vs = compile_shader(VERTEX_SHADER, gl::VERTEX_SHADER);
        let fs_shader = compile_shader(FRAGMENT_SHADER, gl::FRAGMENT_SHADER);
        let shader = link_program(vs, fs_shader);

        let (u_mvp, u_lightmap) = unsafe {
            let name = CString::new("u_mvp").unwrap();
            let mvp = gl::GetUniformLocation(shader, name.as_ptr());
            let name = CString::new("u_lightmap").unwrap();
            let lm = gl::GetUniformLocation(shader, name.as_ptr());
            (mvp, lm)
        };

        // Set texture units
        unsafe {
            gl::UseProgram(shader);
            let name = CString::new("u_diffuse").unwrap();
            let u_diffuse = gl::GetUniformLocation(shader, name.as_ptr());
            gl::Uniform1i(u_diffuse, 0);
            gl::Uniform1i(u_lightmap, 1);
        }

        // Upload to GPU
        let (mut vao, mut vbo, mut ebo) = (0u32, 0u32, 0u32);
        unsafe {
            gl::GenVertexArrays(1, &mut vao);
            gl::GenBuffers(1, &mut vbo);
            gl::GenBuffers(1, &mut ebo);

            gl::BindVertexArray(vao);

            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (all_vertices.len() * std::mem::size_of::<Vertex>()) as isize,
                all_vertices.as_ptr() as *const _,
                gl::STATIC_DRAW,
            );

            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, ebo);
            gl::BufferData(
                gl::ELEMENT_ARRAY_BUFFER,
                (all_indices.len() * std::mem::size_of::<u32>()) as isize,
                all_indices.as_ptr() as *const _,
                gl::STATIC_DRAW,
            );

            let stride = std::mem::size_of::<Vertex>() as i32;
            // position
            gl::VertexAttribPointer(0, 3, gl::FLOAT, gl::FALSE, stride, std::ptr::null());
            gl::EnableVertexAttribArray(0);
            // diffuse uv
            gl::VertexAttribPointer(
                1,
                2,
                gl::FLOAT,
                gl::FALSE,
                stride,
                (3 * std::mem::size_of::<f32>()) as *const _,
            );
            gl::EnableVertexAttribArray(1);
            // lightmap uv
            gl::VertexAttribPointer(
                2,
                2,
                gl::FLOAT,
                gl::FALSE,
                stride,
                (5 * std::mem::size_of::<f32>()) as *const _,
            );
            gl::EnableVertexAttribArray(2);

            gl::BindVertexArray(0);
        }

        BspRenderer {
            vao,
            vbo,
            ebo,
            shader,
            draw_calls,
            lightmap_atlas,
            u_mvp,
            u_lightmap,
        }
    }

    pub fn render(&self, mvp: &glam::Mat4) {
        unsafe {
            gl::UseProgram(self.shader);
            gl::UniformMatrix4fv(self.u_mvp, 1, gl::FALSE, mvp.as_ref().as_ptr());

            // Bind lightmap atlas to texture unit 1
            gl::ActiveTexture(gl::TEXTURE1);
            gl::BindTexture(gl::TEXTURE_2D, self.lightmap_atlas);
            gl::ActiveTexture(gl::TEXTURE0);

            gl::BindVertexArray(self.vao);

            for dc in &self.draw_calls {
                gl::BindTexture(gl::TEXTURE_2D, dc.texture_id);
                gl::DrawElements(
                    gl::TRIANGLES,
                    dc.index_count as i32,
                    gl::UNSIGNED_INT,
                    (dc.index_offset * std::mem::size_of::<u32>()) as *const _,
                );
            }

            gl::BindVertexArray(0);
        }
    }
}

// === Debug entity cube renderer ===

const DEBUG_VS: &str = r#"
#version 330 core
layout (location = 0) in vec3 aPos;
uniform mat4 u_mvp;
uniform vec3 u_color;
out vec3 vColor;
void main() {
    gl_Position = u_mvp * vec4(aPos, 1.0);
    vColor = u_color;
}
"#;

const DEBUG_FS: &str = r#"
#version 330 core
in vec3 vColor;
out vec4 FragColor;
void main() {
    FragColor = vec4(vColor, 1.0);
}
"#;

/// Unit cube vertices (8 corners of a [-0.5, 0.5] cube)
const CUBE_VERTS: [[f32; 3]; 8] = [
    [-0.5, -0.5, -0.5], [ 0.5, -0.5, -0.5],
    [ 0.5,  0.5, -0.5], [-0.5,  0.5, -0.5],
    [-0.5, -0.5,  0.5], [ 0.5, -0.5,  0.5],
    [ 0.5,  0.5,  0.5], [-0.5,  0.5,  0.5],
];

/// Line indices for wireframe cube (12 edges × 2 indices)
const CUBE_LINES: [u16; 24] = [
    0,1, 1,2, 2,3, 3,0,  // front face
    4,5, 5,6, 6,7, 7,4,  // back face
    0,4, 1,5, 2,6, 3,7,  // connecting edges
];

pub struct DebugRenderer {
    vao: u32,
    shader: u32,
    u_mvp: i32,
    u_color: i32,
}

impl DebugRenderer {
    pub fn new() -> Self {
        let vs = compile_shader(DEBUG_VS, gl::VERTEX_SHADER);
        let fs = compile_shader(DEBUG_FS, gl::FRAGMENT_SHADER);
        let shader = link_program(vs, fs);

        let u_mvp = unsafe {
            let name = CString::new("u_mvp").unwrap();
            gl::GetUniformLocation(shader, name.as_ptr())
        };
        let u_color = unsafe {
            let name = CString::new("u_color").unwrap();
            gl::GetUniformLocation(shader, name.as_ptr())
        };

        let (mut vao, mut vbo, mut ebo) = (0u32, 0u32, 0u32);
        unsafe {
            gl::GenVertexArrays(1, &mut vao);
            gl::GenBuffers(1, &mut vbo);
            gl::GenBuffers(1, &mut ebo);

            gl::BindVertexArray(vao);

            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                std::mem::size_of_val(&CUBE_VERTS) as isize,
                CUBE_VERTS.as_ptr() as *const _,
                gl::STATIC_DRAW,
            );

            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, ebo);
            gl::BufferData(
                gl::ELEMENT_ARRAY_BUFFER,
                std::mem::size_of_val(&CUBE_LINES) as isize,
                CUBE_LINES.as_ptr() as *const _,
                gl::STATIC_DRAW,
            );

            gl::VertexAttribPointer(0, 3, gl::FLOAT, gl::FALSE, 12, std::ptr::null());
            gl::EnableVertexAttribArray(0);
            gl::BindVertexArray(0);
        }

        DebugRenderer { vao, shader, u_mvp, u_color }
    }

    /// Render all entities as colored wireframe cubes.
    pub fn render_entities(&self, vp: &Mat4, entities: &EntityManager) {
        unsafe {
            gl::UseProgram(self.shader);
            gl::BindVertexArray(self.vao);
            gl::LineWidth(2.0);

            for ent in entities.entities() {
                if !ent.active {
                    continue;
                }

                let scale = ent.bbox_extents.max_element().max(1.0);
                let model = Mat4::from_translation(ent.transform.position)
                    * Mat4::from_scale(Vec3::splat(scale));
                let mvp = *vp * model;

                gl::UniformMatrix4fv(self.u_mvp, 1, gl::FALSE, mvp.as_ref().as_ptr());
                let color = ent.type_id.color();
                gl::Uniform3f(self.u_color, color[0], color[1], color[2]);

                gl::DrawElements(
                    gl::LINES,
                    CUBE_LINES.len() as i32,
                    gl::UNSIGNED_SHORT,
                    std::ptr::null(),
                );
            }

            gl::BindVertexArray(0);
        }
    }

    /// Render a grid on the XZ plane at Y=0 for model viewers.
    pub fn render_grid(&self, vp: &Mat4, size: f32, spacing: f32) {
        let steps = (size / spacing) as i32;
        let half = steps as f32 * spacing;

        // Build line vertices dynamically
        let mut verts: Vec<[f32; 3]> = Vec::new();
        for i in -steps..=steps {
            let p = i as f32 * spacing;
            // X-axis parallel lines
            verts.push([-half, 0.0, p]);
            verts.push([half, 0.0, p]);
            // Z-axis parallel lines
            verts.push([p, 0.0, -half]);
            verts.push([p, 0.0, half]);
        }

        let mut grid_vao = 0u32;
        let mut grid_vbo = 0u32;
        unsafe {
            gl::GenVertexArrays(1, &mut grid_vao);
            gl::GenBuffers(1, &mut grid_vbo);
            gl::BindVertexArray(grid_vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, grid_vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (verts.len() * 12) as isize,
                verts.as_ptr() as *const _,
                gl::STREAM_DRAW,
            );
            gl::VertexAttribPointer(0, 3, gl::FLOAT, gl::FALSE, 12, std::ptr::null());
            gl::EnableVertexAttribArray(0);

            gl::UseProgram(self.shader);
            gl::UniformMatrix4fv(self.u_mvp, 1, gl::FALSE, vp.as_ref().as_ptr());
            gl::Uniform3f(self.u_color, 0.3, 0.3, 0.3);
            gl::LineWidth(1.0);
            gl::DrawArrays(gl::LINES, 0, verts.len() as i32);

            gl::BindVertexArray(0);
            gl::DeleteBuffers(1, &grid_vbo);
            gl::DeleteVertexArrays(1, &grid_vao);
        }
    }

    pub fn render_triggers(&self, vp: &Mat4, triggers: &TriggerSystem) {
        unsafe {
            gl::UseProgram(self.shader);
            gl::BindVertexArray(self.vao);
            gl::LineWidth(1.0);

            for trigger in triggers.triggers() {
                let model = Mat4::from_translation(trigger.center)
                    * Mat4::from_scale(trigger.half_extents * 2.0);
                let mvp = *vp * model;

                gl::UniformMatrix4fv(self.u_mvp, 1, gl::FALSE, mvp.as_ref().as_ptr());
                let color = trigger.type_id.color();
                let alpha = if trigger.was_inside { 1.0 } else { 0.4 };
                gl::Uniform3f(self.u_color, color[0] * alpha, color[1] * alpha, color[2] * alpha);

                gl::DrawElements(
                    gl::LINES,
                    CUBE_LINES.len() as i32,
                    gl::UNSIGNED_SHORT,
                    std::ptr::null(),
                );
            }

            gl::BindVertexArray(0);
        }
    }
}
