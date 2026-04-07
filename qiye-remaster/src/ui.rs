/// Simple 2D overlay UI system.
///
/// Renders quads (dialog boxes, health bars) and text (ASCII bitmap font).
/// The full SBN font system is not yet implemented; this is a placeholder.

use std::ffi::CString;

const UI_VS: &str = r#"
#version 330 core
layout (location = 0) in vec2 aPos;
layout (location = 1) in vec2 aUV;
layout (location = 2) in vec4 aColor;

out vec2 vUV;
out vec4 vColor;

uniform vec2 u_screen_size;

void main() {
    vec2 ndc = (aPos / u_screen_size) * 2.0 - 1.0;
    ndc.y = -ndc.y; // flip Y (screen coords: top-left origin)
    gl_Position = vec4(ndc, 0.0, 1.0);
    vUV = aUV;
    vColor = aColor;
}
"#;

const UI_FS: &str = r#"
#version 330 core
in vec2 vUV;
in vec4 vColor;
out vec4 FragColor;

uniform sampler2D u_texture;
uniform int u_use_texture;

void main() {
    if (u_use_texture != 0) {
        vec4 tex = texture(u_texture, vUV);
        FragColor = tex * vColor;
    } else {
        FragColor = vColor;
    }
}
"#;

#[repr(C)]
#[derive(Clone, Copy)]
struct UiVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

pub struct UiRenderer {
    shader: u32,
    vao: u32,
    vbo: u32,
    u_screen_size: i32,
    u_use_texture: i32,
    font_tex: u32,
}

/// Simple 8×8 ASCII bitmap font (printable chars 32-127).
/// Each char is 8 pixels wide, stored as 8 bytes (one per row, MSB=leftmost).
fn generate_font_texture() -> u32 {
    // Minimal 8x8 bitmap font for ASCII 32-127
    // This is a basic built-in font; real game uses SBN fonts
    #[rustfmt::skip]
    const FONT_DATA: [u8; 768] = include!("font_data.inc");

    let char_count = 96; // chars 32-127
    let atlas_w = 16 * 8; // 16 chars per row
    let atlas_h = 6 * 8;  // 6 rows
    let mut pixels = vec![0u8; atlas_w * atlas_h * 4]; // RGBA

    for ch in 0..char_count {
        let col = ch % 16;
        let row = ch / 16;
        for y in 0..8 {
            let byte = FONT_DATA[ch * 8 + y];
            for x in 0..8 {
                if byte & (0x80 >> x) != 0 {
                    let px = col * 8 + x;
                    let py = row * 8 + y;
                    let idx = (py * atlas_w + px) * 4;
                    pixels[idx] = 255;
                    pixels[idx + 1] = 255;
                    pixels[idx + 2] = 255;
                    pixels[idx + 3] = 255;
                }
            }
        }
    }

    let mut tex = 0u32;
    unsafe {
        gl::GenTextures(1, &mut tex);
        gl::BindTexture(gl::TEXTURE_2D, tex);
        gl::TexImage2D(
            gl::TEXTURE_2D, 0, gl::RGBA as i32,
            atlas_w as i32, atlas_h as i32, 0,
            gl::RGBA, gl::UNSIGNED_BYTE,
            pixels.as_ptr() as *const _,
        );
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
    }
    tex
}

fn compile_shader(src: &str, shader_type: u32) -> u32 {
    unsafe {
        let shader = gl::CreateShader(shader_type);
        let c_str = CString::new(src).unwrap();
        gl::ShaderSource(shader, 1, &c_str.as_ptr(), std::ptr::null());
        gl::CompileShader(shader);
        shader
    }
}

