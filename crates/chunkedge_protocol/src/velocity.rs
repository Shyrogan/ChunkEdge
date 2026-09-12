use std::fmt;
use std::io::Write;

use anyhow::{Context, ensure};
use chunkedge_binary::{Decode, Encode, VarInt};
use chunkedge_math::DVec3;
use derive_more::{From, Into};

/// Quantized entity velocity in 1/8000 blocks-per-tick units.
///
/// Since 26.x this is serialized on the wire with vanilla's compact
/// low-precision vector codec (previously three raw shorts); the in-memory
/// representation is unchanged.
#[derive(Copy, Clone, PartialEq, Eq, From, Into)]
pub struct Velocity(pub [i16; 3]);

impl Encode for Velocity {
    fn encode(&self, w: impl std::io::Write) -> anyhow::Result<()> {
        encode_lp_vec3(DVec3::from_array(self.0.map(|v| f64::from(v) / 8000.0)), w)
    }
}

impl Decode<'_> for Velocity {
    fn decode(r: &mut &[u8]) -> anyhow::Result<Self> {
        Ok(Self(
            decode_lp_vec3(r)?.to_array().map(|v| (v * 8000.0) as i16),
        ))
    }
}

impl Velocity {
    /// From meters/second.
    pub fn from_ms_f32(ms: [f32; 3]) -> Self {
        Self(ms.map(|v| (8000.0 / 20.0 * v) as i16))
    }

    /// From meters/second.
    pub fn from_ms_f64(ms: [f64; 3]) -> Self {
        Self(ms.map(|v| (8000.0 / 20.0 * v) as i16))
    }

    /// To meters/second.
    pub fn to_ms_f32(self) -> [f32; 3] {
        self.0.map(|v| f32::from(v) / (8000.0 / 20.0))
    }

    /// To meters/second.
    pub fn to_ms_f64(self) -> [f64; 3] {
        self.0.map(|v| f64::from(v) / (8000.0 / 20.0))
    }
}

impl fmt::Debug for Velocity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for Velocity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [x, y, z] = self.to_ms_f32();
        write!(f, "⟨{x},{y},{z}⟩ m/s")
    }
}

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

/// Encodes a vector in blocks per tick with vanilla's low-precision codec
/// (`net.minecraft.network.LpVec3`): each component is quantized to 15 bits
/// relative to a shared scale factor.
fn encode_lp_vec3(vec: DVec3, mut w: impl Write) -> anyhow::Result<()> {
    let x = sanitize(vec.x);
    let y = sanitize(vec.y);
    let z = sanitize(vec.z);

    let chessboard_length = x.abs().max(y.abs()).max(z.abs());

    if chessboard_length < ABS_MIN_VALUE {
        w.write_all(&[0]).context("failed to write velocity")?;
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

    // Matches vanilla `LpVec3.write`: two raw bytes followed by a big-endian
    // int (`FriendlyByteBuf.writeInt`).
    w.write_all(&[buffer as u8, (buffer >> 8) as u8])
        .context("failed to write velocity")?;
    w.write_all(&((buffer >> 16) as u32).to_be_bytes())
        .context("failed to write velocity")?;

    if is_partial {
        VarInt((scale >> 2) as i32)
            .encode(&mut w)
            .context("failed to write velocity scale")?;
    }

    Ok(())
}

fn decode_lp_vec3(r: &mut &[u8]) -> anyhow::Result<DVec3> {
    ensure!(!r.is_empty(), "unexpected end of input decoding velocity");
    let lowest = u64::from(r[0]);
    *r = &r[1..];

    if lowest == 0 {
        return Ok(DVec3::new(0.0, 0.0, 0.0));
    }

    ensure!(r.len() >= 5, "unexpected end of input decoding velocity");
    let middle = u64::from(r[0]);
    // Vanilla reads the upper word with `readUnsignedInt` (big-endian).
    let highest = u64::from(u32::from_be_bytes([r[1], r[2], r[3], r[4]]));
    *r = &r[5..];

    let buffer = highest << 16 | middle << 8 | lowest;

    let mut scale = lowest & SCALE_BITS_MASK;
    if lowest & CONTINUATION_FLAG == CONTINUATION_FLAG {
        scale |= (VarInt::decode(r)?.0 as u64) << 2;
    }

    Ok(DVec3::new(
        unpack(buffer >> 3) * scale as f64,
        unpack(buffer >> 18) * scale as f64,
        unpack(buffer >> 33) * scale as f64,
    ))
}

#[cfg(test)]
#[test]
fn velocity_from_ms() {
    let val_1 = Velocity::from_ms_f32([(); 3].map(|()| -3.3575)).0[0];
    let val_2 = Velocity::from_ms_f64([(); 3].map(|()| -3.3575)).0[0];

    assert_eq!(val_1, val_2);
    assert_eq!(val_1, -1343);
}

#[cfg(test)]
#[test]
fn velocity_wire_format_matches_vanilla() {
    // 8000 units = exactly 1 block/tick, so this must match vanilla's
    // low-precision encoding of (1.0, 0.0, 0.0): two raw bytes followed by
    // a big-endian int. A little-endian upper word would instead end in
    // `[0xff, 0xff, 0xfe, 0x7f]`.
    let mut buf = Vec::new();
    Velocity([8000, 0, 0]).encode(&mut buf).unwrap();
    assert_eq!(buf, vec![0xf1, 0xff, 0x7f, 0xfe, 0xff, 0xff]);

    let decoded = Velocity::decode(&mut &buf[..]).unwrap();
    assert_eq!(decoded, Velocity([8000, 0, 0]));
}

#[cfg(test)]
#[test]
fn lp_vec3_roundtrip() {
    for vec in [
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(0.5, -0.25, 0.125),
        DVec3::new(1.5, 64.0, -300.25),
        DVec3::new(0.00001, 0.0, 0.0),
    ] {
        let mut buf = Vec::new();
        encode_lp_vec3(vec, &mut buf).unwrap();
        let decoded = decode_lp_vec3(&mut &buf[..]).unwrap();
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
