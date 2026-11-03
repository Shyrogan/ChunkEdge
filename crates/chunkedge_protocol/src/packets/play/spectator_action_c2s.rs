use chunkedge_binary::{Decode, Encode, VarInt};

use crate::Packet;

/// Requests a spectator action on an entity.
#[derive(Copy, Clone, Debug, Encode, Decode, Packet)]
pub struct SpectatorActionC2s {
    pub entity_id: SpectatorTarget,
}

/// An optionally-present spectated entity, encoded like vanilla
/// `OPTIONAL_VAR_INT`: `0` means no target, otherwise the entity ID plus one.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct SpectatorTarget(pub Option<VarInt>);

impl Encode for SpectatorTarget {
    fn encode(&self, w: impl std::io::Write) -> anyhow::Result<()> {
        match self.0 {
            None => VarInt(0).encode(w),
            Some(id) => VarInt(id.0 + 1).encode(w),
        }
    }
}

impl Decode<'_> for SpectatorTarget {
    fn decode(r: &mut &[u8]) -> anyhow::Result<Self> {
        Ok(Self(match VarInt::decode(r)?.0 {
            0 => None,
            n => Some(VarInt(n.checked_sub(1).ok_or_else(|| {
                anyhow::anyhow!("spectator target ID {n} out of range")
            })?)),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_target_overflow() {
        let mut buf = vec![];
        VarInt(i32::MIN).encode(&mut buf).unwrap();
        assert!(SpectatorTarget::decode(&mut buf.as_slice()).is_err());
    }

    #[test]
    fn round_trip() {
        for target in [SpectatorTarget(None), SpectatorTarget(Some(VarInt(41)))] {
            let mut buf = vec![];
            target.encode(&mut buf).unwrap();
            assert_eq!(
                SpectatorTarget::decode(&mut buf.as_slice()).unwrap(),
                target
            );
        }
    }
}
