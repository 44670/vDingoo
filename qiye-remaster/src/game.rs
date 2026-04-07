use crate::audio::AudioSystem;
use crate::bsp;
use crate::camera::{Camera, CameraMode};
use crate::enemy::{EnemyManager, EnemyType};
use crate::game_data::GameData;
use crate::entity::{EntityManager, EntityTypeId};
use crate::fs_app::AppFs;
use crate::input::{Action, InputState};
use crate::model::SojModel;
use crate::model_render::ModelRenderer;
use crate::player::Player;
use crate::render;
use crate::script::{self, ScriptEngine, ScriptState};
use crate::time::FrameTimer;
use crate::trigger::TriggerSystem;
use crate::ui::{DialogBox, UiRenderer};
use glam::{Mat4, Vec3};

pub struct Game {
    fs: AppFs,
    bsp: bsp::Bsp,  // stored for collision
    bsp_renderer: render::BspRenderer,
    debug_renderer: render::DebugRenderer,
    model_renderer: ModelRenderer,
    ui_renderer: UiRenderer,
    audio: AudioSystem,
    dialog: DialogBox,
    entities: EntityManager,
    triggers: TriggerSystem,
    enemies: EnemyManager,
    game_data: GameData,
    camera: Camera,
    timer: FrameTimer,
    input: InputState,
    scripts: Vec<ScriptEngine>,
    player: Option<Player>,
    player_mode: bool,    // true = third-person player, false = free-cam
    first_mouse: bool,
    show_entities: bool,
    // Map list for switching
    bsp_files: Vec<String>,
    current_map_idx: usize,
}

impl Game {
    pub fn new(fs: AppFs, bsp: bsp::Bsp, bsp_path: &str, audio: AudioSystem) -> Self {
        // Build sorted list of all BSP files
        let mut bsp_files: Vec<String> = fs
            .list_files()
            .filter(|p| p.ends_with(".sbp"))
            .map(|p| p.to_string())
            .collect();
        bsp_files.sort();
        let current_map_idx = bsp_files
            .iter()
            .position(|p| p == bsp_path)
            .unwrap_or(0);

        let debug_renderer = render::DebugRenderer::new();
        let mut model_renderer = ModelRenderer::new();

        let (bsp_renderer, entities, mut camera, scripts) =
            Self::load_map_data(&fs, &bsp, bsp_path, &mut model_renderer);

        Self::load_entity_models(&mut model_renderer, &entities, &fs);

        let mut triggers = TriggerSystem::new();
        triggers.load_from_entities(&entities);

        // Find player spawn point
        let player = Self::create_player(&entities);

        // Spawn enemies from BSP entities
        let enemies = Self::create_enemies(&entities);

        let ui_renderer = UiRenderer::new();
        let dialog = DialogBox::new();

        // Default to 3rd person if we have a player
        if let Some(ref p) = player {
            camera.set_follow_mode(
                p.pos + Vec3::new(0.0, 2.0, 0.0),
                Vec3::new(0.0, 5.0, -10.0),
            );
        }

        Game {
            fs,
            bsp,
            bsp_renderer,
            debug_renderer,
            model_renderer,
            ui_renderer,
            audio,
            dialog,
            entities,
            triggers,
            enemies,
            game_data: GameData::new(),
            camera,
            player,
            player_mode: true,
            timer: FrameTimer::new(),
            input: InputState::new(),
            scripts,
            first_mouse: true,
            show_entities: true,
            bsp_files,
            current_map_idx,
        }
    }

    fn load_map_data(
        fs: &AppFs,
        bsp: &bsp::Bsp,
        bsp_path: &str,
        _model_renderer: &mut ModelRenderer,
    ) -> (render::BspRenderer, EntityManager, Camera, Vec<ScriptEngine>) {
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

        let bsp_renderer = render::BspRenderer::new(bsp, fs);

        let mut entities = EntityManager::new();
        entities.load_from_bsp(bsp);

        let mut camera = Camera::new(start_pos, cam_speed);
        camera.yaw = start_yaw;
        camera.pitch = start_pitch;

        let scripts = Self::load_scripts(fs, bsp_path);

        (bsp_renderer, entities, camera, scripts)
    }

