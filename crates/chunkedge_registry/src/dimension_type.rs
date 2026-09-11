//! Contains dimension types and the dimension type registry. Minecraft's
//! default dimensions are added to the registry by default.
//!
//! ### **NOTE:**
//! - Modifying the dimension type registry after the server has started can
//!   break invariants within instances and clients! Make sure there are no
//!   instances or clients spawned before mutating.

use std::ops::{Deref, DerefMut};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use chunkedge_ident::{Ident, ident};
use chunkedge_nbt::serde::ser::CompoundSerializer;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::error;

use crate::codec::{RegistryCodec, RegistryValue};
use crate::{Registry, RegistryIdx, RegistrySet};
pub struct DimensionTypePlugin;

impl Plugin for DimensionTypePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DimensionTypeRegistry>()
            .add_systems(PreStartup, load_default_dimension_types)
            .add_systems(
                PostUpdate,
                update_dimension_type_registry.before(RegistrySet),
            );
    }
}

/// Loads the default dimension types from the registry codec.
fn load_default_dimension_types(mut reg: ResMut<DimensionTypeRegistry>, codec: Res<RegistryCodec>) {
    let mut helper = move || -> anyhow::Result<()> {
        for value in codec.registry(DimensionTypeRegistry::KEY) {
            let mut dimension_type = DimensionType::deserialize(value.element.clone())?;

            // HACK: We don't have a lighting engine implemented. To avoid shrouding the
            // world in darkness, give all dimensions the max ambient light.
            dimension_type.ambient_light = 1.0;

            reg.insert(value.name.clone(), dimension_type);
        }

        Ok(())
    };

    if let Err(e) = helper() {
        error!("failed to load default dimension types from registry codec: {e:#}");
    }
}

/// Updates the registry codec as the dimension type registry is modified by
/// users.
fn update_dimension_type_registry(
    reg: Res<DimensionTypeRegistry>,
    mut codec: ResMut<RegistryCodec>,
) {
    if reg.is_changed() {
        let dimension_types = codec.registry_mut(DimensionTypeRegistry::KEY);

        dimension_types.clear();

        dimension_types.extend(reg.iter().map(|(_, name, dim)| {
            RegistryValue {
                name: name.into(),
                element: dim
                    .serialize(CompoundSerializer)
                    .expect("failed to serialize dimension type"),
            }
        }));
    }
}

#[derive(Resource, Default, Debug)]
pub struct DimensionTypeRegistry {
    reg: Registry<DimensionTypeId, DimensionType>,
}

impl DimensionTypeRegistry {
    pub const KEY: Ident<&'static str> = ident!("dimension_type");
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct DimensionTypeId(u16);

impl DimensionTypeId {
    pub fn new(value: u16) -> Self {
        DimensionTypeId(value)
    }
    pub fn get_value(&self) -> u16 {
        self.0
    }
}

impl RegistryIdx for DimensionTypeId {
    const MAX: usize = u16::MAX as usize;

    fn to_index(self) -> usize {
        self.0 as usize
    }

    fn from_index(idx: usize) -> Self {
        Self(idx as u16)
    }
}

impl Deref for DimensionTypeRegistry {
    type Target = Registry<DimensionTypeId, DimensionType>;

    fn deref(&self) -> &Self::Target {
        &self.reg
    }
}

impl DerefMut for DimensionTypeRegistry {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.reg
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct DimensionType {
    pub ambient_light: f32,
    pub coordinate_scale: f64,
    #[serde(default)]
    pub has_fixed_time: bool,
    pub has_skylight: bool,
    pub has_ceiling: bool,
    #[serde(default)]
    pub has_ender_dragon_fight: bool,
    pub height: i32,
    pub infiniburn: String,
    pub logical_height: i32,
    pub min_y: i32,
    pub monster_spawn_block_light_limit: i32,
    pub monster_spawn_light_level: MonsterSpawnLightLevel,
    /// Environment attributes introduced in 26.1. Preserved verbatim in the
    /// login codec, but not re-serialized on runtime mutation.
    #[serde(default, skip_serializing)]
    pub attributes: BTreeMap<String, serde::de::IgnoredAny>,
    /// Timelines (world clocks) reference, e.g. `"#minecraft:in_overworld"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timelines: Option<String>,
    /// Default world clock, e.g. `"minecraft:overworld"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_clock: Option<String>,
    /// Skybox override, e.g. `"end"` or `"none"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skybox: Option<String>,
    /// Cardinal lighting override, e.g. `"nether"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cardinal_light: Option<String>,
}

impl Default for DimensionType {
    fn default() -> Self {
        Self {
            ambient_light: 0.0,
            coordinate_scale: 1.0,
            has_fixed_time: false,
            has_ceiling: false,
            has_skylight: true,
            height: 384,
            infiniburn: "#minecraft:infiniburn_overworld".into(),
            logical_height: 384,
            min_y: -64,
            monster_spawn_block_light_limit: 0,
            monster_spawn_light_level: MonsterSpawnLightLevel::Int(7),
            has_ender_dragon_fight: false,
            attributes: BTreeMap::new(),
            timelines: None,
            default_clock: None,
            skybox: None,
            cardinal_light: None,
        }
    }
}

/// Determines what skybox/fog effects to use in dimensions.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum DimensionEffects {
    #[serde(rename = "minecraft:overworld")]
    #[default]
    Overworld,
    #[serde(rename = "minecraft:the_nether")]
    TheNether,
    #[serde(rename = "minecraft:the_end")]
    TheEnd,
}

#[derive(Copy, Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MonsterSpawnLightLevel {
    Int(i32),
    Tagged(MonsterSpawnLightLevelTagged),
}

#[derive(Copy, Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MonsterSpawnLightLevelTagged {
    #[serde(rename = "minecraft:uniform")]
    Uniform {
        min_inclusive: i32,
        max_inclusive: i32,
    },
}

impl From<i32> for MonsterSpawnLightLevel {
    fn from(value: i32) -> Self {
        Self::Int(value)
    }
}
