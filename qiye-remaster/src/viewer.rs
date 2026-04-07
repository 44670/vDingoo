/// Standalone viewer modes: BSP map viewer, SOJ model viewer, NPC/player viewer.

use crate::bsp;
use crate::camera::Camera;
use crate::entity::EntityManager;
use crate::fs_app::AppFs;
use crate::input::{Action, InputState};
use crate::model::SojModel;
use crate::model_render::ModelRenderer;
use crate::player::Player;
use crate::render;
use crate::time::FrameTimer;
use crate::trigger::TriggerSystem;
use crate::ui::UiRenderer;
use glam::{Mat4, Vec3};

// ============================================================================
// BSP Viewer — free-cam fly-through, map cycling, entity debug overlay
// ============================================================================

pub fn run_bsp(
    fs: AppFs,
    bsp: &bsp::Bsp,
    bsp_path: &str,
    window: &sdl2::video::Window,
    event_pump: &mut sdl2::EventPump,
) {
    let debug_renderer = render::DebugRenderer::new();
    let mut model_renderer = ModelRenderer::new();
    let ui_renderer = UiRenderer::new();

    // Build sorted BSP file list
    let mut bsp_files: Vec<String> = fs
        .list_files()
        .filter(|p| p.ends_with(".sbp"))
        .map(|p| p.to_string())
        .collect();
    bsp_files.sort();
    let mut current_map_idx = bsp_files
        .iter()
        .position(|p| p == bsp_path)
        .unwrap_or(0);

    // Load initial map
    let (mut bsp_renderer, mut entities, mut camera, mut triggers) =
        load_bsp_viewer_data(&fs, bsp, bsp_path, &mut model_renderer);

    let mut input = InputState::new();
    let mut timer = FrameTimer::new();
    let mut show_entities = true;
    let mut first_mouse = true;

    loop {
        input.begin_frame();
        for event in event_pump.poll_iter() {
            if first_mouse {
                if matches!(event, sdl2::event::Event::MouseMotion { .. }) {
                    first_mouse = false;
                    continue;
                }
            }
            input.handle_event(&event);
        }
        if input.quit { break; }

        let keys = event_pump.keyboard_state();
        input.update_from_keyboard(&keys);
        timer.update();

        camera.update(&input, timer.dt);
        model_renderer.update_animations(timer.dt);

        // Map cycling
        if input.just_pressed(Action::NextMap) {
            let next = (current_map_idx + 1) % bsp_files.len();
            if let Some(data) = fs.read(&bsp_files[next]) {
                let new_bsp = bsp::Bsp::parse(data);
                let result = load_bsp_viewer_data(&fs, &new_bsp, &bsp_files[next], &mut model_renderer);
                bsp_renderer = result.0;
                entities = result.1;
                camera = result.2;
                triggers = result.3;
                current_map_idx = next;
                first_mouse = true;
            }
            continue;
        }
        if input.just_pressed(Action::PrevMap) {
            let prev = if current_map_idx == 0 { bsp_files.len() - 1 } else { current_map_idx - 1 };
            if let Some(data) = fs.read(&bsp_files[prev]) {
                let new_bsp = bsp::Bsp::parse(data);
                let result = load_bsp_viewer_data(&fs, &new_bsp, &bsp_files[prev], &mut model_renderer);
                bsp_renderer = result.0;
                entities = result.1;
                camera = result.2;
                triggers = result.3;
                current_map_idx = prev;
                first_mouse = true;
            }
            continue;
        }

        if input.just_pressed(Action::Attack) {
            show_entities = !show_entities;
        }

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

            bsp_renderer.render(&mvp);

            // Entity models
            for ent in entities.entities() {
                if !ent.active { continue; }
                if let Some(ref model_name) = ent.model_name {
                    let model_mat = if let Some(anim_mat) = model_renderer.get_anim_matrix(model_name) {
                        ent.transform.matrix * anim_mat
                    } else {
                        ent.transform.matrix
                    };
                    model_renderer.render(model_name, &model_mat, &mvp);
                }
            }

            if show_entities {
                debug_renderer.render_entities(&mvp, &entities);
                debug_renderer.render_triggers(&mvp, &triggers);
            }

            // HUD
            let sw = w as f32;
            let sh = h as f32;
            let map_name = &bsp_files[current_map_idx];
            let entity_count = entities.entities().iter().filter(|e| e.active).count();
            let hud = format!(
                "BSP Viewer | [{}/{}] {} | {} entities",
                current_map_idx + 1, bsp_files.len(), map_name, entity_count
            );
            ui_renderer.draw_text(&hud, 5.0, 5.0, 1.5, [0.8, 0.8, 0.8, 0.8], sw, sh);

            let pos = camera.pos;
            let pos_str = format!("Pos: ({:.0}, {:.0}, {:.0})", pos.x, pos.y, pos.z);
            ui_renderer.draw_text(&pos_str, 5.0, 22.0, 1.5, [0.5, 0.7, 1.0, 0.7], sw, sh);

            let help = "WASD+Mouse: fly | PgDn/PgUp: maps | F: debug | Esc: quit";
            ui_renderer.draw_text(help, 5.0, sh - 18.0, 1.2, [0.5, 0.5, 0.5, 0.6], sw, sh);
        }

        window.gl_swap_window();
    }
}

