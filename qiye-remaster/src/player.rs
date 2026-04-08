/// Player entity — aligned with original binary (Player struct at 0xF44 bytes).
///
/// State machine: Stand(0), Run(1), AttackA1-A3(2-4), AttackB1-B3(5-7),
/// AttackC1-C3(8-10), Hurt1-4(11-14), Die(15), Push(16), QTE(17), Hide(18), FPS(19).
///
/// Key offsets from RE:
///   +0x0C  position (vec3 fp16.16)
///   +0x90  physics_flags
///   +0x0A0 velocity (vec3 fp16.16) — dead zone 0x28F
///   +0x6B4 move_speed (fp16.16, init 0xCCCC ≈ 0.8)
///   +0x6F0 state_machine (StateMachine<Player>)
///   +0x6FC attack_damage
///   +0x700 state_table pointer
///   +0x718 death_counter
///   +0x728 invincible flag
///   +0x730 hp
///   +0x7A0 pending_anim_state
///   +0x7B8 hurt1_pending
///   +0x7BC hurt2_pending (fall damage)
///   +0x7C0 hurt3_pending
///   +0x7C4 hurt4_pending (die)
///   +0x7CC dead_flag
///   +0x7D0 last_damage_source
///   +0x7E4 combo_pending
///   +0x7F8 combo_hit_counter
///   +0x800 heavy_attack
///   +0x9BC push_target
///   +0x9C0 knockback_dir (vec3)
///   +0x9DC anim_enabled
///   +0x9E0 cutscene_active

use crate::bsp::Bsp;
use crate::input::{Action, InputState};
use glam::Vec3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PlayerState {
    Stand = 0,
    Run = 1,
    AttackA1 = 2,
    AttackA2 = 3,
    AttackA3 = 4,
    AttackB1 = 5,
    AttackB2 = 6,
    AttackB3 = 7,
    AttackC1 = 8,
    AttackC2 = 9,
    AttackC3 = 10,
    Hurt1 = 11,
    Hurt2 = 12,
    Hurt3 = 13,
    Hurt4 = 14,
    Die = 15,
    Push = 16,
    Qte = 17,
    Hide = 18,
    Fps = 19,
}

pub struct Player {
    pub pos: Vec3,
    pub velocity: Vec3,
    pub facing_yaw: f32,
    pub state: PlayerState,
    pub hp: i32,
    pub max_hp: i32,
    pub move_speed: f32,       // original: 0xCCCC fp16.16 ≈ 0.8 world units/tick → ~8.0 at 60fps
    pub attack_damage: i32,    // +0x6FC in original
    pub invincible: bool,      // +0x728
    pub dead: bool,            // +0x7CC
    pub active: bool,
    pub model_name: Option<String>,
    pub anim_enabled: bool,    // +0x9DC, set to true on state change

    // Pending damage flags — original checks these in priority order during transitions
    pub hurt1_pending: bool,   // +0x7B8 — normal hit
    pub hurt2_pending: bool,   // +0x7BC — fall damage
    pub hurt3_pending: bool,   // +0x7C0 — heavy hit
    pub hurt4_pending: bool,   // +0x7C4 — death blow

    // State timer (seconds in remaster; original counts animation frames)
    state_timer: f32,
    // Attack combo system
    combo_pending: bool,       // +0x7E4
    combo_hit_counter: i32,    // +0x7F8
    heavy_attack: bool,        // +0x800
    // Weapon set determines which attack chain (A/B/C) — original +0x794
    weapon_set: i32,           // 0=A chain, 1=B chain, 2=C chain
    // Death counter
    pub death_counter: i32,    // +0x718
    // Knockback
    knockback_dir: Vec3,       // +0x9C0

    // Physics
    pub on_ground: bool,
    gravity: f32,
    // Movement lock (cutscene, etc) — original +0x6D0
    pub movement_lock: bool,
    // Cutscene
    pub cutscene_active: bool, // +0x9E0
    saved_pos: Vec3,           // +0x9EC — position before cutscene

    // Animation state (what to tell the model renderer)
    pub anim_state_id: i32,    // +0x7A0 — 0=stand, 1=run, 2=atkA1, 3=atkA2, etc.
    pub anim_loop: bool,       // +0x7A4
    pub anim_restart: bool,    // +0x7A8
    // Floor index (determines attack sounds, some transitions)
    pub floor_index: i32,      // from scene data
}

