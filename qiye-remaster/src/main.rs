mod bsp;
mod fs_app;
mod packed_string;
mod render;
mod texture;

use glam::{Mat4, Vec3};
use sdl2::event::Event;
use sdl2::keyboard::Scancode;

struct Camera {
    pos: Vec3,
    yaw: f32,
    pitch: f32,
    speed: f32,
}

impl Camera {
    fn new(pos: Vec3, speed: f32) -> Self {
        Self {
            pos,
            yaw: 0.0,
            pitch: 0.0,
            speed,
        }
    }

    /// Full 3D look direction (for view matrix)
    fn look_dir(&self) -> Vec3 {
        Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        )
    }

    /// Horizontal forward (on XZ ground plane, ignoring pitch)
    fn forward_xz(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin()).normalize()
    }

    /// Horizontal right (on XZ ground plane)
    fn right_xz(&self) -> Vec3 {
        Vec3::new(self.yaw.sin(), 0.0, -self.yaw.cos())
    }

    fn view_matrix(&self) -> Mat4 {
        Mat4::look_at_lh(self.pos, self.pos + self.look_dir(), Vec3::Y)
    }

    fn update(&mut self, keys: &sdl2::keyboard::KeyboardState, dt: f32) {
        let fwd = self.forward_xz();
        let right = self.right_xz();
        let mut vel = Vec3::ZERO;

        if keys.is_scancode_pressed(Scancode::W) {
            vel += fwd;
        }
        if keys.is_scancode_pressed(Scancode::S) {
            vel -= fwd;
        }
        if keys.is_scancode_pressed(Scancode::D) {
            vel += right;
        }
        if keys.is_scancode_pressed(Scancode::A) {
            vel -= right;
        }
        if keys.is_scancode_pressed(Scancode::Space) {
            vel += Vec3::Y;
        }
        if keys.is_scancode_pressed(Scancode::LShift) {
            vel -= Vec3::Y;
        }

        let speed = if keys.is_scancode_pressed(Scancode::LCtrl) {
            self.speed * 3.0
        } else {
            self.speed
        };

        if vel.length_squared() > 0.0 {
            self.pos += vel.normalize() * speed * dt;
        }
    }
}

fn find_bsp(fs: &fs_app::AppFs, name: Option<&str>) -> Option<String> {
    if let Some(name) = name {
        if fs.read(name).is_some() {
            return Some(name.to_string());
        }
    }

    let mut bsp_files: Vec<String> = fs
        .list_files()
        .filter(|p| p.ends_with(".sbp"))
        .map(|p| p.to_string())
        .collect();
    bsp_files.sort();

    if bsp_files.is_empty() {
        return None;
    }

    println!("Found {} BSP maps:", bsp_files.len());
    for (i, path) in bsp_files.iter().enumerate().take(10) {
        println!("  [{i}] {path}");
    }
    if bsp_files.len() > 10 {
        println!("  ... and {} more", bsp_files.len() - 10);
    }

    bsp_files
        .iter()
        .find(|p| p.contains("day1"))
        .cloned()
        .or_else(|| bsp_files.first().cloned())
}

