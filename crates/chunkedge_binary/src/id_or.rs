use std::fmt::Debug;
use std::io::Write;

use anyhow::Error;
use chunkedge_generated::registry_id::RegistryId;

use crate::{Decode, Encode, VarInt};

#[derive(Clone, Debug, PartialEq)]
pub enum IdOr<T: Encode + Clone + Debug + PartialEq> {
    Id(RegistryId),
    Inline(T),
}

impl<T: Encode + Clone + Debug + PartialEq> IdOr<T> {
    pub fn id<I: Into<RegistryId>>(id: I) -> Self {
        Self::Id(id.into())
    }

    pub fn inline(value: T) -> Self {
        Self::Inline(value)
    }
}

impl<T: Encode + Clone + Debug + PartialEq> Encode for IdOr<T> {
    fn encode(&self, mut buf: impl Write) -> anyhow::Result<()> {
        match self {
            Self::Id(id) => VarInt(id.id() + 1).encode(buf),
            Self::Inline(value) => {
                VarInt(0).encode(&mut buf)?;
                value.encode(&mut buf)
            }
        }
    }
}

impl<'a, T: Decode<'a> + Encode + Clone + Debug + PartialEq> Decode<'a> for IdOr<T> {
    fn decode(buf: &mut &'a [u8]) -> Result<Self, Error> {
        let id = VarInt::decode(buf)?;
        if id == VarInt(0) {
            let value = T::decode(buf)?;
            Ok(Self::Inline(value))
        } else {
            let id =
                id.0.checked_sub(1)
                    .ok_or_else(|| anyhow::anyhow!("registry ID {} out of range", id.0))?;
            let registry_id = RegistryId::new(id);
            Ok(Self::Id(registry_id))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_id_overflow() {
        let mut buf = vec![];
        VarInt(i32::MIN).encode(&mut buf).unwrap();
        assert!(IdOr::<VarInt>::decode(&mut buf.as_slice()).is_err());
    }

    #[test]
    fn round_trip() {
        for value in [IdOr::id(RegistryId::new(5)), IdOr::inline(VarInt(9))] {
            let mut buf = vec![];
            value.encode(&mut buf).unwrap();
            assert_eq!(IdOr::<VarInt>::decode(&mut buf.as_slice()).unwrap(), value);
        }
    }
}