/// Velocity dead zone — original uses 0x28F in fp16.16 ≈ 0.0099
const VEL_DEAD_ZONE: f32 = 0.01;
/// Fall damage amount — original hardcoded 0x1E = 30 HP
const FALL_DAMAGE: i32 = 30;
/// Attack durations in seconds (approximate from animation frame counts at 30fps)
const ATTACK_A1_DUR: f32 = 0.4;
const ATTACK_A2_DUR: f32 = 0.5;
const ATTACK_A3_DUR: f32 = 0.6;
const ATTACK_B1_DUR: f32 = 0.4;
const ATTACK_B2_DUR: f32 = 0.5;
const ATTACK_B3_DUR: f32 = 0.6;
const ATTACK_C1_DUR: f32 = 0.5;
const ATTACK_C2_DUR: f32 = 0.5;
const ATTACK_C3_DUR: f32 = 0.7;
const HURT_DUR: f32 = 0.4;
const DIE_DUR: f32 = 2.0;

impl Player {
    pub fn new(pos: Vec3) -> Self {
        Self {
            pos,
            velocity: Vec3::ZERO,
            facing_yaw: 0.0,
            state: PlayerState::Stand,
            hp: 50,
            max_hp: 100,
            move_speed: 8.0,
            attack_damage: 10,
            invincible: false,
            dead: false,
            active: true,
            model_name: None,
            anim_enabled: true,
            hurt1_pending: false,
            hurt2_pending: false,
            hurt3_pending: false,
            hurt4_pending: false,
            state_timer: 0.0,
            combo_pending: false,
            combo_hit_counter: 0,
            heavy_attack: false,
            weapon_set: 0,
            death_counter: 0,
            knockback_dir: Vec3::ZERO,
            on_ground: true,
            gravity: 20.0,
            movement_lock: false,
            cutscene_active: false,
            saved_pos: Vec3::ZERO,
            anim_state_id: 0,
            anim_loop: true,
            anim_restart: false,
            floor_index: 0,
        }
    }

    pub fn update(&mut self, input: &InputState, camera_yaw: f32, dt: f32, bsp: Option<&Bsp>) {
        if self.dead || !self.active {
            return;
        }

        // Process pending damage flags (original: checked in Player_update before state machine)
        self.process_pending_damage();

        // State transition — mirrors StateMachine_Player_processState
        let next = self.get_transition(input, camera_yaw);
        if next != self.state {
            self.change_state(next);
        }

        // Per-state update
        match self.state {
            PlayerState::Stand => self.update_stand(input, camera_yaw),
            PlayerState::Run => self.update_run(input, camera_yaw, dt),
            PlayerState::AttackA1 | PlayerState::AttackA2 | PlayerState::AttackA3
            | PlayerState::AttackB1 | PlayerState::AttackB2 | PlayerState::AttackB3
            | PlayerState::AttackC1 | PlayerState::AttackC2 | PlayerState::AttackC3 => {
                self.update_attack(input, dt);
            }
            PlayerState::Hurt1 | PlayerState::Hurt2 | PlayerState::Hurt3 | PlayerState::Hurt4 => {
                self.update_hurt(dt);
            }
            PlayerState::Die => self.update_die(dt),
            PlayerState::Push => self.update_push(dt),
            _ => {}
        }

        // Apply gravity (original: PhysicsEntity_applyGravity)
        if !self.on_ground {
            self.velocity.y -= self.gravity * dt;
        }

        // Dead zone — original zeros velocity components below 0x28F (~0.01)
        if self.velocity.x.abs() < VEL_DEAD_ZONE { self.velocity.x = 0.0; }
        if self.velocity.z.abs() < VEL_DEAD_ZONE { self.velocity.z = 0.0; }

        // Move with BSP collision (Quake-style moveAndSlide)
        let move_delta = self.velocity * dt;
        if let Some(bsp) = bsp {
            self.move_and_slide(bsp, move_delta);
        } else {
            self.pos += move_delta;
        }

        // Ground trace — check if standing on something
        if let Some(bsp) = bsp {
            self.trace_ground(bsp);
        }

        // Friction (XZ only)
        let friction = 0.85_f32.powf(dt * 60.0);
        self.velocity.x *= friction;
        self.velocity.z *= friction;
    }