fn load_bsp_viewer_data(
    fs: &AppFs,
    bsp: &bsp::Bsp,
    bsp_path: &str,
    model_renderer: &mut ModelRenderer,
) -> (render::BspRenderer, EntityManager, Camera, TriggerSystem) {
    // Camera setup
    let diag = (Vec3::from(bsp.bbox_max) - Vec3::from(bsp.bbox_min)).length();
    let cam_speed = diag * 0.15;
    let mut camera = Camera::new(Vec3::ZERO, cam_speed);

    if let Some(spot) = bsp.camera_spots.first() {
        camera.pos = Vec3::new(spot.pos[0], spot.pos[1], spot.pos[2]);
        let dir = Vec3::new(spot.dir[0], spot.dir[1], spot.dir[2]);
        camera.yaw = dir.z.atan2(dir.x);
        camera.pitch = dir.y.asin();
    } else {
        camera.pos = (Vec3::from(bsp.bbox_min) + Vec3::from(bsp.bbox_max)) * 0.5;
    }

    let bsp_renderer = render::BspRenderer::new(bsp, fs);
    let mut entities = EntityManager::new();
    entities.load_from_bsp(bsp);

    // Load entity models
    for ent in entities.entities() {
        if let Some(ref name) = ent.model_name {
            if !model_renderer.has_model(name) {
                let path = format!(".\\common\\{name}");
                if let Some(data) = fs.read(&path) {
                    if let Some(soj) = SojModel::parse(&data) {
                        model_renderer.upload_model(name, &soj, fs);
                    }
                }
            }
            model_renderer.load_animation(name, fs);
        }
    }

    let mut triggers = TriggerSystem::new();
    triggers.load_from_entities(&entities);

    (bsp_renderer, entities, camera, triggers)
}

// ============================================================================
// OBJ (SOJ) Model Viewer — orbit camera, single model, animation controls
// ============================================================================

