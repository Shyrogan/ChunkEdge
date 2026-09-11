use chunkedge_binary::{Decode, Encode, VarInt};

use crate::Packet;

/// Requests a spectator action on an entity.
///
/// wiki: [Spectator Action](https://minecraft.wiki/w/Java_Edition_protocol#Spectator_Action)
#[derive(Copy, Clone, Debug, Encode, Decode, Packet)]
pub struct SpectatorActionC2s {
    pub entity_id: VarInt,
}