    fn load_entity_models(model_renderer: &mut ModelRenderer, entities: &EntityManager, fs: &AppFs) {
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
                        if model_renderer.upload_model(model_name, &soj, fs) {
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
        // Try loading animations for each model
        let mut anims = 0;
        for ent in entities.entities() {
            if let Some(ref model_name) = ent.model_name {
                if model_renderer.load_animation(model_name, fs) {
                    anims += 1;
                }
            }
        }
        if loaded > 0 || failed > 0 {
            println!("Models: {loaded} loaded, {failed} failed, {anims} with animations");
        }
    }

    fn switch_map(&mut self, new_idx: usize) {
        if new_idx >= self.bsp_files.len() {
            return;
        }
        self.current_map_idx = new_idx;
        let bsp_path = self.bsp_files[new_idx].clone();

        println!("\n========================================");
        println!("Loading map [{}/{}]: {bsp_path}", new_idx + 1, self.bsp_files.len());
        println!("========================================");

        let bsp_data = match self.fs.read(&bsp_path) {
            Some(d) => d,
            None => {
                eprintln!("Failed to read BSP: {bsp_path}");
                return;
            }
        };
        let bsp = bsp::Bsp::parse(bsp_data);

        let (bsp_renderer, entities, camera, scripts) =
            Self::load_map_data(&self.fs, &bsp, &bsp_path, &mut self.model_renderer);

        Self::load_entity_models(&mut self.model_renderer, &entities, &self.fs);

        self.audio.stop_all();
        self.bsp = bsp;
        self.bsp_renderer = bsp_renderer;
        self.entities = entities;
        self.triggers = TriggerSystem::new();
        self.triggers.load_from_entities(&self.entities);
        self.player = Self::create_player(&self.entities);
        self.enemies = Self::create_enemies(&self.entities);
        self.camera = camera;
        self.scripts = scripts;
        self.first_mouse = true;
        self.player_mode = true;
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
            // Toggle player mode with P key
            if self.input.just_pressed(Action::Cancel) && self.player.is_some() {
                self.player_mode = !self.player_mode;
                if self.player_mode {
                    let player = self.player.as_ref().unwrap();
                    self.camera.set_follow_mode(
                        player.pos,
                        Vec3::new(0.0, 5.0, -10.0),
                    );
                    println!("Player mode: ON (third-person)");
                } else {
                    self.camera.set_free_mode();
                    println!("Player mode: OFF (free-cam)");
                }
            }

            // Update player
            if self.player_mode {
                if let Some(ref mut player) = self.player {
                    player.update(&self.input, self.camera.yaw, self.timer.dt, Some(&self.bsp));
                    self.camera.update_follow_target(player.pos + Vec3::new(0.0, 2.0, 0.0));
                }
            }

            self.camera.update(&self.input, self.timer.dt);
            self.model_renderer.update_animations(self.timer.dt);

            // Update enemies
            if self.player_mode {
                let player_pos = self.player.as_ref().map_or(Vec3::ZERO, |p| p.pos);
                let player_alive = self.player.as_ref().map_or(false, |p| !p.dead);
                self.enemies.update(player_pos, player_alive, self.timer.dt);

                // Combat: player attacks hit enemies
                if let Some(ref player) = self.player {
                    if player.is_attacking() {
                        let attack_dir = Vec3::new(player.facing_yaw.cos(), 0.0, player.facing_yaw.sin());
                        for enemy in &mut self.enemies.enemies {
                            if !enemy.active { continue; }
                            let dist = (enemy.pos - player.pos).length();
                            if dist < 3.0 {
                                let to_enemy = (enemy.pos - player.pos).normalize();
                                let dot = attack_dir.dot(to_enemy);
                                if dot > 0.3 {
                                    enemy.take_damage(10, to_enemy);
                                }
                            }
                        }
                    }
                }

                // Combat: enemies hit player
                if let Some(ref mut player) = self.player {
                    for enemy in &self.enemies.enemies {
                        if enemy.can_hit_player(player.pos) {
                            let dir = (player.pos - enemy.pos).normalize();
                            player.take_damage(enemy.damage, dir);
                        }
                    }
                }
            }

            // Map switching: PageDown = next, PageUp = prev
            if self.input.just_pressed(Action::NextMap) {
                let next = (self.current_map_idx + 1) % self.bsp_files.len();
                self.switch_map(next);
                continue;
            }
            if self.input.just_pressed(Action::PrevMap) {
                let prev = if self.current_map_idx == 0 {
                    self.bsp_files.len() - 1
                } else {
                    self.current_map_idx - 1
                };
                self.switch_map(prev);
                continue;
            }

            // Check triggers against player or camera position
            let check_pos = if self.player_mode {
                self.player.as_ref().map_or(self.camera.pos, |p| p.pos)
            } else {
                self.camera.pos
            };
            let trigger_events = self.triggers.check(check_pos);
            for event in &trigger_events {
                println!(
                    "Trigger {:?} {} (idx={})",
                    event.type_id,
                    if event.entered { "ENTER" } else { "EXIT" },
                    event.trigger_idx,
                );

                if event.entered {
                    match event.type_id {
                        EntityTypeId::TalkTrigger | EntityTypeId::EventTrigger => {
                            // Find and activate a script matching this trigger
                            // For now, activate the next idle script
                            for script in &mut self.scripts {
                                if script.state == ScriptState::Done {
                                    script.reset();
                                    break;
                                }
                            }
                        }
                        EntityTypeId::CameraTrigger => {
                            // Switch to follow mode if player exists
                            if self.player_mode {
                                if let Some(ref player) = self.player {
                                    self.camera.set_follow_mode(
                                        player.pos,
                                        Vec3::new(0.0, 5.0, -10.0),
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Dialog handling
            self.dialog.update(self.timer.dt);
            if self.input.just_pressed(Action::Confirm) {
                if self.dialog.visible {
                    if self.dialog.is_complete() {
                        self.dialog.dismiss();
                    } else {
                        self.dialog.skip_to_end();
                    }
                } else {
                    // Script stepping: Enter = step one command
                    let mut stepped_cmd = None;
                    for (i, script) in self.scripts.iter_mut().enumerate() {
                        if script.state == ScriptState::Running {
                            if let Some(cmd) = script.step() {
                                let name = script::command_name(cmd.id);
                                let args_str: Vec<String> =
                                    cmd.args.iter().map(|a| a.to_string()).collect();
                                println!("SST[{i}] [{:3}] {name}({})", cmd.id, args_str.join(", "));
                                stepped_cmd = Some((cmd.id, cmd.args.clone()));
                            }
                            break;
                        }
                    }
                    if let Some((id, args)) = stepped_cmd {
                        self.handle_command(id, &args);
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

            // Auto-run scripts (one command per frame, pause during dialog)
            if !self.dialog.visible {
                let mut auto_cmd = None;
                for (i, script) in self.scripts.iter_mut().enumerate() {
                    if script.state == ScriptState::Running {
                        if let Some(cmd) = script.step() {
                            let name = script::command_name(cmd.id);
                            let args_str: Vec<String> =
                                cmd.args.iter().map(|a| a.to_string()).collect();
                            println!("AUTO[{i}] [{:3}] {name}({})", cmd.id, args_str.join(", "));
                            auto_cmd = Some((cmd.id, cmd.args.clone()));
                        }
                        break;
                    }
                }
                if let Some((id, args)) = auto_cmd {
                    self.handle_command(id, &args);
                }
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

                // Render entity models (with animation if available)
                for ent in self.entities.entities() {
                    if !ent.active {
                        continue;
                    }
                    if let Some(ref model_name) = ent.model_name {
                        let model_mat = if let Some(anim_mat) = self.model_renderer.get_anim_matrix(model_name) {
                            ent.transform.matrix * anim_mat
                        } else {
                            ent.transform.matrix
                        };
                        self.model_renderer.render(model_name, &model_mat, &mvp);
                    }
                }

                // Render player model
                if let Some(ref player) = self.player {
                    if let Some(ref name) = player.model_name {
                        let rot = Mat4::from_rotation_y(player.facing_yaw);
                        let trans = Mat4::from_translation(player.pos);
                        let model_mat = trans * rot;
                        let model_mat = if let Some(anim_mat) = self.model_renderer.get_anim_matrix(name) {
                            model_mat * anim_mat
                        } else {
                            model_mat
                        };
                        self.model_renderer.render(name, &model_mat, &mvp);
                    }
                }

                // Render enemies
                for enemy in &self.enemies.enemies {
                    if !enemy.active { continue; }
                    if let Some(ref name) = enemy.model_name {
                        let rot = Mat4::from_rotation_y(enemy.facing_yaw);
                        let trans = Mat4::from_translation(enemy.pos);
                        let model_mat = trans * rot;
                        self.model_renderer.render(name, &model_mat, &mvp);
                    }
                }

                if self.show_entities {
                    self.debug_renderer.render_entities(&mvp, &self.entities);
                    self.debug_renderer.render_triggers(&mvp, &self.triggers);
                }

                // UI overlay
                let sw = w as f32;
                let sh = h as f32;

                // HUD: map name + day
                let map_name = &self.bsp_files[self.current_map_idx];
                let hud = format!(
                    "Day {} | [{}/{}] {}",
                    self.game_data.day,
                    self.current_map_idx + 1,
                    self.bsp_files.len(),
                    map_name
                );
                self.ui_renderer.draw_text(&hud, 5.0, 5.0, 1.5, [0.8, 0.8, 0.8, 0.8], sw, sh);

                // HUD: player info
                if let Some(ref player) = self.player {
                    let mode_str = if self.player_mode { "PLAYER" } else { "FREE-CAM" };
                    let enemy_count = self.enemies.active_count();
                    let player_hud = format!(
                        "{} | HP:{}/{} | {} | Enemies:{}",
                        mode_str, player.hp, player.max_hp, player.state_name(), enemy_count
                    );
                    self.ui_renderer.draw_text(&player_hud, 5.0, 22.0, 1.5, [0.5, 1.0, 0.5, 0.9], sw, sh);

                    // Health bar
                    if self.player_mode {
                        let bar_w = 200.0;
                        let bar_h = 8.0;
                        let bar_x = 5.0;
                        let bar_y = 42.0;
                        let hp_frac = player.hp as f32 / player.max_hp as f32;
                        // Background
                        self.ui_renderer.draw_rect(bar_x, bar_y, bar_w, bar_h,
                            [0.2, 0.0, 0.0, 0.7], sw, sh);
                        // Health fill
                        let color = if hp_frac > 0.5 {
                            [0.1, 0.8, 0.1, 0.9]
                        } else if hp_frac > 0.25 {
                            [0.9, 0.7, 0.1, 0.9]
                        } else {
                            [0.9, 0.1, 0.1, 0.9]
                        };
                        self.ui_renderer.draw_rect(bar_x, bar_y, bar_w * hp_frac, bar_h,
                            color, sw, sh);
                    }
                }

                // Dialog box
                self.dialog.render(&self.ui_renderer, sw, sh);
            }

            window.gl_swap_window();
        }
    }

    /// Handle a script command — dispatch to audio, dialog, etc.
    fn handle_command(&mut self, cmd_id: u16, args: &[script::ArgValue]) {
        match cmd_id {
            // PlayBgm(sound_id, loop_flag)
            41 => {
                if let Some(script::ArgValue::Int(id)) = args.first() {
                    let name = format!("{id}.sau");
                    let looping = args.get(1).map_or(true, |a| {
                        matches!(a, script::ArgValue::Int(v) if *v != 0)
                    });
                    if self.audio.load_clip(&name, &self.fs) {
                        self.audio.play(&name, looping, 0.8);
                        println!("Audio: PlayBgm {name} (loop={looping})");
                    }
                }
            }
            // StopBgm
            42 => {
                self.audio.stop_all();
                println!("Audio: StopBgm");
            }
            // PlaySfx(sound_id)
            45 => {
                if let Some(script::ArgValue::Int(id)) = args.first() {
                    let name = format!("{id}.sau");
                    if self.audio.load_clip(&name, &self.fs) {
                        self.audio.play(&name, false, 1.0);
                        println!("Audio: PlaySfx {name}");
                    }
                }
            }
            // StopAllSounds
            46 => {
                self.audio.stop_all();
                println!("Audio: StopAllSounds");
            }
            // PlaySfx3D(sound_id, x, y, z) — play as 2D for now
            47 => {
                if let Some(script::ArgValue::Int(id)) = args.first() {
                    let name = format!("{id}.sau");
                    if self.audio.load_clip(&name, &self.fs) {
                        self.audio.play(&name, false, 0.6);
                        println!("Audio: PlaySfx3D {name}");
                    }
                }
            }
            // FreeAllSounds
            48 => {
                self.audio.stop_all();
                println!("Audio: FreeAllSounds");
            }
            // SetPlayerPosition(x, y, z)
            78 => {
                if args.len() >= 3 {
                    let x = match &args[0] { script::ArgValue::Fixed(v) => *v, script::ArgValue::Int(v) => *v as f32, _ => return };
                    let y = match &args[1] { script::ArgValue::Fixed(v) => *v, script::ArgValue::Int(v) => *v as f32, _ => return };
                    let z = match &args[2] { script::ArgValue::Fixed(v) => *v, script::ArgValue::Int(v) => *v as f32, _ => return };
                    if let Some(ref mut player) = self.player {
                        player.pos = Vec3::new(x, y, z);
                        println!("Player: SetPosition({x:.1}, {y:.1}, {z:.1})");
                    }
                }
            }
            // ShowDialog(text)
            30 => {
                if let Some(script::ArgValue::Str(text)) = args.first() {
                    self.dialog.show(text);
                    println!("Dialog: \"{text}\"");
                }
            }
            // SetGameDataFlag(idx)
            50 => {
                if let Some(script::ArgValue::Int(idx)) = args.first() {
                    self.game_data.set_flag(*idx);
                    println!("GameData: SetFlag({idx})");
                }
            }
            // GetGameDataFlag(idx) — stores result for script comparison
            51 => {
                if let Some(script::ArgValue::Int(idx)) = args.first() {
                    let val = self.game_data.get_flag(*idx);
                    println!("GameData: GetFlag({idx}) = {val}");
                }
            }
            // ClearGameDataFlag(idx)
            52 => {
                if let Some(script::ArgValue::Int(idx)) = args.first() {
                    self.game_data.clear_flag(*idx);
                    println!("GameData: ClearFlag({idx})");
                }
            }
            // GetDay
            96 => {
                println!("GameData: GetDay() = {}", self.game_data.day);
            }
            // CollectItem(idx)
            212 => {
                if let Some(script::ArgValue::Int(idx)) = args.first() {
                    if self.game_data.collect_item(*idx) {
                        println!("GameData: CollectItem({idx}) — new!");
                    }
                }
            }
            // CheckItemCollected(idx)
            213 => {
                if let Some(script::ArgValue::Int(idx)) = args.first() {
                    let has = self.game_data.has_item(*idx);
                    println!("GameData: CheckItem({idx}) = {has}");
                }
            }
            _ => {}
        }
    }

    fn create_enemies(entities: &EntityManager) -> EnemyManager {
        let mut manager = EnemyManager::new();
        for (i, ent) in entities.entities().iter().enumerate() {
            if ent.type_id == EntityTypeId::Enemy || ent.type_id == EntityTypeId::Creature {
                let enemy_type = EnemyType::Generic;
                manager.spawn(ent.transform.position, enemy_type, i, ent.model_name.clone());
            }
        }
        if manager.active_count() > 0 {
            println!("Enemies: spawned {}", manager.enemies.len());
        }
        manager
    }

    fn create_player(entities: &EntityManager) -> Option<Player> {
        // Find first Player entity spawn point
        for ent in entities.entities() {
            if ent.type_id == EntityTypeId::Player {
                let mut player = Player::new(ent.transform.position);
                player.model_name = ent.model_name.clone();
                println!(
                    "Player spawned at ({:.1}, {:.1}, {:.1})",
                    player.pos.x, player.pos.y, player.pos.z,
                );
                return Some(player);
            }
        }
        None
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