pub fn run_obj(
    fs: &AppFs,
    model_name: &str,
    window: &sdl2::video::Window,
    event_pump: &mut sdl2::EventPump,
) {
    let mut model_renderer = ModelRenderer::new();
    let debug_renderer = render::DebugRenderer::new();
    let ui_renderer = UiRenderer::new();

    // Load the model
    let soj_path = format!(".\\common\\{model_name}.soj");
    let data = match fs.read(&soj_path) {
        Some(d) => d,
        None => {
            // Try without .soj extension
            match fs.read(&format!(".\\common\\{model_name}")) {
                Some(d) => d,
                None => {
                    eprintln!("Model not found: {soj_path}");
                    list_models(fs);
                    return;
                }
            }
        }
    };

    let soj = match SojModel::parse(&data) {
        Some(s) => s,
        None => {
            eprintln!("Failed to parse SOJ: {model_name}");
            return;
        }
    };

    // Compute model center and size for camera setup
    let center = Vec3::new(
        (soj.bbox_min[0] + soj.bbox_max[0]) * 0.5,
        (soj.bbox_min[1] + soj.bbox_max[1]) * 0.5,
        (soj.bbox_min[2] + soj.bbox_max[2]) * 0.5,
    );
    let extent = Vec3::new(
        soj.bbox_max[0] - soj.bbox_min[0],
        soj.bbox_max[1] - soj.bbox_min[1],
        soj.bbox_max[2] - soj.bbox_min[2],
    );
    let dist = extent.length().max(5.0) * 1.5;

    println!("Model: {model_name}");
    println!(
        "  {} materials, bbox ({:.1},{:.1},{:.1})-({:.1},{:.1},{:.1})",
        soj.materials.len(),
        soj.bbox_min[0], soj.bbox_min[1], soj.bbox_min[2],
        soj.bbox_max[0], soj.bbox_max[1], soj.bbox_max[2],
    );

    model_renderer.upload_model(model_name, &soj, fs);
    let has_anim = model_renderer.load_animation(model_name, fs);
    if has_anim {
        println!("  Animation loaded");
    }

    let mut camera = Camera::new(Vec3::ZERO, 1.0);
    camera.set_orbit_mode(center, dist);

    let mut input = InputState::new();
    let mut timer = FrameTimer::new();
    let mut first_mouse = true;

    loop {
        input.begin_frame();
        for event in event_pump.poll_iter() {
            if first_mouse {
                if matches!(event, sdl2::event::Event::MouseMotion { .. }) {
                    first_mouse = false;
                    continue;
                }
            }
            input.handle_event(&event);
        }
        if input.quit { break; }

        let keys = event_pump.keyboard_state();
        input.update_from_keyboard(&keys);
        timer.update();

        // Controls
        if input.just_pressed(Action::Confirm) {
            model_renderer.toggle_animation(model_name);
        }
        if input.just_pressed(Action::Cancel) {
            model_renderer.reset_animation(model_name);
        }

        camera.update(&input, timer.dt);
        model_renderer.update_animations(timer.dt);

        let (w, h) = window.size();
        if w > 0 && h > 0 {
            let aspect = w as f32 / h as f32;
            let proj = Mat4::perspective_lh(50.0_f32.to_radians(), aspect, 0.1, 10000.0);
            let view = camera.view_matrix();
            let mvp = proj * view;

            unsafe {
                gl::Viewport(0, 0, w as i32, h as i32);
                gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
            }

            // Grid floor
            debug_renderer.render_grid(&mvp, dist * 2.0, dist * 0.2);

            // Model at origin
            let model_mat = if let Some(anim_mat) = model_renderer.get_anim_matrix(model_name) {
                anim_mat
            } else {
                Mat4::IDENTITY
            };
            model_renderer.render(model_name, &model_mat, &mvp);

            // HUD
            let sw = w as f32;
            let sh = h as f32;
            let mut hud = format!("OBJ Viewer | {model_name}");
            if let Some((indices, mats)) = model_renderer.get_model_stats(model_name) {
                hud += &format!(" | {} tris, {} mats", indices / 3, mats);
            }
            ui_renderer.draw_text(&hud, 5.0, 5.0, 1.5, [0.8, 0.8, 0.8, 0.8], sw, sh);

            if let Some((frame, total, playing)) = model_renderer.get_anim_info(model_name) {
                let status = if playing { ">" } else { "||" };
                let anim_hud = format!("{status} Frame {:.0}/{total}", frame);
                ui_renderer.draw_text(&anim_hud, 5.0, 22.0, 1.5, [0.5, 1.0, 0.5, 0.8], sw, sh);
            }

            let help = "Mouse: orbit | Space/Shift: zoom | Enter: pause | Backspace: reset | Esc: quit";
            ui_renderer.draw_text(help, 5.0, sh - 18.0, 1.2, [0.5, 0.5, 0.5, 0.6], sw, sh);
        }

        window.gl_swap_window();
    }
}

fn list_models(fs: &AppFs) {
    let mut models: Vec<&str> = fs
        .list_files()
        .filter(|p| p.ends_with(".soj"))
        .collect();
    models.sort();
    eprintln!("Available models ({}):", models.len());
    for (i, m) in models.iter().enumerate().take(20) {
        eprintln!("  {m}");
    }
    if models.len() > 20 {
        eprintln!("  ... and {} more", models.len() - 20);
    }
}

// ============================================================================
// NPC Viewer — model + player state machine + animation state controls
// ============================================================================

