/// GL renderer for SOJ models.

use crate::fs_app::AppFs;
use crate::model::SojModel;
use crate::texture;
use glam::Mat4;
use std::collections::HashMap;
use std::ffi::CString;

const MODEL_VS: &str = r#"
#version 330 core
layout (location = 0) in vec3 aPos;
layout (location = 1) in vec3 aNormal;
layout (location = 2) in vec2 aUV;

uniform mat4 u_mvp;
uniform mat4 u_model;

out vec2 vUV;
out vec3 vNormal;

void main() {
    gl_Position = u_mvp * vec4(aPos, 1.0);
    vUV = aUV;
    vNormal = mat3(u_model) * aNormal;
}
"#;

const MODEL_FS: &str = r#"
#version 330 core
in vec2 vUV;
in vec3 vNormal;
out vec4 FragColor;

uniform sampler2D u_diffuse;
uniform vec4 u_color;

void main() {
    vec3 n = normalize(vNormal);
    float light = max(dot(n, normalize(vec3(0.5, 1.0, 0.3))), 0.0) * 0.6 + 0.4;
    vec4 tex = texture(u_diffuse, vUV);
    FragColor = tex * u_color * vec4(vec3(light), 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy)]
struct ModelVertex {
    pos: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

struct ModelDrawCall {
    texture_id: u32,
    color: [f32; 4],
    index_offset: usize,
    index_count: usize,
}

/// A GPU-ready model built from a parsed SOJ.
pub struct GpuModel {
    vao: u32,
    draw_calls: Vec<ModelDrawCall>,
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
            eprintln!("Model shader compile error: {}", String::from_utf8_lossy(&buf));
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
            gl::GetProgramInfoLog(program, len, std::ptr::null_mut(), buf.as_mut_ptr() as *mut _);
            eprintln!("Model shader link error: {}", String::from_utf8_lossy(&buf));
        }
        gl::DeleteShader(vs);
        gl::DeleteShader(fs);
        program
    }
}

pub struct ModelRenderer {
    shader: u32,
    u_mvp: i32,
    u_model: i32,
    u_color: i32,
    fallback_tex: u32,
    texture_cache: HashMap<String, u32>,
    models: HashMap<String, GpuModel>,
}

impl ModelRenderer {
    pub fn new() -> Self {
        let vs = compile_shader(MODEL_VS, gl::VERTEX_SHADER);
        let fs = compile_shader(MODEL_FS, gl::FRAGMENT_SHADER);
        let shader = link_program(vs, fs);

        let u_mvp;
        let u_model;
        let u_color;
        unsafe {
            let name = CString::new("u_mvp").unwrap();
            u_mvp = gl::GetUniformLocation(shader, name.as_ptr());
            let name = CString::new("u_model").unwrap();
            u_model = gl::GetUniformLocation(shader, name.as_ptr());
            let name = CString::new("u_color").unwrap();
            u_color = gl::GetUniformLocation(shader, name.as_ptr());

            gl::UseProgram(shader);
            let name = CString::new("u_diffuse").unwrap();
            let u_diffuse = gl::GetUniformLocation(shader, name.as_ptr());
            gl::Uniform1i(u_diffuse, 0);
        }

        let mut fallback_tex = 0u32;
        unsafe {
            gl::GenTextures(1, &mut fallback_tex);
            gl::BindTexture(gl::TEXTURE_2D, fallback_tex);
            let white: [u8; 4] = [200, 200, 200, 255];
            gl::TexImage2D(
                gl::TEXTURE_2D, 0, gl::RGBA as i32, 1, 1, 0,
                gl::RGBA, gl::UNSIGNED_BYTE, white.as_ptr() as *const _,
            );
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
        }

        ModelRenderer {
            shader,
            u_mvp,
            u_model,
            u_color,
            fallback_tex,
            texture_cache: HashMap::new(),
            models: HashMap::new(),
        }
    }

    /// Load a texture from PAK, caching by name.
    fn load_texture(&mut self, name: &str, fs: &AppFs) -> u32 {
        if let Some(&id) = self.texture_cache.get(name) {
            return id;
        }
        let path = format!(".\\common\\{}.stx", name);
        let tex_id = if let Some(data) = fs.read(&path) {
            if let Some(tex) = texture::parse_vid_impl(data) {
                unsafe { texture::upload_texture(&tex) }
            } else {
                self.fallback_tex
            }
        } else {
            self.fallback_tex
        };
        self.texture_cache.insert(name.to_string(), tex_id);
        tex_id
    }

