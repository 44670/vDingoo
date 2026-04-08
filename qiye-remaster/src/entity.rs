use crate::bsp;
use glam::{Mat4, Vec3};

/// Entity type IDs matching BSP entity types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i16)]
pub enum EntityTypeId {
    SceneObject = 0,
    Player = 1,
    Creature = 2,
    Enemy = 3,
    PickTrigger = 4,
    ActTrigger = 5,
    CameraTrigger = 6,
    DoorTrigger = 7,
    EventTrigger = 8,
    TalkTrigger = 9,
    CameraSpot = 10,
    Unknown = -1,
}

impl EntityTypeId {
    pub fn from_i16(v: i16) -> Self {
        match v {
            0 => Self::SceneObject,
            1 => Self::Player,
            2 => Self::Creature,
            3 => Self::Enemy,
            4 => Self::PickTrigger,
            5 => Self::ActTrigger,
            6 => Self::CameraTrigger,
            7 => Self::DoorTrigger,
            8 => Self::EventTrigger,
            9 => Self::TalkTrigger,
            10 => Self::CameraSpot,
            _ => Self::Unknown,
        }
    }

    pub fn color(&self) -> [f32; 3] {
        match self {
            Self::SceneObject => [0.8, 0.8, 0.8],    // white-ish (static props)
            Self::Player => [0.0, 1.0, 0.0],          // green
            Self::Creature => [0.0, 0.8, 1.0],        // cyan
            Self::Enemy => [1.0, 0.0, 0.0],           // red
            Self::PickTrigger => [1.0, 1.0, 0.0],     // yellow
            Self::ActTrigger => [1.0, 0.5, 0.0],      // orange
            Self::CameraTrigger => [0.5, 0.0, 1.0],   // purple
            Self::DoorTrigger => [0.0, 0.5, 1.0],     // blue
            Self::EventTrigger => [1.0, 0.0, 1.0],    // magenta
            Self::TalkTrigger => [0.5, 1.0, 0.5],     // light green
            Self::CameraSpot => [1.0, 1.0, 1.0],      // white
            Self::Unknown => [0.5, 0.5, 0.5],         // gray
        }
    }
}

pub struct Transform {
    pub position: Vec3,
    pub matrix: Mat4,
}

impl Transform {
    pub fn from_pos(pos: Vec3) -> Self {
        Self {
            position: pos,
            matrix: Mat4::from_translation(pos),
        }
    }

    pub fn from_bsp(pos: [f32; 3], transform: Option<[[f32; 3]; 3]>) -> Self {
        let position = Vec3::from(pos);
        let matrix = if let Some(rows) = transform {
            Mat4::from_cols(
                glam::Vec4::new(rows[0][0], rows[0][1], rows[0][2], 0.0),
                glam::Vec4::new(rows[1][0], rows[1][1], rows[1][2], 0.0),
                glam::Vec4::new(rows[2][0], rows[2][1], rows[2][2], 0.0),
                glam::Vec4::new(pos[0], pos[1], pos[2], 1.0),
            )
        } else {
            Mat4::from_translation(position)
        };
        Self { position, matrix }
    }
}

pub struct Entity {
    pub id: u32,
    pub type_id: EntityTypeId,
    pub transform: Transform,
    pub model_name: Option<String>,
    pub bbox_extents: Vec3,
    pub active: bool,
}

pub struct EntityManager {
    entities: Vec<Entity>,
    next_id: u32,
}

impl EntityManager {
    pub fn new() -> Self {
        Self {
            entities: Vec::new(),
            next_id: 1,
        }
    }

    /// Load entities from a parsed BSP.
    pub fn load_from_bsp(&mut self, bsp: &bsp::Bsp) {
        for ent in &bsp.entities {
            let type_id = EntityTypeId::from_i16(ent.entity_type);

            // Skip entities without position (can't place them)
            let pos = match ent.position {
                Some(p) => p,
                None => continue,
            };

            let bbox_extents = ent
                .bbox
                .map(Vec3::from)
                .unwrap_or(Vec3::new(1.0, 1.0, 1.0));

            let entity = Entity {
                id: self.next_id,
                type_id,
                transform: Transform::from_bsp(pos, ent.transform),
                model_name: ent.model_name.clone(),
                bbox_extents,
                active: true,
            };

            self.next_id += 1;
            self.entities.push(entity);
        }

        // Count by type
        let mut with_models = 0;
        for ent in &self.entities {
            if ent.model_name.is_some() {
                with_models += 1;
            }
        }
        println!(
            "EntityManager: loaded {} entities from BSP ({} with models)",
            self.entities.len(),
            with_models,
        );
    }

    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    #[allow(dead_code)]
    pub fn entities_mut(&mut self) -> &mut [Entity] {
        &mut self.entities
    }

    pub fn set_active(&mut self, idx: usize, active: bool) {
        if let Some(ent) = self.entities.get_mut(idx) {
            ent.active = active;
        }
    }

    #[allow(dead_code)]
    pub fn spawn(&mut self, type_id: EntityTypeId, pos: Vec3, model_name: Option<String>) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.entities.push(Entity {
            id,
            type_id,
            transform: Transform::from_pos(pos),
            model_name,
            bbox_extents: Vec3::new(1.0, 2.0, 1.0),
            active: true,
        });
        id
    }
}