    /// Player AABB — relative mins/maxs. Original player is roughly human-sized.
    const PLAYER_MINS: [f32; 3] = [-0.5, 0.0, -0.5];
    const PLAYER_MAXS: [f32; 3] = [0.5, 2.5, 0.5];

    /// Quake-style moveAndSlide: iteratively sweep and clip velocity against hit planes.
    /// Up to 5 iterations (matches original PhysicsEntity_moveAndSlide).
    fn move_and_slide(&mut self, bsp: &Bsp, mut move_delta: Vec3) {
        let mut clip_planes: Vec<Vec3> = Vec::new();
        let original_vel = move_delta;

        for _iter in 0..5 {
            if move_delta.length_squared() < 1e-8 {
                break;
            }

            let start = [self.pos.x, self.pos.y, self.pos.z];
            let end = [
                self.pos.x + move_delta.x,
                self.pos.y + move_delta.y,
                self.pos.z + move_delta.z,
            ];

            let trace = bsp.trace_box(start, end, Self::PLAYER_MINS, Self::PLAYER_MAXS);

            if trace.all_solid {
                // Stuck — zero velocity and bail
                self.velocity = Vec3::ZERO;
                break;
            }

            // Move to the trace end position
            self.pos = Vec3::new(trace.end_pos[0], trace.end_pos[1], trace.end_pos[2]);

            if !trace.hit {
                break; // no collision, done
            }

            // Reduce remaining movement by consumed fraction
            let remaining = 1.0 - trace.fraction;
            move_delta *= remaining;

            // Record this clip plane
            let normal = Vec3::new(trace.normal[0], trace.normal[1], trace.normal[2]);
            clip_planes.push(normal);

            // Clip velocity off all accumulated planes
            for plane_n in &clip_planes {
                let d = move_delta.dot(*plane_n);
                if d < 0.0 {
                    // Moving into plane — reflect off with small overbounce
                    move_delta -= *plane_n * (d - 0.01);
                }
            }

            // Also clip the stored velocity
            let d = self.velocity.dot(normal);
            if d < 0.0 {
                self.velocity -= normal * d;
            }

            // Stop if reversed direction (ping-pong between walls)
            if move_delta.dot(original_vel) <= 0.0 {
                move_delta = Vec3::ZERO;
                break;
            }
        }
    }

    /// Trace downward to detect ground contact (mirrors PhysicsEntity_traceGround).
    fn trace_ground(&mut self, bsp: &Bsp) {
        // Original GameUnit_groundCheck traces 10 units (0xA0000 in 16.16 fixed-point) downward
        let ground_check_dist = 10.0;
        let start = [self.pos.x, self.pos.y, self.pos.z];
        let end = [self.pos.x, self.pos.y - ground_check_dist, self.pos.z];

        let trace = bsp.trace_box(start, end, Self::PLAYER_MINS, Self::PLAYER_MAXS);

        if trace.hit && trace.normal[1] > 0.7 {
            // Standing on a surface with a mostly-upward normal
            self.on_ground = true;
            if self.velocity.y < 0.0 {
                self.velocity.y = 0.0;
            }
            // Snap to ground
            self.pos = Vec3::new(trace.end_pos[0], trace.end_pos[1], trace.end_pos[2]);
        } else {
            self.on_ground = false;
        }
    }