impl UiRenderer {
    pub fn new() -> Self {
        let vs = compile_shader(UI_VS, gl::VERTEX_SHADER);
        let fs = compile_shader(UI_FS, gl::FRAGMENT_SHADER);
        let shader;
        let u_screen_size;
        let u_use_texture;
        unsafe {
            shader = gl::CreateProgram();
            gl::AttachShader(shader, vs);
            gl::AttachShader(shader, fs);
            gl::LinkProgram(shader);
            gl::DeleteShader(vs);
            gl::DeleteShader(fs);

            let name = CString::new("u_screen_size").unwrap();
            u_screen_size = gl::GetUniformLocation(shader, name.as_ptr());
            let name = CString::new("u_use_texture").unwrap();
            u_use_texture = gl::GetUniformLocation(shader, name.as_ptr());
        }

        let (mut vao, mut vbo) = (0u32, 0u32);
        unsafe {
            gl::GenVertexArrays(1, &mut vao);
            gl::GenBuffers(1, &mut vbo);
            gl::BindVertexArray(vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);

            let stride = std::mem::size_of::<UiVertex>() as i32;
            // pos
            gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, stride, std::ptr::null());
            gl::EnableVertexAttribArray(0);
            // uv
            gl::VertexAttribPointer(1, 2, gl::FLOAT, gl::FALSE, stride, (2 * 4) as *const _);
            gl::EnableVertexAttribArray(1);
            // color
            gl::VertexAttribPointer(2, 4, gl::FLOAT, gl::FALSE, stride, (4 * 4) as *const _);
            gl::EnableVertexAttribArray(2);

            gl::BindVertexArray(0);
        }

        let font_tex = generate_font_texture();

        UiRenderer {
            shader,
            vao,
            vbo,
            u_screen_size,
            u_use_texture,
            font_tex,
        }
    }

    /// Draw a filled rectangle.
    pub fn draw_rect(&self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4], screen_w: f32, screen_h: f32) {
        let verts = [
            UiVertex { pos: [x, y], uv: [0.0, 0.0], color },
            UiVertex { pos: [x + w, y], uv: [1.0, 0.0], color },
            UiVertex { pos: [x + w, y + h], uv: [1.0, 1.0], color },
            UiVertex { pos: [x, y], uv: [0.0, 0.0], color },
            UiVertex { pos: [x + w, y + h], uv: [1.0, 1.0], color },
            UiVertex { pos: [x, y + h], uv: [0.0, 1.0], color },
        ];

        unsafe {
            gl::UseProgram(self.shader);
            gl::Uniform2f(self.u_screen_size, screen_w, screen_h);
            gl::Uniform1i(self.u_use_texture, 0);

            gl::Disable(gl::DEPTH_TEST);
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);

            gl::BindVertexArray(self.vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (verts.len() * std::mem::size_of::<UiVertex>()) as isize,
                verts.as_ptr() as *const _,
                gl::STREAM_DRAW,
            );
            gl::DrawArrays(gl::TRIANGLES, 0, 6);

            gl::Enable(gl::DEPTH_TEST);
            gl::Disable(gl::BLEND);
            gl::BindVertexArray(0);
        }
    }

    /// Draw ASCII text at screen position.
    pub fn draw_text(&self, text: &str, x: f32, y: f32, scale: f32, color: [f32; 4], screen_w: f32, screen_h: f32) {
        let char_w = 8.0 * scale;
        let char_h = 8.0 * scale;
        let atlas_cols = 16.0;
        let atlas_rows = 6.0;
        let uv_w = 1.0 / atlas_cols;
        let uv_h = 1.0 / atlas_rows;

        let mut verts = Vec::with_capacity(text.len() * 6);
        let mut cx = x;
        let mut cy = y;

        for ch in text.bytes() {
            if ch == b'\n' {
                cx = x;
                cy += char_h + 2.0 * scale;
                continue;
            }
            let idx = if ch >= 32 && ch < 128 { (ch - 32) as f32 } else { 0.0 };
            let col = idx % atlas_cols;
            let row = (idx / atlas_cols).floor();
            let u0 = col * uv_w;
            let v0 = row * uv_h;
            let u1 = u0 + uv_w;
            let v1 = v0 + uv_h;

            verts.push(UiVertex { pos: [cx, cy], uv: [u0, v0], color });
            verts.push(UiVertex { pos: [cx + char_w, cy], uv: [u1, v0], color });
            verts.push(UiVertex { pos: [cx + char_w, cy + char_h], uv: [u1, v1], color });
            verts.push(UiVertex { pos: [cx, cy], uv: [u0, v0], color });
            verts.push(UiVertex { pos: [cx + char_w, cy + char_h], uv: [u1, v1], color });
            verts.push(UiVertex { pos: [cx, cy + char_h], uv: [u0, v1], color });

            cx += char_w;
        }

        if verts.is_empty() {
            return;
        }

        unsafe {
            gl::UseProgram(self.shader);
            gl::Uniform2f(self.u_screen_size, screen_w, screen_h);
            gl::Uniform1i(self.u_use_texture, 1);

            gl::Disable(gl::DEPTH_TEST);
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);

            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, self.font_tex);

            gl::BindVertexArray(self.vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (verts.len() * std::mem::size_of::<UiVertex>()) as isize,
                verts.as_ptr() as *const _,
                gl::STREAM_DRAW,
            );
            gl::DrawArrays(gl::TRIANGLES, 0, verts.len() as i32);

            gl::Enable(gl::DEPTH_TEST);
            gl::Disable(gl::BLEND);
            gl::BindVertexArray(0);
        }
    }
}