pub fn run_npc(
    fs: &AppFs,
    model_name: &str,
    window: &sdl2::video::Window,
    event_pump: &mut sdl2::EventPump,
) {
    let mut model_renderer = ModelRenderer::new();
    let debug_renderer = render::DebugRenderer::new();
    let ui_renderer = UiRenderer::new();

    // Load the model
    let soj_path = format!(".\\common\\{model_name}.soj");
    let data = match fs.read(&soj_path).or_else(|| fs.read(&format!(".\\common\\{model_name}"))) {
        Some(d) => d,
        None => {
            eprintln!("Model not found: {soj_path}");
            list_models(fs);
            return;
        }
    };

    let soj = match SojModel::parse(&data) {
        Some(s) => s,
        None => {
            eprintln!("Failed to parse SOJ: {model_name}");
            return;
        }
    };

    let extent = Vec3::new(
        soj.bbox_max[0] - soj.bbox_min[0],
        soj.bbox_max[1] - soj.bbox_min[1],
        soj.bbox_max[2] - soj.bbox_min[2],
    );
    let dist = extent.length().max(5.0) * 2.0;

    model_renderer.upload_model(model_name, &soj, fs);
    model_renderer.load_animation(model_name, fs);

    let mut player = Player::new(Vec3::ZERO);
    player.model_name = Some(model_name.to_string());

    let mut camera = Camera::new(Vec3::ZERO, 1.0);
    camera.set_orbit_mode(Vec3::new(0.0, extent.y * 0.4, 0.0), dist);

    let mut input = InputState::new();
    let mut timer = FrameTimer::new();
    let mut first_mouse = true;

    loop {
        input.begin_frame();
        for event in event_pump.poll_iter() {
            if first_mouse {
                if matches!(event, sdl2::event::Event::MouseMotion { .. }) {
                    first_mouse = false;
                    continue;
                }
            }
            input.handle_event(&event);
        }
        if input.quit { break; }

        let keys = event_pump.keyboard_state();
        input.update_from_keyboard(&keys);
        timer.update();

        // Player update (state machine, movement)
        player.update(&input, camera.yaw, timer.dt, None);

        // Camera follows player
        camera.update_orbit_target(player.pos + Vec3::new(0.0, extent.y * 0.4, 0.0));
        camera.update(&input, timer.dt);
        model_renderer.update_animations(timer.dt);

        let (w, h) = window.size();
        if w > 0 && h > 0 {
            let aspect = w as f32 / h as f32;
            let proj = Mat4::perspective_lh(50.0_f32.to_radians(), aspect, 0.1, 10000.0);
            let view = camera.view_matrix();
            let mvp = proj * view;

            unsafe {
                gl::Viewport(0, 0, w as i32, h as i32);
                gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
            }

            // Grid floor
            debug_renderer.render_grid(&mvp, dist * 2.0, dist * 0.15);

            // Render player model
            let rot = Mat4::from_rotation_y(player.facing_yaw);
            let trans = Mat4::from_translation(player.pos);
            let model_mat = trans * rot;
            let model_mat = if let Some(anim_mat) = model_renderer.get_anim_matrix(model_name) {
                model_mat * anim_mat
            } else {
                model_mat
            };
            model_renderer.render(model_name, &model_mat, &mvp);

            // HUD
            let sw = w as f32;
            let sh = h as f32;
            let hud = format!(
                "NPC Viewer | {model_name} | {} | HP:{}/{}",
                player.state_name(), player.hp, player.max_hp
            );
            ui_renderer.draw_text(&hud, 5.0, 5.0, 1.5, [0.8, 0.8, 0.8, 0.8], sw, sh);

            let state_hud = format!(
                "Anim:{} | Pos:({:.0},{:.0},{:.0})",
                player.anim_state_id, player.pos.x, player.pos.y, player.pos.z
            );
            ui_renderer.draw_text(&state_hud, 5.0, 22.0, 1.5, [0.5, 1.0, 0.5, 0.8], sw, sh);

            // Health bar
            let bar_w = 200.0;
            let bar_h = 8.0;
            let hp_frac = player.hp as f32 / player.max_hp as f32;
            ui_renderer.draw_rect(5.0, 42.0, bar_w, bar_h, [0.2, 0.0, 0.0, 0.7], sw, sh);
            let color = if hp_frac > 0.5 { [0.1, 0.8, 0.1, 0.9] }
                        else if hp_frac > 0.25 { [0.9, 0.7, 0.1, 0.9] }
                        else { [0.9, 0.1, 0.1, 0.9] };
            ui_renderer.draw_rect(5.0, 42.0, bar_w * hp_frac, bar_h, color, sw, sh);

            let help = "WASD: move | F: attack | Mouse: orbit | Space/Shift: zoom | Esc: quit";
            ui_renderer.draw_text(help, 5.0, sh - 18.0, 1.2, [0.5, 0.5, 0.5, 0.6], sw, sh);
        }

        window.gl_swap_window();
    }
}