    /// Snap player to the ground below. Mirrors Scene_loadPlayerAdult from the original binary.
    /// Original traces from pos.y + 5 to pos.y - 10 (15 units total sweep).
    pub fn ground_snap(&mut self, bsp: &Bsp) {
        let start = [self.pos.x, self.pos.y + 5.0, self.pos.z];
        let end = [self.pos.x, self.pos.y - 10.0, self.pos.z];

        println!("Player: ground_snap trace from ({:.1},{:.1},{:.1}) to ({:.1},{:.1},{:.1})",
            start[0], start[1], start[2], end[0], end[1], end[2]);
        println!("  AABB mins={:?} maxs={:?}", Self::PLAYER_MINS, Self::PLAYER_MAXS);
        println!("  BSP: {} nodes, {} leaves, {} brushes",
            bsp.nodes.len(), bsp.leaves.len(), bsp.brushes.len());

        let trace = bsp.trace_box(start, end, Self::PLAYER_MINS, Self::PLAYER_MAXS);
        println!("  trace result: hit={} fraction={:.3} all_solid={} normal=({:.2},{:.2},{:.2}) end=({:.1},{:.1},{:.1})",
            trace.hit, trace.fraction, trace.all_solid,
            trace.normal[0], trace.normal[1], trace.normal[2],
            trace.end_pos[0], trace.end_pos[1], trace.end_pos[2]);

        if trace.hit {
            self.pos = Vec3::new(trace.end_pos[0], trace.end_pos[1], trace.end_pos[2]);
            self.on_ground = true;
            self.velocity = Vec3::ZERO;
            println!("Player: ground snap → ({:.1}, {:.1}, {:.1})", self.pos.x, self.pos.y, self.pos.z);
        } else {
            println!("Player: ground snap FAILED — no floor found below");
        }
    }

    /// Process pending damage flags in priority order.
    /// Original: checked in Player_update before state machine tick.
    fn process_pending_damage(&mut self) {
        if self.dead { return; }

        if self.hurt4_pending {
            // Die
            self.hurt4_pending = false;
            self.hp = 0;
            self.dead = true;
            self.death_counter += 1;
            self.change_state(PlayerState::Die);
        } else if self.hurt3_pending {
            // Heavy hit
            self.hurt3_pending = false;
            self.change_state(PlayerState::Hurt3);
        } else if self.hurt2_pending {
            // Fall damage
            self.hurt2_pending = false;
            self.change_state(PlayerState::Hurt2);
        } else if self.hurt1_pending {
            // Normal hit
            self.hurt1_pending = false;
            self.change_state(PlayerState::Hurt1);
        }
    }

    /// State transitions — mirrors PlayerState_Stand_getTransition / PlayerState_getTransition.
    ///
    /// Original priority in every state:
    ///   1. dead_flag (+0x7CC) → Die
    ///   2. hurt1_pending (+0x7B8) → Hurt1
    ///   3. hurt2_pending (+0x7BC) → Hurt2
    ///   4. hurt3_pending (+0x7C0) → Hurt3
    ///   5. hurt4_pending (+0x7C4) → Hurt4
    ///   6. State-specific logic
    fn get_transition(&self, input: &InputState, _camera_yaw: f32) -> PlayerState {
        // Global priority checks (present in every state's getTransition)
        if self.dead {
            return PlayerState::Die;
        }

        match self.state {
            PlayerState::Stand => {
                // Movement lock (cutscene etc) — no transitions
                if self.movement_lock {
                    return PlayerState::Stand;
                }
                // Attack — checks floor_index to determine attack type
                if input.just_pressed(Action::Attack) {
                    return self.select_attack_state();
                }
                // Movement — original checks abs(velocity.x) >= 0x28F || abs(velocity.z) >= 0x28F
                if self.has_movement_input(input) {
                    return PlayerState::Run;
                }
                PlayerState::Stand
            }
            PlayerState::Run => {
                if self.movement_lock {
                    return PlayerState::Stand;
                }
                if input.just_pressed(Action::Attack) {
                    return self.select_attack_state();
                }
                // Stop when no input — original checks velocity dead zone
                if !self.has_movement_input(input) {
                    return PlayerState::Stand;
                }
                PlayerState::Run
            }
            // Attack states — transition when animation finishes
            PlayerState::AttackA1 => {
                if self.state_timer <= 0.0 {
                    if self.combo_pending {
                        return PlayerState::AttackA2;
                    }
                    // Original: return to Stand (state_table + 0x10 → Stand after anim done)
                    return PlayerState::Stand;
                }
                PlayerState::AttackA1
            }
            PlayerState::AttackA2 => {
                if self.state_timer <= 0.0 {
                    if self.combo_pending {
                        return PlayerState::AttackA3;
                    }
                    return PlayerState::Stand;
                }
                PlayerState::AttackA2
            }
            PlayerState::AttackA3 => {
                if self.state_timer <= 0.0 {
                    return PlayerState::Stand;
                }
                PlayerState::AttackA3
            }
            PlayerState::AttackB1 => {
                if self.state_timer <= 0.0 {
                    if self.combo_pending { return PlayerState::AttackB2; }
                    return PlayerState::Stand;
                }
                PlayerState::AttackB1
            }
            PlayerState::AttackB2 => {
                if self.state_timer <= 0.0 {
                    if self.combo_pending { return PlayerState::AttackB3; }
                    return PlayerState::Stand;
                }
                PlayerState::AttackB2
            }
            PlayerState::AttackB3 => {
                if self.state_timer <= 0.0 { return PlayerState::Stand; }
                PlayerState::AttackB3
            }
            PlayerState::AttackC1 => {
                if self.state_timer <= 0.0 {
                    if self.combo_pending { return PlayerState::AttackC2; }
                    return PlayerState::Stand;
                }
                PlayerState::AttackC1
            }
            PlayerState::AttackC2 => {
                if self.state_timer <= 0.0 {
                    if self.combo_pending { return PlayerState::AttackC3; }
                    return PlayerState::Stand;
                }
                PlayerState::AttackC2
            }
            PlayerState::AttackC3 => {
                if self.state_timer <= 0.0 { return PlayerState::Stand; }
                PlayerState::AttackC3
            }
            // Hurt — return to Stand when timer expires
            // Original: also checks hurt escalation flags during hurt
            PlayerState::Hurt1 | PlayerState::Hurt2 | PlayerState::Hurt3 | PlayerState::Hurt4 => {
                if self.state_timer <= 0.0 {
                    return PlayerState::Stand;
                }
                self.state
            }
            // Die — stays in Die (original: checks for cutscene/respawn after anim)
            PlayerState::Die => PlayerState::Die,
            // Push — timer-based return to Stand
            PlayerState::Push => {
                if self.state_timer <= 0.0 {
                    return PlayerState::Stand;
                }
                PlayerState::Push
            }
            _ => self.state,
        }
    }

