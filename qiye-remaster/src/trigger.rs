/// Trigger volume system.
///
/// Triggers are AABB volumes from BSP entities. When the camera/player
/// enters a trigger, the associated action fires.

use crate::entity::{EntityManager, EntityTypeId};
use glam::Vec3;

#[derive(Debug, Clone)]
pub struct TriggerVolume {
    pub entity_idx: usize,
    pub type_id: EntityTypeId,
    pub center: Vec3,
    pub half_extents: Vec3,
    pub was_inside: bool,
}

impl TriggerVolume {
    pub fn contains(&self, point: Vec3) -> bool {
        let d = point - self.center;
        d.x.abs() <= self.half_extents.x
            && d.y.abs() <= self.half_extents.y
            && d.z.abs() <= self.half_extents.z
    }
}

pub struct TriggerSystem {
    triggers: Vec<TriggerVolume>,
}

#[derive(Debug)]
pub struct TriggerEvent {
    pub trigger_idx: usize,
    pub type_id: EntityTypeId,
    pub entered: bool, // true=entered, false=exited
}

impl TriggerSystem {
    pub fn new() -> Self {
        Self {
            triggers: Vec::new(),
        }
    }

    /// Build trigger volumes from BSP entity data.
    pub fn load_from_entities(&mut self, entities: &EntityManager) {
        self.triggers.clear();

        for (idx, ent) in entities.entities().iter().enumerate() {
            // Only trigger-type entities become volumes
            match ent.type_id {
                EntityTypeId::ActTrigger
                | EntityTypeId::DoorTrigger
                | EntityTypeId::CameraTrigger
                | EntityTypeId::EventTrigger
                | EntityTypeId::TalkTrigger
                | EntityTypeId::PickTrigger => {}
                _ => continue,
            }

            let half = ent.bbox_extents * 0.5;
            // Skip tiny or zero-size triggers
            if half.length_squared() < 0.01 {
                continue;
            }

            self.triggers.push(TriggerVolume {
                entity_idx: idx,
                type_id: ent.type_id,
                center: ent.transform.position,
                half_extents: half,
                was_inside: false,
            });
        }

        if !self.triggers.is_empty() {
            let mut counts = std::collections::HashMap::new();
            for t in &self.triggers {
                *counts.entry(format!("{:?}", t.type_id)).or_insert(0) += 1;
            }
            let mut types: Vec<_> = counts.into_iter().collect();
            types.sort();
            let summary: Vec<String> = types.iter().map(|(k, v)| format!("{k}:{v}")).collect();
            println!("Triggers: {} volumes ({})", self.triggers.len(), summary.join(", "));
        }
    }

    /// Check all triggers against a point, returning enter/exit events.
    pub fn check(&mut self, point: Vec3) -> Vec<TriggerEvent> {
        let mut events = Vec::new();

        for (i, trigger) in self.triggers.iter_mut().enumerate() {
            let inside = trigger.contains(point);
            if inside && !trigger.was_inside {
                events.push(TriggerEvent {
                    trigger_idx: i,
                    type_id: trigger.type_id,
                    entered: true,
                });
            } else if !inside && trigger.was_inside {
                events.push(TriggerEvent {
                    trigger_idx: i,
                    type_id: trigger.type_id,
                    entered: false,
                });
            }
            trigger.was_inside = inside;
        }

        events
    }

    pub fn triggers(&self) -> &[TriggerVolume] {
        &self.triggers
    }
}
