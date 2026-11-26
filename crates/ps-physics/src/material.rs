//! Surface materials: penetration and presentation properties.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct MaterialId(pub u16);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialDef {
    pub name: String,
    /// Hardness: penetration power consumed per 8 units of thickness
    /// (see `engine_weapons::damage::penetrate`). `0` = cannot be penetrated;
    /// higher means harder.
    pub penetration_modifier: f32,
    /// Fraction of damage kept after crossing one unit of this material (applied
    /// per unit, deterministic): `damage × retention^thickness`.
    pub damage_retention_per_unit: f32,
    /// Fixed fraction of damage lost on entering the surface.
    pub entry_loss: f32,
    /// Render colour (linear RGB).
    pub color: [f32; 3],
    /// Footstep/impact sound family understood by the client.
    pub sound: String,
}

impl Default for MaterialDef {
    fn default() -> Self {
        Self {
            name: "default".into(),
            penetration_modifier: 0.0,
            damage_retention_per_unit: 1.0,
            entry_loss: 0.0,
            color: [0.6, 0.6, 0.6],
            sound: "concrete".into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MaterialTable {
    pub materials: Vec<MaterialDef>,
}

impl MaterialTable {
    pub fn get(&self, id: MaterialId) -> &MaterialDef {
        static FALLBACK: std::sync::OnceLock<MaterialDef> = std::sync::OnceLock::new();
        self.materials
            .get(id.0 as usize)
            .unwrap_or_else(|| FALLBACK.get_or_init(MaterialDef::default))
    }

    pub fn id_of(&self, name: &str) -> Option<MaterialId> {
        self.materials
            .iter()
            .position(|m| m.name == name)
            .map(|i| MaterialId(i as u16))
    }

    pub fn validate(&self) -> Result<(), String> {
        for m in &self.materials {
            if !(0.0..=1.0).contains(&m.damage_retention_per_unit)
                || !(0.0..=1.0).contains(&m.entry_loss)
            {
                return Err(format!(
                    "material {}: retention/entry_loss must be in [0,1]",
                    m.name
                ));
            }
            if m.penetration_modifier < 0.0 {
                return Err(format!("material {}: penetration_modifier < 0", m.name));
            }
        }
        Ok(())
    }
}