    /// Select which attack chain based on weapon_set.
    /// Original: Player_selectAttackAnim uses weapon_set (+0x794) and floor_index.
    fn select_attack_state(&self) -> PlayerState {
        match self.weapon_set {
            0 => PlayerState::AttackA1,
            1 => PlayerState::AttackB1,
            2 => PlayerState::AttackC1,
            _ => PlayerState::AttackA1,
        }
    }

    /// Change state — mirrors Player_changeState.
    /// Resets all transient flags, calls ground check.
    fn change_state(&mut self, state: PlayerState) {
        let old = self.state;
        self.state = state;

        // Reset flags (original: clears combo, heavy_attack, all pending, etc.)
        self.combo_pending = false;
        self.combo_hit_counter = 0;
        self.heavy_attack = false;
        self.anim_restart = true;

        // State-specific enter logic
        match state {
            PlayerState::Stand => {
                self.velocity.x = 0.0;
                self.velocity.z = 0.0;
                self.set_anim(0, true); // anim 0 = stand
            }
            PlayerState::Run => {
                self.set_anim(1, true); // anim 1 = run (looping)
            }
            PlayerState::AttackA1 => {
                self.velocity.x = 0.0;
                self.velocity.z = 0.0;
                self.state_timer = ATTACK_A1_DUR;
                self.set_anim(2, false);
            }
            PlayerState::AttackA2 => {
                self.velocity.x = 0.0;
                self.velocity.z = 0.0;
                self.state_timer = ATTACK_A2_DUR;
                self.set_anim(3, false);
                // Original sets combo_pending = true, sword link update
            }
            PlayerState::AttackA3 => {
                self.velocity.x = 0.0;
                self.velocity.z = 0.0;
                self.state_timer = ATTACK_A3_DUR;
                self.set_anim(4, false);
            }
            PlayerState::AttackB1 => {
                self.velocity.x = 0.0;
                self.velocity.z = 0.0;
                self.state_timer = ATTACK_B1_DUR;
                self.set_anim(5, false);
            }
            PlayerState::AttackB2 => {
                self.velocity.x = 0.0;
                self.velocity.z = 0.0;
                self.state_timer = ATTACK_B2_DUR;
                self.set_anim(6, false);
            }
            PlayerState::AttackB3 => {
                self.velocity.x = 0.0;
                self.velocity.z = 0.0;
                self.state_timer = ATTACK_B3_DUR;
                self.set_anim(7, false);
            }
            PlayerState::AttackC1 => {
                self.velocity.x = 0.0;
                self.velocity.z = 0.0;
                self.state_timer = ATTACK_C1_DUR;
                self.set_anim(8, false);
            }
            PlayerState::AttackC2 => {
                self.velocity.x = 0.0;
                self.velocity.z = 0.0;
                self.state_timer = ATTACK_C2_DUR;
                self.set_anim(9, false);
            }
            PlayerState::AttackC3 => {
                self.velocity.x = 0.0;
                self.velocity.z = 0.0;
                self.state_timer = ATTACK_C3_DUR;
                self.set_anim(10, false);
            }
            PlayerState::Hurt1 => {
                self.state_timer = HURT_DUR;
                self.set_anim(8, false); // original: anim 8
                // Apply knockback from knockback_dir
                let kb_speed = 3.0;
                self.velocity.x = self.knockback_dir.x * kb_speed;
                self.velocity.z = self.knockback_dir.z * kb_speed;
            }
            PlayerState::Hurt2 => {
                self.state_timer = HURT_DUR;
                self.set_anim(8, false);
            }
            PlayerState::Hurt3 => {
                self.state_timer = HURT_DUR * 1.5;
                self.set_anim(8, false);
            }
            PlayerState::Hurt4 => {
                self.state_timer = HURT_DUR * 2.0;
                self.set_anim(8, false);
            }
            PlayerState::Die => {
                self.velocity = Vec3::ZERO;
                self.state_timer = DIE_DUR;
                // Original: anim 9 (normal die) or 11 (fall death)
                let anim = if old == PlayerState::Hurt2 { 11 } else { 9 };
                self.set_anim(anim, false);
            }
            PlayerState::Push => {
                self.state_timer = 1.0;
                self.set_anim(0, true); // push uses stand-like anim
            }
            _ => {}
        }
    }

