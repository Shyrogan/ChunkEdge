use std::io::Write;

use anyhow::{Context, ensure};
use chunkedge_binary::{Decode, Encode, VarInt};
use chunkedge_math::DVec3;
use derive_more::{From, Into};

/// A lossily quantized 3D vector, as used by vanilla's `LpVec3` codec (e.g.
/// the interaction target in [`InteractC2s`](crate::packets::play::interact_c2s::InteractC2s)).
///
/// Each component is quantized to 15 bits with a shared power-of-two-ish
/// scale, so small vectors encode in as little as two bytes.
#[derive(Copy, Clone, PartialEq, Debug, From, Into)]
pub struct LpVec3(pub DVec3);

const DATA_BITS_MASK: u64 = 0x7fff;
const MAX_QUANTIZED_VALUE: f64 = 32766.0;
const SCALE_BITS_MASK: u64 = 0x3;
const CONTINUATION_FLAG: u64 = 0x4;
const ABS_MAX_VALUE: f64 = 17179869183.0;
const ABS_MIN_VALUE: f64 = 0.00003051944088384301;

fn sanitize(value: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(-ABS_MAX_VALUE, ABS_MAX_VALUE)
    }
}

fn pack(value: f64) -> u64 {
    ((value * 0.5 + 0.5) * MAX_QUANTIZED_VALUE).round() as u64
}

fn unpack(value: u64) -> f64 {
    (value & DATA_BITS_MASK).min(MAX_QUANTIZED_VALUE as u64) as f64 * 2.0 / MAX_QUANTIZED_VALUE
        - 1.0
}

impl Encode for LpVec3 {
    fn encode(&self, mut w: impl Write) -> anyhow::Result<()> {
        let x = sanitize(self.0.x);
        let y = sanitize(self.0.y);
        let z = sanitize(self.0.z);

        let chessboard_length = x.abs().max(y.abs()).max(z.abs());

        if chessboard_length < ABS_MIN_VALUE {
            w.write_all(&[0]).context("failed to write LpVec3")?;
            return Ok(());
        }

        let scale = chessboard_length.ceil() as u64;
        let is_partial = (scale & SCALE_BITS_MASK) != scale;
        let markers = if is_partial {
            scale & SCALE_BITS_MASK | CONTINUATION_FLAG
        } else {
            scale
        };

        let buffer = markers
            | (pack(x / scale as f64) << 3)
            | (pack(y / scale as f64) << 18)
            | (pack(z / scale as f64) << 33);

        w.write_all(&buffer.to_le_bytes()[..6])
            .context("failed to write LpVec3")?;

        if is_partial {
            VarInt((scale >> 2) as i32)
                .encode(&mut w)
                .context("failed to write LpVec3 scale")?;
        }

        Ok(())
    }
}

impl Decode<'_> for LpVec3 {
    fn decode(r: &mut &[u8]) -> anyhow::Result<Self> {
        ensure!(!r.is_empty(), "unexpected end of input decoding LpVec3");
        let lowest = u64::from(r[0]);
        *r = &r[1..];

        if lowest == 0 {
            return Ok(Self(DVec3::new(0.0, 0.0, 0.0)));
        }

        ensure!(r.len() >= 5, "unexpected end of input decoding LpVec3");
        let middle = u64::from(r[0]);
        let highest = u64::from(u32::from_le_bytes([r[1], r[2], r[3], r[4]]));
        *r = &r[5..];

        let buffer = highest << 16 | middle << 8 | lowest;

        let mut scale = lowest & SCALE_BITS_MASK;
        if lowest & CONTINUATION_FLAG == CONTINUATION_FLAG {
            scale |= (VarInt::decode(r)?.0 as u64) << 2;
            // Recompute the packed buffer contribution of the extended scale?
            // No: the buffer bytes are already fully read; only the scale grows.
            let _ = buffer;
        }

        Ok(Self(DVec3::new(
            unpack(buffer >> 3) * scale as f64,
            unpack(buffer >> 18) * scale as f64,
            unpack(buffer >> 33) * scale as f64,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lp_vec3_roundtrip() {
        for vec in [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(0.5, -0.25, 0.125),
            DVec3::new(1.5, 64.0, -300.25),
            DVec3::new(0.00001, 0.0, 0.0),
        ] {
            let mut buf = Vec::new();
            LpVec3(vec).encode(&mut buf).unwrap();
            let decoded = LpVec3::decode(&mut &buf[..]).unwrap().0;
            // The format quantizes each component to 15 bits relative to the
            // shared scale, so allow about one quantization step of error.
            let scale = vec
                .x
                .abs()
                .max(vec.y.abs())
                .max(vec.z.abs())
                .ceil()
                .max(1.0);
            let tolerance = scale / 32766.0 * 1.5 + 0.0001;
            for (a, b) in [vec.x, vec.y, vec.z]
                .into_iter()
                .zip([decoded.x, decoded.y, decoded.z])
            {
                assert!(
                    (a - b).abs() <= tolerance,
                    "roundtrip failed for {vec:?}: got {decoded:?}"
                );
            }
        }
    }
}
