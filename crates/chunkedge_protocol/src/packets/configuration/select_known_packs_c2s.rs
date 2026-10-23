use chunkedge_binary::{Bounded, Decode, Encode};

use super::select_known_packs_s2c::KnownPack;
use crate::{Packet, PacketState};

/// Maximum number of known packs accepted from a client, matching vanilla.
pub const MAX_KNOWN_PACKS: usize = 64;

#[derive(Clone, Debug, Encode, Decode, Packet)]
#[packet(state = PacketState::Configuration)]
pub struct SelectKnownPacksC2s<'a> {
    pub packs: Bounded<Vec<KnownPack<'a>>, MAX_KNOWN_PACKS>,
}

#[cfg(test)]
mod tests {
    use chunkedge_binary::VarInt;

    use super::*;

    fn encode_packs(count: usize) -> Vec<u8> {
        let mut buf = vec![];
        VarInt(count as i32).encode(&mut buf).unwrap();
        for _ in 0..count {
            for field in ["minecraft", "core", "26.2"] {
                field.encode(&mut buf).unwrap();
            }
        }
        buf
    }

    #[test]
    fn accepts_vanilla_limit() {
        let buf = encode_packs(MAX_KNOWN_PACKS);
        let pkt = SelectKnownPacksC2s::decode(&mut buf.as_slice()).unwrap();
        assert_eq!(pkt.packs.len(), MAX_KNOWN_PACKS);
    }

    #[test]
    fn rejects_too_many_packs() {
        let buf = encode_packs(MAX_KNOWN_PACKS + 1);
        assert!(SelectKnownPacksC2s::decode(&mut buf.as_slice()).is_err());
    }
}