    fn set_anim(&mut self, id: i32, looping: bool) {
        self.anim_state_id = id;
        self.anim_loop = looping;
        self.anim_enabled = true;
    }

    fn update_stand(&mut self, _input: &InputState, _camera_yaw: f32) {
        // Decelerate to stop
        self.velocity.x *= 0.9;
        self.velocity.z *= 0.9;
    }

    /// Camera-relative movement.
    /// Original: Scene_updatePlayerMovement gets camera forward/right vectors,
    /// projects input onto them, applies move_speed multiplier, then dead-zones.
    fn update_run(&mut self, input: &InputState, camera_yaw: f32, _dt: f32) {
        let fwd = Vec3::new(camera_yaw.cos(), 0.0, camera_yaw.sin());
        let right = Vec3::new(-camera_yaw.sin(), 0.0, camera_yaw.cos());
        let mut dir = Vec3::ZERO;

        if input.is_down(Action::MoveForward) { dir += fwd; }
        if input.is_down(Action::MoveBack) { dir -= fwd; }
        if input.is_down(Action::MoveRight) { dir += right; }
        if input.is_down(Action::MoveLeft) { dir -= right; }

        if dir.length_squared() > 0.001 {
            let dir = dir.normalize();
            let speed = if input.is_down(Action::Sprint) {
                self.move_speed * 1.5
            } else {
                self.move_speed
            };

            self.velocity.x = dir.x * speed;
            self.velocity.z = dir.z * speed;
            self.facing_yaw = dir.z.atan2(dir.x);
        }
    }

    fn update_attack(&mut self, input: &InputState, dt: f32) {
        self.state_timer -= dt;
        if input.just_pressed(Action::Attack) {
            self.combo_pending = true;
        }
        // Decelerate during attack
        self.velocity.x *= 0.95;
        self.velocity.z *= 0.95;
    }

    fn update_hurt(&mut self, dt: f32) {
        self.state_timer -= dt;
        self.velocity.x *= 0.9;
        self.velocity.z *= 0.9;
    }

    fn update_die(&mut self, dt: f32) {
        self.state_timer -= dt;
        if self.state_timer <= 0.0 {
            self.dead = true;
        }
    }

    fn update_push(&mut self, dt: f32) {
        self.state_timer -= dt;
    }

