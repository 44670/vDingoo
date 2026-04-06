use crate::bsp;
use crate::entity::EntityManager;
use crate::fs_app::AppFs;
use crate::input::{Action, InputState};
use crate::model::SojModel;
use crate::model_render::ModelRenderer;
use crate::render;
use crate::script::{self, ScriptEngine, ScriptState};
use crate::time::FrameTimer;
use glam::{Mat4, Vec3};

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

    fn look_dir(&self) -> Vec3 {
        Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        )
    }

    fn forward_xz(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin()).normalize()
    }

    fn right_xz(&self) -> Vec3 {
        Vec3::new(self.yaw.sin(), 0.0, -self.yaw.cos())
    }

    fn view_matrix(&self) -> Mat4 {
        Mat4::look_at_lh(self.pos, self.pos + self.look_dir(), Vec3::Y)
    }

    fn update(&mut self, input: &InputState, dt: f32) {
        let fwd = self.forward_xz();
        let right = self.right_xz();
        let mut vel = Vec3::ZERO;

        if input.is_down(Action::MoveForward) {
            vel += fwd;
        }
        if input.is_down(Action::MoveBack) {
            vel -= fwd;
        }
        if input.is_down(Action::MoveRight) {
            vel += right;
        }
        if input.is_down(Action::MoveLeft) {
            vel -= right;
        }
        if input.is_down(Action::MoveUp) {
            vel += Vec3::Y;
        }
        if input.is_down(Action::MoveDown) {
            vel -= Vec3::Y;
        }

        let speed = if input.is_down(Action::Sprint) {
            self.speed * 3.0
        } else {
            self.speed
        };

        if vel.length_squared() > 0.0 {
            self.pos += vel.normalize() * speed * dt;
        }

        // Mouse look
        self.yaw += input.mouse_dx * 0.003;
        self.pitch -= input.mouse_dy * 0.003;
        self.pitch = self.pitch.clamp(-1.5, 1.5);
    }
}

pub struct Game {
    fs: AppFs,
    bsp_renderer: render::BspRenderer,
    debug_renderer: render::DebugRenderer,
    model_renderer: ModelRenderer,
    entities: EntityManager,
    camera: Camera,
    timer: FrameTimer,
    input: InputState,
    scripts: Vec<ScriptEngine>,
    first_mouse: bool,
    show_entities: bool,
}

impl Game {
    pub fn new(fs: AppFs, bsp: &bsp::Bsp, bsp_path: &str) -> Self {
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

        let bsp_renderer = render::BspRenderer::new(bsp, &fs);
        let debug_renderer = render::DebugRenderer::new();
        let mut model_renderer = ModelRenderer::new();

        let mut entities = EntityManager::new();
        entities.load_from_bsp(bsp);

        // Load SOJ models for entities that have them
        let mut loaded = 0;
        let mut failed = 0;
        for ent in entities.entities() {
            if let Some(ref model_name) = ent.model_name {
                if model_renderer.has_model(model_name) {
                    continue;
                }
                let path = format!(".\\common\\{}", model_name);
                if let Some(data) = fs.read(&path) {
                    if let Some(soj) = SojModel::parse(data) {
                        if model_renderer.upload_model(model_name, &soj, &fs) {
                            loaded += 1;
                        } else {
                            failed += 1;
                        }
                    } else {
                        failed += 1;
                    }
                } else {
                    failed += 1;
                }
            }
        }
        if loaded > 0 || failed > 0 {
            println!("Models: {loaded} loaded, {failed} failed");
        }

        let mut camera = Camera::new(start_pos, cam_speed);
        camera.yaw = start_yaw;
        camera.pitch = start_pitch;

        // Load SST scripts for the same day directory
        let scripts = Self::load_scripts(&fs, bsp_path);

        Game {
            fs,
            bsp_renderer,
            debug_renderer,
            model_renderer,
            entities,
            camera,
            timer: FrameTimer::new(),
            input: InputState::new(),
            scripts,
            first_mouse: true,
            show_entities: true,
        }
    }

