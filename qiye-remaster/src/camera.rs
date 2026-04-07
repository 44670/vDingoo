/// Multi-mode camera system.
///
/// Modes matching the original engine:
/// - Free: WASD fly-cam (debug)
/// - Follow: Track entity from behind with offset
/// - Static: Fixed position looking at target
/// - Cinematic: Lerp between two positions over time

use crate::input::{Action, InputState};
use glam::{Mat4, Vec3};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CameraMode {
    Free,
    Follow,
    Static,
    Cinematic,
    Orbit,
}

pub struct Camera {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub speed: f32,
    pub mode: CameraMode,

    // Follow mode
    follow_target: Vec3,
    follow_offset: Vec3,   // offset from target (in target-local space)
    follow_smooth: f32,    // smoothing factor (0=instant, higher=smoother)

    // Static mode
    static_look_at: Vec3,

    // Cinematic mode
    cine_start_pos: Vec3,
    cine_end_pos: Vec3,
    cine_start_look: Vec3,
    cine_end_look: Vec3,
    cine_duration: f32,
    cine_elapsed: f32,

    // Orbit mode
    orbit_target: Vec3,
    orbit_distance: f32,
}

impl Camera {
    pub fn new(pos: Vec3, speed: f32) -> Self {
        Self {
            pos,
            yaw: 0.0,
            pitch: 0.0,
            speed,
            mode: CameraMode::Free,

            follow_target: Vec3::ZERO,
            follow_offset: Vec3::new(0.0, 5.0, -10.0),
            follow_smooth: 5.0,

            static_look_at: Vec3::ZERO,

            cine_start_pos: Vec3::ZERO,
            cine_end_pos: Vec3::ZERO,
            cine_start_look: Vec3::ZERO,
            cine_end_look: Vec3::ZERO,
            cine_duration: 1.0,
            cine_elapsed: 0.0,

            orbit_target: Vec3::ZERO,
            orbit_distance: 50.0,
        }
    }

    pub fn look_dir(&self) -> Vec3 {
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

    pub fn view_matrix(&self) -> Mat4 {
        match self.mode {
            CameraMode::Free | CameraMode::Follow => {
                Mat4::look_at_lh(self.pos, self.pos + self.look_dir(), Vec3::Y)
            }
            CameraMode::Static => {
                Mat4::look_at_lh(self.pos, self.static_look_at, Vec3::Y)
            }
            CameraMode::Cinematic => {
                let t = (self.cine_elapsed / self.cine_duration).clamp(0.0, 1.0);
                let t_smooth = t * t * (3.0 - 2.0 * t); // smoothstep
                let look = self.cine_start_look.lerp(self.cine_end_look, t_smooth);
                Mat4::look_at_lh(self.pos, look, Vec3::Y)
            }
            CameraMode::Orbit => {
                let eye = self.orbit_target + Vec3::new(
                    self.yaw.cos() * self.pitch.cos() * self.orbit_distance,
                    self.pitch.sin() * self.orbit_distance,
                    self.yaw.sin() * self.pitch.cos() * self.orbit_distance,
                );
                Mat4::look_at_lh(eye, self.orbit_target, Vec3::Y)
            }
        }
    }

    pub fn update(&mut self, input: &InputState, dt: f32) {
        match self.mode {
            CameraMode::Free => self.update_free(input, dt),
            CameraMode::Follow => self.update_follow(dt),
            CameraMode::Static => {}
            CameraMode::Cinematic => self.update_cinematic(dt),
            CameraMode::Orbit => self.update_orbit(input, dt),
        }
    }

    fn update_free(&mut self, input: &InputState, dt: f32) {
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

    fn update_follow(&mut self, dt: f32) {
        // Compute desired camera position behind the target
        let desired = self.follow_target + self.follow_offset;
        let t = (self.follow_smooth * dt).min(1.0);
        self.pos = self.pos.lerp(desired, t);

        // Look at the target
        let to_target = self.follow_target - self.pos;
        if to_target.length_squared() > 0.001 {
            let dir = to_target.normalize();
            self.yaw = dir.z.atan2(dir.x);
            self.pitch = dir.y.asin();
        }
    }

    fn update_cinematic(&mut self, dt: f32) {
        self.cine_elapsed += dt;
        let t = (self.cine_elapsed / self.cine_duration).clamp(0.0, 1.0);
        let t_smooth = t * t * (3.0 - 2.0 * t); // smoothstep
        self.pos = self.cine_start_pos.lerp(self.cine_end_pos, t_smooth);
    }

    // --- Mode switching ---

    pub fn set_free_mode(&mut self) {
        self.mode = CameraMode::Free;
    }

    pub fn set_follow_mode(&mut self, target: Vec3, offset: Vec3) {
        self.mode = CameraMode::Follow;
        self.follow_target = target;
        self.follow_offset = offset;
    }

    pub fn set_static_mode(&mut self, pos: Vec3, look_at: Vec3) {
        self.mode = CameraMode::Static;
        self.pos = pos;
        self.static_look_at = look_at;
    }

    pub fn set_cinematic_mode(&mut self, from: Vec3, to: Vec3, look_from: Vec3, look_to: Vec3, duration: f32) {
        self.mode = CameraMode::Cinematic;
        self.cine_start_pos = from;
        self.cine_end_pos = to;
        self.cine_start_look = look_from;
        self.cine_end_look = look_to;
        self.cine_duration = duration.max(0.01);
        self.cine_elapsed = 0.0;
        self.pos = from;
    }

    /// Update follow target position (call each frame for tracked entity).
    pub fn update_follow_target(&mut self, target: Vec3) {
        self.follow_target = target;
    }

    pub fn set_orbit_mode(&mut self, target: Vec3, distance: f32) {
        self.mode = CameraMode::Orbit;
        self.orbit_target = target;
        self.orbit_distance = distance;
        self.yaw = 0.5;
        self.pitch = 0.3;
    }

    pub fn update_orbit_target(&mut self, target: Vec3) {
        self.orbit_target = target;
    }

    fn update_orbit(&mut self, input: &InputState, _dt: f32) {
        self.yaw += input.mouse_dx * 0.005;
        self.pitch += input.mouse_dy * 0.005;
        self.pitch = self.pitch.clamp(-1.4, 1.4);

        // Scroll zoom via MoveUp/MoveDown
        if input.is_down(Action::MoveUp) {
            self.orbit_distance *= 0.97;
        }
        if input.is_down(Action::MoveDown) {
            self.orbit_distance *= 1.03;
        }
        self.orbit_distance = self.orbit_distance.clamp(1.0, 500.0);
    }

    pub fn is_cinematic_done(&self) -> bool {
        self.mode == CameraMode::Cinematic && self.cine_elapsed >= self.cine_duration
    }
}
