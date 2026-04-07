/// Enemy AI system.
///
/// Enemies use a simple state machine: Idle → Chase → Attack → cooldown → repeat.
/// Based on the original Creature/Enemy hierarchy from qiye.elf.

use glam::Vec3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyState {
    Idle,
    Chase,
    Attack,
    Hurt,
    Die,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyType {
    Bully,
    Ghost,
    DarkKen,
    Lamper,
    Weed,
    Generic,
}

pub struct Enemy {
    pub pos: Vec3,
    pub velocity: Vec3,
    pub facing_yaw: f32,
    pub state: EnemyState,
    pub enemy_type: EnemyType,
    pub hp: i32,
    pub max_hp: i32,
    pub damage: i32,
    pub move_speed: f32,
    pub attack_range: f32,
    pub detect_range: f32,
    pub active: bool,
    pub entity_idx: usize,       // index into EntityManager
    pub model_name: Option<String>,

    state_timer: f32,
    attack_cooldown: f32,
    idle_timer: f32,
}

impl Enemy {
    pub fn new(pos: Vec3, enemy_type: EnemyType, entity_idx: usize) -> Self {
        let (hp, damage, speed, attack_range, detect_range) = match enemy_type {
            EnemyType::Bully => (30, 5, 4.0, 2.5, 20.0),
            EnemyType::Ghost => (20, 8, 5.0, 2.0, 25.0),
            EnemyType::DarkKen => (50, 10, 6.0, 3.0, 30.0),
            EnemyType::Lamper => (15, 3, 3.0, 2.0, 15.0),
            EnemyType::Weed => (10, 4, 0.0, 5.0, 10.0), // stationary
            EnemyType::Generic => (20, 5, 4.0, 2.5, 20.0),
        };

        Self {
            pos,
            velocity: Vec3::ZERO,
            facing_yaw: 0.0,
            state: EnemyState::Idle,
            enemy_type,
            hp,
            max_hp: hp,
            damage,
            move_speed: speed,
            attack_range,
            detect_range,
            active: true,
            entity_idx,
            model_name: None,
            state_timer: 0.0,
            attack_cooldown: 0.0,
            idle_timer: 2.0 + (entity_idx as f32 * 0.7) % 3.0, // stagger idle
        }
    }

    pub fn update(&mut self, player_pos: Vec3, player_alive: bool, dt: f32) {
        if !self.active || self.state == EnemyState::Die {
            return;
        }

        let to_player = player_pos - self.pos;
        let dist_xz = Vec3::new(to_player.x, 0.0, to_player.z).length();

        // State transitions
        let next = self.get_transition(dist_xz, player_alive);
        if next != self.state {
            self.enter_state(next);
        }

        // State behavior
        match self.state {
            EnemyState::Idle => {
                self.idle_timer -= dt;
                self.velocity.x *= 0.9;
                self.velocity.z *= 0.9;
            }
            EnemyState::Chase => {
                if dist_xz > 0.5 {
                    let dir = Vec3::new(to_player.x, 0.0, to_player.z).normalize();
                    self.velocity.x = dir.x * self.move_speed;
                    self.velocity.z = dir.z * self.move_speed;
                    self.facing_yaw = dir.z.atan2(dir.x);
                }
            }
            EnemyState::Attack => {
                self.state_timer -= dt;
                self.velocity.x *= 0.9;
                self.velocity.z *= 0.9;
                // Face player during attack
                if dist_xz > 0.01 {
                    let dir = Vec3::new(to_player.x, 0.0, to_player.z).normalize();
                    self.facing_yaw = dir.z.atan2(dir.x);
                }
            }
            EnemyState::Hurt => {
                self.state_timer -= dt;
                self.velocity.x *= 0.85;
                self.velocity.z *= 0.85;
            }
            EnemyState::Die => {}
        }

        // Cooldown
        if self.attack_cooldown > 0.0 {
            self.attack_cooldown -= dt;
        }

        // Apply velocity
        self.pos += self.velocity * dt;
    }

    fn get_transition(&self, dist_to_player: f32, player_alive: bool) -> EnemyState {
        match self.state {
            EnemyState::Idle => {
                if !player_alive {
                    return EnemyState::Idle;
                }
                if dist_to_player < self.detect_range && self.idle_timer <= 0.0 {
                    return EnemyState::Chase;
                }
                EnemyState::Idle
            }
            EnemyState::Chase => {
                if !player_alive {
                    return EnemyState::Idle;
                }
                if dist_to_player <= self.attack_range && self.attack_cooldown <= 0.0 {
                    return EnemyState::Attack;
                }
                if dist_to_player > self.detect_range * 1.5 {
                    return EnemyState::Idle;
                }
                EnemyState::Chase
            }
            EnemyState::Attack => {
                if self.state_timer <= 0.0 {
                    return EnemyState::Chase;
                }
                EnemyState::Attack
            }
            EnemyState::Hurt => {
                if self.state_timer <= 0.0 {
                    if self.hp <= 0 {
                        return EnemyState::Die;
                    }
                    return EnemyState::Chase;
                }
                EnemyState::Hurt
            }
            EnemyState::Die => EnemyState::Die,
        }
    }

    fn enter_state(&mut self, state: EnemyState) {
        self.state = state;
        match state {
            EnemyState::Idle => {
                self.idle_timer = 1.0;
            }
            EnemyState::Attack => {
                self.state_timer = 0.5;
                self.attack_cooldown = 1.5;
            }
            EnemyState::Hurt => {
                self.state_timer = 0.3;
            }
            EnemyState::Die => {
                self.velocity = Vec3::ZERO;
                self.active = false;
            }
            _ => {}
        }
    }

    /// Apply damage to the enemy.
    pub fn take_damage(&mut self, damage: i32, from_dir: Vec3) {
        if !self.active || self.state == EnemyState::Die {
            return;
        }

        self.hp -= damage.max(1);
        // Knockback
        self.velocity = from_dir * 5.0;

        if self.hp <= 0 {
            self.hp = 0;
            self.enter_state(EnemyState::Die);
        } else {
            self.enter_state(EnemyState::Hurt);
        }
    }

    /// Check if this enemy is attacking and within range of the player.
    pub fn can_hit_player(&self, player_pos: Vec3) -> bool {
        if self.state != EnemyState::Attack {
            return false;
        }
        // Deal damage at the midpoint of the attack animation
        if self.state_timer > 0.25 && self.state_timer < 0.35 {
            let dist = (player_pos - self.pos).length();
            return dist < self.attack_range * 1.5;
        }
        false
    }

    pub fn state_name(&self) -> &'static str {
        match self.state {
            EnemyState::Idle => "Idle",
            EnemyState::Chase => "Chase",
            EnemyState::Attack => "Attack",
            EnemyState::Hurt => "Hurt",
            EnemyState::Die => "Die",
        }
    }
}

/// Manages all enemies in the scene.
pub struct EnemyManager {
    pub enemies: Vec<Enemy>,
}

impl EnemyManager {
    pub fn new() -> Self {
        Self {
            enemies: Vec::new(),
        }
    }

    pub fn spawn(&mut self, pos: Vec3, enemy_type: EnemyType, entity_idx: usize, model_name: Option<String>) {
        let mut enemy = Enemy::new(pos, enemy_type, entity_idx);
        enemy.model_name = model_name;
        self.enemies.push(enemy);
    }

    pub fn update(&mut self, player_pos: Vec3, player_alive: bool, dt: f32) {
        for enemy in &mut self.enemies {
            enemy.update(player_pos, player_alive, dt);
        }
    }

    pub fn active_count(&self) -> usize {
        self.enemies.iter().filter(|e| e.active).count()
    }
}