    pub fn run(
        &mut self,
        window: &sdl2::video::Window,
        event_pump: &mut sdl2::EventPump,
    ) {
        'main: loop {
            self.input.begin_frame();

            for event in event_pump.poll_iter() {
                // Skip first mouse event (SDL2 sends bogus absolute delta)
                if self.first_mouse {
                    if matches!(event, sdl2::event::Event::MouseMotion { .. }) {
                        self.first_mouse = false;
                        continue;
                    }
                }
                self.input.handle_event(&event);
            }

            if self.input.quit {
                break 'main;
            }

            let keys = event_pump.keyboard_state();
            self.input.update_from_keyboard(&keys);

            self.timer.update();
            self.camera.update(&self.input, self.timer.dt);

            // Script stepping: Enter = step one command, Tab = dump all remaining
            if self.input.just_pressed(Action::Confirm) {
                for (i, script) in self.scripts.iter_mut().enumerate() {
                    if script.state == ScriptState::Running {
                        if let Some(cmd) = script.step() {
                            let name = script::command_name(cmd.id);
                            let args: Vec<String> =
                                cmd.args.iter().map(|a| a.to_string()).collect();
                            println!("SST[{i}] [{:3}] {name}({})", cmd.id, args.join(", "));
                        }
                        break;
                    }
                }
            }
            if self.input.just_pressed(Action::MenuToggle) {
                println!("=== Dumping all SST commands ===");
                for (i, script) in self.scripts.iter_mut().enumerate() {
                    println!("--- SST[{i}] ---");
                    script.reset();
                    while script.state == ScriptState::Running {
                        if let Some(cmd) = script.step() {
                            let name = script::command_name(cmd.id);
                            let args: Vec<String> =
                                cmd.args.iter().map(|a| a.to_string()).collect();
                            println!("  [{:3}] {name}({})", cmd.id, args.join(", "));
                        }
                    }
                }
            }
            // Toggle entity debug display with F key
            if self.input.just_pressed(Action::Attack) {
                self.show_entities = !self.show_entities;
                println!("Entity debug: {}", if self.show_entities { "ON" } else { "OFF" });
            }

            // Update script wait states
            for script in &mut self.scripts {
                script.update();
            }

            let (w, h) = window.size();
            if w > 0 && h > 0 {
                let aspect = w as f32 / h as f32;
                let proj = Mat4::perspective_lh(70.0_f32.to_radians(), aspect, 0.1, 10000.0);
                let view = self.camera.view_matrix();
                let mvp = proj * view;

                unsafe {
                    gl::Viewport(0, 0, w as i32, h as i32);
                    gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
                }

                self.bsp_renderer.render(&mvp);

                // Render entity models
                for ent in self.entities.entities() {
                    if !ent.active {
                        continue;
                    }
                    if let Some(ref model_name) = ent.model_name {
                        self.model_renderer.render(model_name, &ent.transform.matrix, &mvp);
                    }
                }

                if self.show_entities {
                    self.debug_renderer.render_entities(&mvp, &self.entities);
                }
            }

            window.gl_swap_window();
        }
    }

    fn load_scripts(fs: &AppFs, bsp_path: &str) -> Vec<ScriptEngine> {
        // Extract day directory from BSP path (e.g., ".\day1\0101.sbp" → "day1")
        let day_prefix = bsp_path
            .replace('/', "\\")
            .split('\\')
            .nth(1)
            .unwrap_or("day1")
            .to_string();

        let mut sst_files: Vec<String> = fs
            .list_files()
            .filter(|p| p.contains(&day_prefix) && p.ends_with(".sst"))
            .map(|p| p.to_string())
            .collect();
        sst_files.sort();

        let mut scripts = Vec::new();
        for path in &sst_files {
            if let Some(data) = fs.read(path) {
                let engine = ScriptEngine::parse_sst(data);
                println!("SST: {path} — {} commands", engine.command_count());
                scripts.push(engine);
            }
        }
        println!("Loaded {} SST scripts for {day_prefix}", scripts.len());
        scripts
    }

    #[allow(dead_code)]
    pub fn fs(&self) -> &AppFs {
        &self.fs
    }
}
