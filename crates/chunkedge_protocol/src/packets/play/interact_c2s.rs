use chunkedge_binary::{Decode, Encode, VarInt};

use crate::{Hand, LpVec3, Packet};
use chunkedge_math::DVec3;

/// Sent when a player interacts with (right-clicks) an entity. In 26.1 the
/// attack action was split out into [`AttackC2s`](super::attack_c2s::AttackC2s),
/// so this packet is now always an interaction.
///
/// The `target` is a lossily quantized vector relative to the entity. See
/// [`LpVec3`].
///
/// wiki: [Interact](https://minecraft.wiki/w/Java_Edition_protocol#Interact)
#[derive(Copy, Clone, Debug, Encode, Decode, Packet)]
pub struct InteractC2s {
    pub entity_id: VarInt,
    pub hand: Hand,
    pub target: LpVec3,
    pub sneaking: bool,
}

impl InteractC2s {
    pub fn target_vec(&self) -> DVec3 {
        self.target.0
    }
}
