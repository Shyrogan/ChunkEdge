use std::borrow::Cow;
use std::io::Write;

use anyhow::ensure;
use chunkedge_binary::{Decode, Encode, VarInt};
use chunkedge_ident::Ident;
use chunkedge_nbt::Compound;

use crate::Packet;

const MAX_PAYLOAD_SIZE: usize = 0x10000;

/// Carries the payload of a custom click action from a dialog or message.
/// The `payload` is length-prefixed untrusted NBT.
///
/// See [`ServerboundCustomClickActionPacket`](https://minecraft.wiki/w/Java_Edition_protocol#Custom_Click_Action).
#[derive(Clone, Debug, Encode, Decode, Packet)]
pub struct CustomClickActionC2s<'a> {
    pub id: Ident<Cow<'a, str>>,
    pub payload: LengthPrefixedNbt,
}

/// Length-prefixed optional NBT, as used by vanilla
/// `ByteBufCodecs.optionalTagCodec(...).apply(ByteBufCodecs.lengthPrefixed(..))`:
/// a VarInt byte count followed by the TAG bytes. An absent payload is a
/// count of one followed by a single `TAG_End` byte.
#[derive(Clone, Debug, Default)]
pub struct LengthPrefixedNbt(pub Option<Compound>);

impl Encode for LengthPrefixedNbt {
    fn encode(&self, mut w: impl Write) -> anyhow::Result<()> {
        let mut buf = Vec::new();
        match &self.0 {
            Some(tag) => tag.encode(&mut buf)?,
            None => buf.push(0),
        }

        ensure!(
            buf.len() <= MAX_PAYLOAD_SIZE,
            "custom click action payload exceeds max of {MAX_PAYLOAD_SIZE} bytes"
        );

        VarInt(buf.len() as i32).encode(&mut w)?;
        w.write_all(&buf)?;
        Ok(())
    }
}

impl Decode<'_> for LengthPrefixedNbt {
    fn decode(r: &mut &[u8]) -> anyhow::Result<Self> {
        let len = VarInt::decode(r)?.0 as usize;

        ensure!(
            len <= MAX_PAYLOAD_SIZE,
            "custom click action payload exceeds max of {MAX_PAYLOAD_SIZE} bytes"
        );
        ensure!(
            r.len() >= len,
            "custom click action payload of {len} bytes exceeds remaining input"
        );

        let (mut bytes, rest) = r.split_at(len);
        *r = rest;

        let tag = Compound::decode(&mut bytes)?;
        Ok(Self(if tag.is_empty() { None } else { Some(tag) }))
    }
}