    /// Upload a SOJ model to the GPU. Returns true if successful.
    pub fn upload_model(&mut self, name: &str, soj: &SojModel, fs: &AppFs) -> bool {
        if self.models.contains_key(name) {
            return true;
        }

        let triangulated = soj.triangulate();
        if triangulated.is_empty() {
            return false;
        }

        let mut all_vertices: Vec<ModelVertex> = Vec::new();
        let mut all_indices: Vec<u32> = Vec::new();
        let mut draw_calls: Vec<ModelDrawCall> = Vec::new();

        for (mat_idx, verts, indices) in &triangulated {
            if indices.is_empty() {
                continue;
            }

            let base = all_vertices.len() as u32;
            let index_offset = all_indices.len();

            for v in verts {
                all_vertices.push(ModelVertex {
                    pos: v.pos,
                    normal: v.normal,
                    uv: v.uv,
                });
            }
            for &idx in indices {
                all_indices.push(base + idx as u32);
            }

            let (tex_id, color) = if *mat_idx >= 0 && (*mat_idx as usize) < soj.materials.len() {
                let mat = &soj.materials[*mat_idx as usize];
                let tex = self.load_texture(&mat.texture_name, fs);
                let c = [
                    mat.color[0] as f32 / 255.0,
                    mat.color[1] as f32 / 255.0,
                    mat.color[2] as f32 / 255.0,
                    mat.color[3] as f32 / 255.0,
                ];
                (tex, c)
            } else {
                (self.fallback_tex, [1.0, 1.0, 1.0, 1.0])
            };

            draw_calls.push(ModelDrawCall {
                texture_id: tex_id,
                color,
                index_offset,
                index_count: indices.len(),
            });
        }

        if all_vertices.is_empty() {
            return false;
        }

        let (mut vao, mut vbo, mut ebo) = (0u32, 0u32, 0u32);
        unsafe {
            gl::GenVertexArrays(1, &mut vao);
            gl::GenBuffers(1, &mut vbo);
            gl::GenBuffers(1, &mut ebo);

            gl::BindVertexArray(vao);

            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (all_vertices.len() * std::mem::size_of::<ModelVertex>()) as isize,
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

            let stride = std::mem::size_of::<ModelVertex>() as i32;
            // pos
            gl::VertexAttribPointer(0, 3, gl::FLOAT, gl::FALSE, stride, std::ptr::null());
            gl::EnableVertexAttribArray(0);
            // normal
            gl::VertexAttribPointer(
                1, 3, gl::FLOAT, gl::FALSE, stride,
                (3 * std::mem::size_of::<f32>()) as *const _,
            );
            gl::EnableVertexAttribArray(1);
            // uv
            gl::VertexAttribPointer(
                2, 2, gl::FLOAT, gl::FALSE, stride,
                (6 * std::mem::size_of::<f32>()) as *const _,
            );
            gl::EnableVertexAttribArray(2);

            gl::BindVertexArray(0);
        }

        self.models.insert(name.to_string(), GpuModel { vao, draw_calls });
        true
    }

    /// Render a named model with a model matrix and view-projection matrix.
    pub fn render(&self, name: &str, model_mat: &Mat4, vp: &Mat4) {
        let gpu_model = match self.models.get(name) {
            Some(m) => m,
            None => return,
        };

        let mvp = *vp * *model_mat;

        unsafe {
            gl::UseProgram(self.shader);
            gl::UniformMatrix4fv(self.u_mvp, 1, gl::FALSE, mvp.as_ref().as_ptr());
            gl::UniformMatrix4fv(self.u_model, 1, gl::FALSE, model_mat.as_ref().as_ptr());

            gl::BindVertexArray(gpu_model.vao);

            for dc in &gpu_model.draw_calls {
                gl::Uniform4f(self.u_color, dc.color[0], dc.color[1], dc.color[2], dc.color[3]);
                gl::ActiveTexture(gl::TEXTURE0);
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

    pub fn has_model(&self, name: &str) -> bool {
        self.models.contains_key(name)
    }
}