/// Dialog box state.
pub struct DialogBox {
    pub visible: bool,
    pub text: String,
    pub typewriter_pos: usize,
    pub typewriter_speed: f32, // chars per second
    elapsed: f32,
}

impl DialogBox {
    pub fn new() -> Self {
        Self {
            visible: false,
            text: String::new(),
            typewriter_pos: 0,
            typewriter_speed: 30.0,
            elapsed: 0.0,
        }
    }

    pub fn show(&mut self, text: &str) {
        self.visible = true;
        self.text = text.to_string();
        self.typewriter_pos = 0;
        self.elapsed = 0.0;
    }

    pub fn dismiss(&mut self) {
        self.visible = false;
    }

    pub fn is_complete(&self) -> bool {
        self.typewriter_pos >= self.text.len()
    }

    pub fn skip_to_end(&mut self) {
        self.typewriter_pos = self.text.len();
    }

    pub fn update(&mut self, dt: f32) {
        if !self.visible || self.is_complete() {
            return;
        }
        self.elapsed += dt;
        self.typewriter_pos = (self.elapsed * self.typewriter_speed) as usize;
        if self.typewriter_pos > self.text.len() {
            self.typewriter_pos = self.text.len();
        }
    }

    pub fn render(&self, ui: &UiRenderer, screen_w: f32, screen_h: f32) {
        if !self.visible {
            return;
        }

        let box_h = 120.0;
        let box_y = screen_h - box_h - 10.0;
        let margin = 20.0;

        // Semi-transparent background
        ui.draw_rect(margin, box_y, screen_w - margin * 2.0, box_h,
            [0.0, 0.0, 0.0, 0.7], screen_w, screen_h);

        // Border
        ui.draw_rect(margin, box_y, screen_w - margin * 2.0, 2.0,
            [0.5, 0.5, 0.8, 1.0], screen_w, screen_h);

        // Text (typewriter effect)
        let visible_text = &self.text[..self.typewriter_pos.min(self.text.len())];
        ui.draw_text(visible_text, margin + 10.0, box_y + 10.0, 2.0,
            [1.0, 1.0, 1.0, 1.0], screen_w, screen_h);

        // "Press Enter" prompt when complete
        if self.is_complete() {
            ui.draw_text("[Enter]", screen_w - margin - 80.0, box_y + box_h - 20.0, 1.5,
                [0.7, 0.7, 1.0, 1.0], screen_w, screen_h);
        }
    }
}