    fn has_movement_input(&self, input: &InputState) -> bool {
        input.is_down(Action::MoveForward)
            || input.is_down(Action::MoveBack)
            || input.is_down(Action::MoveLeft)
            || input.is_down(Action::MoveRight)
    }

    /// Apply damage — mirrors Player_applyDamage.
    ///
    /// Original logic:
    ///   - If dead_flag (+0x7CC) set, bail
    ///   - If damage_source (+0x7D0) has attack_damage > 0, use it; else use 1
    ///   - Subtract from HP (+0x730)
    ///   - If HP > 0: set hurt1_pending
    ///   - If HP <= 0: increment death_counter, set dead_flag + hurt4_pending
    pub fn take_damage(&mut self, damage: i32, from_dir: Vec3) {
        if self.invincible || self.dead {
            return;
        }

        let actual_damage = damage.max(1);
        self.hp -= actual_damage;
        self.knockback_dir = from_dir;

        if self.hp <= 0 {
            self.hp = 0;
            self.death_counter += 1;
            self.dead = true;
            self.hurt4_pending = true; // triggers Die on next update
        } else {
            self.hurt1_pending = true; // triggers Hurt1 on next update
        }
    }

    /// Apply fall damage — mirrors Player_takeFallDamage.
    /// Fixed 30 HP damage, sets hurt2_pending.
    pub fn take_fall_damage(&mut self) {
        if self.invincible || self.dead {
            return;
        }

        self.hp -= FALL_DAMAGE;
        if self.hp <= 0 {
            self.hp = 0;
            self.death_counter += 1;
            self.dead = true;
            self.hurt2_pending = true;
        } else {
            self.hurt2_pending = true;
        }
    }

    /// Enter cutscene — mirrors Player_enterCutsceneState.
    /// Saves position, teleports player, locks movement.
    pub fn enter_cutscene(&mut self, target_pos: Vec3) {
        if self.dead { return; }
        self.saved_pos = self.pos;
        self.pos = target_pos;
        self.cutscene_active = true;
        self.movement_lock = true;
        self.velocity = Vec3::ZERO;
    }

    /// Exit cutscene — mirrors Player_exitCutscene.
    /// Restores position, unlocks movement.
    pub fn exit_cutscene(&mut self) {
        self.pos = self.saved_pos;
        self.cutscene_active = false;
        self.movement_lock = false;
        self.anim_enabled = true;
    }

    /// Enter push state — mirrors Player_enterPushState.
    pub fn enter_push(&mut self) {
        if self.dead { return; }
        self.change_state(PlayerState::Push);
    }

    /// Enter FPS mode.
    pub fn enter_fps(&mut self) {
        if self.dead { return; }
        self.change_state(PlayerState::Fps);
    }

    pub fn is_attacking(&self) -> bool {
        matches!(
            self.state,
            PlayerState::AttackA1 | PlayerState::AttackA2 | PlayerState::AttackA3
            | PlayerState::AttackB1 | PlayerState::AttackB2 | PlayerState::AttackB3
            | PlayerState::AttackC1 | PlayerState::AttackC2 | PlayerState::AttackC3
        )
    }

    pub fn state_name(&self) -> &'static str {
        match self.state {
            PlayerState::Stand => "Stand",
            PlayerState::Run => "Run",
            PlayerState::AttackA1 => "AtkA1",
            PlayerState::AttackA2 => "AtkA2",
            PlayerState::AttackA3 => "AtkA3",
            PlayerState::AttackB1 => "AtkB1",
            PlayerState::AttackB2 => "AtkB2",
            PlayerState::AttackB3 => "AtkB3",
            PlayerState::AttackC1 => "AtkC1",
            PlayerState::AttackC2 => "AtkC2",
            PlayerState::AttackC3 => "AtkC3",
            PlayerState::Hurt1 => "Hurt1",
            PlayerState::Hurt2 => "Hurt2",
            PlayerState::Hurt3 => "Hurt3",
            PlayerState::Hurt4 => "Hurt4",
            PlayerState::Die => "Die",
            PlayerState::Push => "Push",
            PlayerState::Qte => "QTE",
            PlayerState::Hide => "Hide",
            PlayerState::Fps => "FPS",
        }
    }
}
