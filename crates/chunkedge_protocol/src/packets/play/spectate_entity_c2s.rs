use chunkedge_binary::{Decode, Encode, VarInt};

use crate::Packet;

/// Requests spectating an entity. Distinct from teleport-to-entity.
///
/// wiki: [Spectate Entity](https://minecraft.wiki/w/Java_Edition_protocol#Spectate_Entity)
#[derive(Copy, Clone, Debug, Encode, Decode, Packet)]
pub struct SpectateEntityC2s {
    pub entity_id: VarInt,
}