fn main() {
    let app_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "../qiye.app".to_string());

    println!("Loading {app_path}...");
    let fs = fs_app::AppFs::open(&app_path);
    println!("PAK: {} files", fs.file_count());

    // List file extensions
    let mut ext_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for path in fs.list_files() {
        let ext = path
            .rsplit('.')
            .next()
            .map(|s| format!(".{s}"))
            .unwrap_or_else(|| "(none)".to_string());
        *ext_counts.entry(ext).or_default() += 1;
    }
    let mut ext_list: Vec<_> = ext_counts.into_iter().collect();
    ext_list.sort_by(|a, b| b.1.cmp(&a.1));
    println!("File types:");
    for (ext, count) in &ext_list {
        println!("  {ext}: {count}");
    }

    // Find and load BSP
    let bsp_name = std::env::args().nth(2);
    let bsp_path = find_bsp(&fs, bsp_name.as_deref()).expect("No BSP file found in PAK");
    println!("Loading BSP: {bsp_path}");
    let bsp_data = fs.read(&bsp_path).unwrap();
    let bsp = bsp::Bsp::parse(bsp_data);

    // Camera: use first BSP camera spot if available, else center of vertex bounds
    let bbox_size = Vec3::new(
        bsp.bbox_max[0] - bsp.bbox_min[0],
        bsp.bbox_max[1] - bsp.bbox_min[1],
        bsp.bbox_max[2] - bsp.bbox_min[2],
    );
    let diagonal = bbox_size.length();
    let cam_speed = (diagonal * 0.15).max(10.0);

    let (start_pos, start_yaw, start_pitch) = if let Some(spot) = bsp.camera_spots.first() {
        let pos = Vec3::from(spot.pos);
        let dir = Vec3::from(spot.dir);
        let yaw = dir.z.atan2(dir.x);
        let pitch = dir.y.asin();
        println!(
            "Using camera spot: pos=({:.1},{:.1},{:.1}) dir=({:.2},{:.2},{:.2})",
            pos.x, pos.y, pos.z, dir.x, dir.y, dir.z,
        );
        (pos, yaw, pitch)
    } else {
        let center = Vec3::new(
            (bsp.bbox_min[0] + bsp.bbox_max[0]) * 0.5,
            bsp.bbox_min[1] + (bsp.bbox_max[1] - bsp.bbox_min[1]) * 0.3,
            (bsp.bbox_min[2] + bsp.bbox_max[2]) * 0.5,
        );
        (center, 0.0, 0.0)
    };

    println!(
        "BSP bounds: ({:.0},{:.0},{:.0}) to ({:.0},{:.0},{:.0}), diagonal {:.0}",
        bsp.bbox_min[0], bsp.bbox_min[1], bsp.bbox_min[2],
        bsp.bbox_max[0], bsp.bbox_max[1], bsp.bbox_max[2],
        diagonal,
    );
    println!(
        "Camera at ({:.0}, {:.0}, {:.0}), yaw={:.2} pitch={:.2}, speed {:.0} u/s",
        start_pos.x, start_pos.y, start_pos.z, start_yaw, start_pitch, cam_speed,
    );

    // SDL2 init
    let sdl = sdl2::init().unwrap();
    let video = sdl.video().unwrap();

    let gl_attr = video.gl_attr();
    gl_attr.set_context_profile(sdl2::video::GLProfile::Core);
    gl_attr.set_context_version(3, 3);
    gl_attr.set_depth_size(24);
    gl_attr.set_stencil_size(8);

    let window = video
        .window("七夜 Remaster", 1280, 720)
        .opengl()
        .resizable()
        .build()
        .unwrap();

    let _gl_context = window.gl_create_context().unwrap();
    gl::load_with(|s| video.gl_get_proc_address(s) as *const _);

    unsafe {
        gl::Enable(gl::DEPTH_TEST);
        // Disable backface culling for now — BSP face winding needs investigation
        gl::Disable(gl::CULL_FACE);
        gl::ClearColor(0.1, 0.1, 0.15, 1.0);
    }

    // Build renderer
    let renderer = render::BspRenderer::new(&bsp, &fs);

    let mut event_pump = sdl.event_pump().unwrap();

    // Enable relative mouse after event pump exists, then flush stale events
    sdl.mouse().set_relative_mouse_mode(true);
    event_pump.poll_iter().for_each(drop);

    let mut camera = Camera::new(start_pos, cam_speed);
    camera.yaw = start_yaw;
    camera.pitch = start_pitch;
    let mut last_time = std::time::Instant::now();
    let mut first_mouse = true;

    'main: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => break 'main,
                Event::KeyDown {
                    scancode: Some(Scancode::Escape),
                    ..
                } => break 'main,
                Event::MouseMotion { xrel, yrel, .. } => {
                    if first_mouse {
                        first_mouse = false;
                        continue;
                    }
                    camera.yaw += xrel as f32 * 0.003;
                    camera.pitch -= yrel as f32 * 0.003;
                    camera.pitch = camera.pitch.clamp(-1.5, 1.5);
                }
                _ => {}
            }
        }

        let now = std::time::Instant::now();
        let dt = (now - last_time).as_secs_f32().min(0.1);
        last_time = now;

        let keys = event_pump.keyboard_state();
        camera.update(&keys, dt);

        let (w, h) = window.size();
        if w > 0 && h > 0 {
            let aspect = w as f32 / h as f32;
            let proj = Mat4::perspective_lh(70.0_f32.to_radians(), aspect, 0.1, 10000.0);
            let view = camera.view_matrix();
            let mvp = proj * view;

            unsafe {
                gl::Viewport(0, 0, w as i32, h as i32);
                gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
            }

            renderer.render(&mvp);
        }

        window.gl_swap_window();
    }
}
