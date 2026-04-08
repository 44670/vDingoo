use crate::audio::AudioSystem;
use crate::bsp;
use crate::camera::Camera;
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
    /// Start a new game on the given day.
    /// Original flow: GameEngine_startNewGame → prepareEpisode(1,1) → Scene_loadDay(1)
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

        let (bsp_renderer, entities, camera, scripts) =
            Self::load_map_data(&fs, &bsp, bsp_path, &mut model_renderer);

        Self::load_entity_models(&mut model_renderer, &entities, &fs);

        let mut triggers = TriggerSystem::new();
        triggers.load_from_entities(&entities);

        // --- Scene_resetPlayerState equivalent ---
        // Player always starts at origin, positioned by script or scene transition
        let model_key = "r_ken_a.soj";
        Self::load_model_with_anim(&mut model_renderer, model_key, "r_ken_stand_a.sai", &fs);

        // Determine player start position:
        // 1. Use DummySpot entity (type 11) if available — this is the player spawn point
        // 2. Fall back to camera spot (type 10)
        // 3. Fall back to BSP center
        // Original: player starts at (0,0,0), scripts position later. We pre-position since BSP is pre-loaded.
        let player_pos = if let Some(ent) = bsp.entities.iter().find(|e| e.entity_type == 11) {
            if let Some(pos) = ent.position {
                println!("Player start: using DummySpot (type 11) at ({:.1},{:.1},{:.1})", pos[0], pos[1], pos[2]);
                Vec3::from(pos)
            } else if let Some(spot) = bsp.camera_spots.first() {
                Vec3::from(spot.pos)
            } else {
                Vec3::ZERO
            }
        } else if let Some(spot) = bsp.camera_spots.first() {
            println!("Player start: using camera spot at ({:.1},{:.1},{:.1})", spot.pos[0], spot.pos[1], spot.pos[2]);
            Vec3::from(spot.pos)
        } else {
            Vec3::ZERO
        };

        // Debug: dump BSP tree structure
        {
            println!("=== BSP TREE DUMP ===");
            println!("Nodes: {}, Leaves: {}, Brushes: {}, BrushSides: {}, LeafBrushes: {}",
                bsp.nodes.len(), bsp.leaves.len(), bsp.brushes.len(), bsp.brush_sides.len(), bsp.leaf_brushes.len());
            for (i, node) in bsp.nodes.iter().enumerate() {
                let p = &bsp.planes[node.plane_idx as usize];
                println!("  Node[{i}]: plane={} n=({:.3},{:.3},{:.3}) d={:.3} type={} children=[{},{}]",
                    node.plane_idx, p.normal[0], p.normal[1], p.normal[2], p.dist, p.type_flags,
                    node.children[0], node.children[1]);
            }
            for (i, leaf) in bsp.leaves.iter().enumerate() {
                println!("  Leaf[{i}]: contents={} first_lb={} num_lb={} cluster={}",
                    leaf.contents, leaf.first_leaf_brush, leaf.num_leaf_brushes, leaf.cluster);
            }
            for (i, lb) in bsp.leaf_brushes.iter().enumerate() {
                println!("  LeafBrush[{i}]: brush_idx={lb}");
            }
            for (i, brush) in bsp.brushes.iter().enumerate() {
                let sides: Vec<String> = (0..brush.num_sides as usize).map(|s| {
                    let si = brush.first_side as usize + s;
                    let pi = bsp.brush_sides[si].plane_index;
                    let p = &bsp.planes[pi as usize];
                    format!("s{si}(p{pi} n=({:.2},{:.2},{:.2}) d={:.2})", p.normal[0], p.normal[1], p.normal[2], p.dist)
                }).collect();
                println!("  Brush[{i}]: first_side={} num_sides={} sides=[{}]",
                    brush.first_side, brush.num_sides, sides.join(", "));
            }
            println!("=== END BSP TREE DUMP ===");
        }

        // Debug: test trace at various positions to verify BSP collision works
        {
            let mins = [0.0f32; 3];
            let maxs = [0.0f32; 3];
            let cx = (bsp.bbox_min[0] + bsp.bbox_max[0]) * 0.5;
            let cz = (bsp.bbox_min[2] + bsp.bbox_max[2]) * 0.5;
            // Point trace from BSP top center downward (with verbose debug)
            println!("=== DEBUG TRACE: center ({cx:.1}, {:.1}, {cz:.1}) → ({cx:.1}, {:.1}, {cz:.1}) ===",
                bsp.bbox_max[1], bsp.bbox_min[1]);
            let t1 = bsp.trace_box_debug([cx, bsp.bbox_max[1], cz], [cx, bsp.bbox_min[1], cz], mins, maxs);
            println!("DEBUG trace center ({:.1},{:.1},{:.1})→({:.1},{:.1},{:.1}): hit={} frac={:.3} solid={} n=({:.2},{:.2},{:.2})",
                cx, bsp.bbox_max[1], cz, cx, bsp.bbox_min[1], cz,
                t1.hit, t1.fraction, t1.all_solid, t1.normal[0], t1.normal[1], t1.normal[2]);
            // Point trace from camera spot downward
            let t2 = bsp.trace_box([110.9, -16.0, 110.5], [110.9, -34.0, 110.5], mins, maxs);
            println!("DEBUG trace cam ({:.1},{:.1},{:.1})→({:.1},{:.1},{:.1}): hit={} frac={:.3} solid={}",
                110.9, -16.0, 110.5, 110.9, -34.0, 110.5,
                t2.hit, t2.fraction, t2.all_solid);
            // Point trace from DummySpot downward
            let t3 = bsp.trace_box([92.5, -16.0, 97.5], [92.5, -34.0, 97.5], mins, maxs);
            println!("DEBUG trace dummy ({:.1},{:.1},{:.1})→({:.1},{:.1},{:.1}): hit={} frac={:.3} solid={}",
                92.5, -16.0, 97.5, 92.5, -34.0, 97.5,
                t3.hit, t3.fraction, t3.all_solid);
        }

        let mut player = Player::new(player_pos);
        player.model_name = Some(model_key.to_string());
        // Original: GameUnit_groundCheck snaps player to floor after positioning
        player.ground_snap(&bsp);

        // --- Scene_loadEnemyModels equivalent (day 1) ---
        // Day 1 loads: r_bully_a, r_nvyong_a, r_weed_a
        Self::load_day_enemy_models(&mut model_renderer, 1, &fs);

        let enemies = Self::create_enemies(&entities);

        let ui_renderer = UiRenderer::new();
        let dialog = DialogBox::new();

        // Camera follows player from the start (use ground-snapped position)
        // Original: Camera_initFollowMode in prepareEpisode
        let mut camera = camera;
        camera.set_follow_mode(
            player.pos + Vec3::new(0.0, 2.0, 0.0),
            Vec3::new(0.0, 5.0, -10.0),
        );

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
            player: Some(player),
            player_mode: true,
            timer: FrameTimer::new(),
            input: InputState::new(),
            scripts,
            first_mouse: true,
            show_entities: false,
            bsp_files,
            current_map_idx,
        }
    }

    /// Load a SOJ model and its SAI animation into the renderer.
    fn load_model_with_anim(renderer: &mut ModelRenderer, model: &str, anim: &str, fs: &AppFs) {
        let soj_path = format!(".\\common\\{model}");
        if let Some(data) = fs.read(&soj_path) {
            if let Some(soj) = SojModel::parse(data) {
                if renderer.upload_model(model, &soj, fs) {
                    println!("Model: loaded {model}");
                }
            }
        }
        let sai_path = format!(".\\common\\{anim}");
        if let Some(data) = fs.read(&sai_path) {
            if let Some(anim_data) = crate::animation::SaiAnimation::parse(data) {
                let ctrl = crate::animation::AnimController::new(&anim_data);
                renderer.set_animation(model, anim_data, ctrl);
            }
        }
    }

    /// Preload enemy models for a specific day.
    /// Original: Scene_loadEnemyModels with day-based switch.
    fn load_day_enemy_models(renderer: &mut ModelRenderer, day: i32, fs: &AppFs) {
        let models: &[(&str, &str)] = match day {
            1 => &[
                ("r_bully_a.soj", "r_bully_a.sai"),
                ("r_nvyong_a.soj", "r_nvyong_a.sai"),
                ("r_weed_a.soj", "r_weed_a.sai"),
            ],
            2 => &[
                ("r_nvyong_a.soj", "r_nvyong_a.sai"),
                ("r_lamper_a.soj", "r_lamper_a.sai"),
                ("r_weed_a.soj", "r_weed_a.sai"),
            ],
            3 => &[
                ("r_nvyong_a.soj", "r_nvyong_a.sai"),
                ("r_weed_a.soj", "r_weed_a.sai"),
                ("r_pangnvyong_a.soj", "r_pangnvyong_a.sai"),
                ("r_nvyong_b.soj", "r_nvyong_b.sai"),
            ],
            4 => &[
                ("r_victor_a.soj", "r_victor_a.sai"),
                ("r_sam_b.soj", "r_sam_b.sai"),
                ("r_bingnvyong_a.soj", "r_bingnvyong_a.sai"),
                ("r_bug_a.soj", "r_bug_a.sai"),
                ("r_qiutu_a.soj", "r_qiutu_a.sai"),
            ],
            5 => &[
                ("r_bully_a.soj", "r_bully_a.sai"),
                ("r_kate_a.soj", "r_kate_a.sai"),
            ],
            _ => &[
                ("r_nvyong_a.soj", "r_nvyong_a.sai"),
            ],
        };
        let mut loaded = 0;
        for (model, anim) in models {
            if !renderer.has_model(model) {
                Self::load_model_with_anim(renderer, model, anim, fs);
                loaded += 1;
            }
        }
        if loaded > 0 {
            println!("Day {day}: preloaded {loaded} enemy models");
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

        let (bsp_renderer, entities, mut camera, scripts) =
            Self::load_map_data(&self.fs, &bsp, &bsp_path, &mut self.model_renderer);

        Self::load_entity_models(&mut self.model_renderer, &entities, &self.fs);

        self.audio.stop_all();
        self.bsp = bsp;
        self.bsp_renderer = bsp_renderer;
        self.entities = entities;
        self.triggers = TriggerSystem::new();
        self.triggers.load_from_entities(&self.entities);
        self.enemies = Self::create_enemies(&self.entities);

        // Reposition player at first camera spot (no Player entities in BSP)
        if let Some(ref mut player) = self.player {
            let new_pos = if let Some(spot) = self.bsp.camera_spots.first() {
                Vec3::from(spot.pos)
            } else {
                Vec3::new(
                    (self.bsp.bbox_min[0] + self.bsp.bbox_max[0]) * 0.5,
                    self.bsp.bbox_min[1] + 1.0,
                    (self.bsp.bbox_min[2] + self.bsp.bbox_max[2]) * 0.5,
                )
            };
            player.pos = new_pos;
            player.velocity = Vec3::ZERO;
            // Original: GameUnit_groundCheck snaps player to floor after positioning
            player.ground_snap(&self.bsp);
            camera.set_follow_mode(
                player.pos + Vec3::new(0.0, 2.0, 0.0),
                Vec3::new(0.0, 5.0, -10.0),
            );
        }

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
                        // Resume scripts that were waiting on dialog
                        for script in &mut self.scripts {
                            if script.state == ScriptState::WaitFrames(u32::MAX) {
                                script.state = ScriptState::Running;
                            }
                        }
                    } else {
                        self.dialog.skip_to_end();
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

            // Auto-run scripts — execute commands until we hit a blocking one
            if !self.dialog.visible {
                let mut cmds_this_frame = 0;
                'script_loop: loop {
                    if cmds_this_frame >= 100 { break; } // safety limit
                    let mut got_cmd = None;
                    for (i, script) in self.scripts.iter_mut().enumerate() {
                        if script.state == ScriptState::Running {
                            if let Some(cmd) = script.step() {
                                let name = script::command_name(cmd.id);
                                let args_str: Vec<String> =
                                    cmd.args.iter().map(|a| a.to_string()).collect();
                                println!("AUTO[{i}] [{:3}] {name}({})", cmd.id, args_str.join(", "));
                                got_cmd = Some((i, cmd.id, cmd.args.clone()));
                            }
                            break;
                        }
                    }
                    match got_cmd {
                        Some((script_idx, id, args)) => {
                            let blocking = self.handle_command(id, &args);
                            cmds_this_frame += 1;
                            if blocking {
                                // Command blocks — stop running more this frame
                                // (e.g., ShowDialog waits for dismiss, WaitFrames pauses)
                                if let Some(script) = self.scripts.get_mut(script_idx) {
                                    if id == 30 || id == 33 || id == 34 {
                                        // Dialog commands — script waits until dialog dismissed
                                        script.state = ScriptState::WaitFrames(u32::MAX);
                                    }
                                }
                                break 'script_loop;
                            }
                        }
                        None => break 'script_loop,
                    }
                }
            } else {
                // Check if dialog was just dismissed — resume script
                // (handled below in dialog update)
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

    /// Extract a numeric value from an ArgValue (int or fixed-point).
    fn arg_f32(arg: &script::ArgValue) -> Option<f32> {
        match arg {
            script::ArgValue::Fixed(v) => Some(*v),
            script::ArgValue::Int(v) => Some(*v as f32),
            _ => None,
        }
    }

    fn arg_i32(arg: &script::ArgValue) -> Option<i32> {
        match arg {
            script::ArgValue::Int(v) => Some(*v),
            script::ArgValue::Fixed(v) => Some(*v as i32),
            _ => None,
        }
    }

    /// Handle a script command. Returns true if the command blocks execution.
    fn handle_command(&mut self, cmd_id: u16, args: &[script::ArgValue]) -> bool {
        match cmd_id {
            // === Input queries (0-6) — no-op in remaster, tree handles result ===
            0..=6 => {}

            // LogMessage(text)
            7 => {
                if let Some(script::ArgValue::Str(text)) = args.first() {
                    println!("Script: ***{text}");
                }
            }

            // SetFogColorTop(r, g, b, frames) / SetFogColorBottom(r, g, b, frames)
            8 | 9 => {
                // Fog color animation — skip for now
            }

            // LoadNextDay
            10 => {
                let next_day = self.game_data.day + 1;
                println!("Script: LoadNextDay → day {next_day}");
                self.game_data.day = next_day;
                // Find first map of next day
                let day_str = format!("day{next_day}");
                if let Some(idx) = self.bsp_files.iter().position(|p| p.contains(&day_str)) {
                    self.switch_map(idx);
                    return true;
                }
            }

            // SetupCamera
            11 => {
                if let Some(ref player) = self.player {
                    self.camera.set_follow_mode(
                        player.pos + Vec3::new(0.0, 2.0, 0.0),
                        Vec3::new(0.0, 5.0, -10.0),
                    );
                }
            }

            // AttachCameraToPlayer
            25 => {
                if let Some(ref player) = self.player {
                    self.camera.set_follow_mode(
                        player.pos + Vec3::new(0.0, 2.0, 0.0),
                        Vec3::new(0.0, 5.0, -10.0),
                    );
                }
            }

            // ShowDialog(text)
            30 => {
                if let Some(script::ArgValue::Str(text)) = args.first() {
                    self.dialog.show(text);
                    println!("Dialog: \"{text}\"");
                    return true; // block until dismissed
                }
            }

            // ShowAutoDialog(text) — auto-dismiss after display
            33 => {
                if let Some(script::ArgValue::Str(text)) = args.first() {
                    self.dialog.show(text);
                    println!("Dialog(auto): \"{text}\"");
                    return true;
                }
            }

            // ShowDialogWithChoice(text, ...) — show dialog, block
            34 => {
                if let Some(script::ArgValue::Str(text)) = args.first() {
                    self.dialog.show(text);
                    println!("Dialog(choice): \"{text}\"");
                    return true;
                }
            }

            // IsDialogActive / CheckDialogDone — query, no-op
            36 | 37 => {}

            // ShowLoadingScreen(mode) — no-op
            38 => {}

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
            }
            // PlaySfx(sound_id)
            45 => {
                if let Some(script::ArgValue::Int(id)) = args.first() {
                    let name = format!("{id}.sau");
                    if self.audio.load_clip(&name, &self.fs) {
                        self.audio.play(&name, false, 1.0);
                    }
                }
            }
            // StopAllSounds / FreeAllSounds
            46 | 48 => {
                self.audio.stop_all();
            }
            // PlaySfx3D(sound_id, x, y, z)
            47 => {
                if let Some(script::ArgValue::Int(id)) = args.first() {
                    let name = format!("{id}.sau");
                    if self.audio.load_clip(&name, &self.fs) {
                        self.audio.play(&name, false, 0.6);
                    }
                }
            }

            // SetSceneFlag(idx)
            49 => {}

            // SetGameDataFlag(idx)
            50 => {
                if let Some(script::ArgValue::Int(idx)) = args.first() {
                    self.game_data.set_flag(*idx);
                }
            }
            // GetGameDataFlag(idx)
            51 => {
                if let Some(script::ArgValue::Int(idx)) = args.first() {
                    let _val = self.game_data.get_flag(*idx);
                }
            }
            // ClearGameDataFlag(idx)
            52 => {
                if let Some(script::ArgValue::Int(idx)) = args.first() {
                    self.game_data.clear_flag(*idx);
                }
            }

            // SetMenuField / SetMenuMode / SetMenuLayout — no-op
            61..=63 => {}

            // GetPlayerPos/Rot — query, no-op (tree handles result)
            64..=67 => {}

            // CountActiveEntities / CountCollectedItems — query
            68 | 69 => {}

            // PausePlayer(flag)
            73 => {}

            // SetPlayerPosition(x, y, z)
            78 => {
                if args.len() >= 3 {
                    if let (Some(x), Some(y), Some(z)) = (
                        Self::arg_f32(&args[0]),
                        Self::arg_f32(&args[1]),
                        Self::arg_f32(&args[2]),
                    ) {
                        if let Some(ref mut player) = self.player {
                            player.pos = Vec3::new(x, y, z);
                            println!("Player: SetPosition({x:.1}, {y:.1}, {z:.1})");
                        }
                    }
                }
            }

            // CheckAllEnemiesDead — query
            93 => {}

            // GetDay — query
            96 => {}

            // ResetSceneState / IsSceneReady / SetSceneVar — no-op for now
            99..=101 => {}

            // SetPlayerModel(name)
            103 => {
                if let Some(script::ArgValue::Str(name)) = args.first() {
                    // Load model if not loaded
                    let path = format!(".\\common\\{name}");
                    if !self.model_renderer.has_model(name) {
                        if let Some(data) = self.fs.read(&path) {
                            if let Some(soj) = SojModel::parse(data) {
                                self.model_renderer.upload_model(name, &soj, &self.fs);
                            }
                        }
                        self.model_renderer.load_animation(name, &self.fs);
                    }
                    if let Some(ref mut player) = self.player {
                        player.model_name = Some(name.clone());
                        println!("Player: SetModel({name})");
                    }
                }
            }

            // IsPlayerAlive — query
            104 => {}

            // ResetScene
            105 => {}

            // SetEntityVisible(entity_idx, visible)
            111 => {
                if args.len() >= 2 {
                    if let (Some(idx), Some(vis)) = (Self::arg_i32(&args[0]), Self::arg_i32(&args[1])) {
                        self.entities.set_active(idx as usize, vis != 0);
                    }
                }
            }

            // SetEntityState / SetEntityAnimation / SetEntityAnimLoop — stub
            117 | 127 | 128 => {}

            // SetTalkTriggerActive(trigger_idx, active)
            115 => {}

            // EnableTrigger(idx, enabled)
            135 => {}

            // RemoveEntity(idx)
            136 => {
                if let Some(idx) = args.first().and_then(Self::arg_i32) {
                    self.entities.set_active(idx as usize, false);
                }
            }

            // SpawnEnemy(type_id, x, y, z, ...) — stub
            137 => {}

            // RemoveAllEnemies
            156 => {
                self.enemies.remove_all();
            }

            // SetStayBoxBounds / AddStayBoxExit / AddStayBoxEntry — collision areas, stub
            159 | 161..=165 => {}

            // StayBox setup
            160 => {}

            // FadeScreen(mode)
            216 => {
                // Fade effect — skip for now
            }

            // CollectItem(idx)
            212 => {
                if let Some(script::ArgValue::Int(idx)) = args.first() {
                    self.game_data.collect_item(*idx);
                }
            }
            // CheckItemCollected(idx)
            213 => {}

            // SpawnEntityWithParams (221) — complex entity spawn
            221 => {
                // Extract entity model name and position from args
                if args.len() >= 7 {
                    if let Some(script::ArgValue::Str(model)) = args.get(3) {
                        let x = args.get(4).and_then(Self::arg_f32).unwrap_or(0.0);
                        let y = args.get(5).and_then(Self::arg_f32).unwrap_or(0.0);
                        let z = args.get(6).and_then(Self::arg_f32).unwrap_or(0.0);
                        println!("Script: SpawnEntity \"{model}\" at ({x:.1}, {y:.1}, {z:.1})");
                        // Load model if needed
                        let path = format!(".\\common\\{model}");
                        if !self.model_renderer.has_model(model) {
                            if let Some(data) = self.fs.read(&path) {
                                if let Some(soj) = SojModel::parse(data) {
                                    self.model_renderer.upload_model(model, &soj, &self.fs);
                                }
                            }
                            self.model_renderer.load_animation(model, &self.fs);
                        }
                    }
                }
            }

            // SetEntityFullParams (121) — complex entity setup
            121 => {}

            // All other commands — log but don't block
            _ => {}
        }
        false // non-blocking by default
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
                let mut engine = ScriptEngine::parse_sst(data);
                // Only auto-run ep0.sst (scene setup script).
                // Other scripts (epXXXX.sst) are triggered by gameplay events.
                let is_ep0 = path.contains("ep0.sst");
                if !is_ep0 {
                    engine.state = ScriptState::Done;
                }
                let state_str = if is_ep0 { "AUTO" } else { "idle" };
                println!("SST: {path} — {} commands [{state_str}]", engine.command_count());
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
