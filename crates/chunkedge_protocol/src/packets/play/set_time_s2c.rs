use chunkedge_binary::{Decode, Encode};
use chunkedge_ident::Ident;

use crate::Packet;

#[derive(Clone, Debug, Encode, Decode, Packet)]
pub struct SetTimeS2c {
    /// The age of the world in 1/20ths of a second.
    pub world_age: i64,
    /// Per-clock network states, keyed by world-clock registry ID. In
    /// practice this contains a single entry for the default clock.
    pub clocks: Vec<ClockState>,
}

#[derive(Clone, Debug, Encode, Decode)]
pub struct ClockState {
    pub clock: Ident<String>,
    /// Total ticks.
    pub total_ticks: i64,
    /// Partial tick progress in `[0, 1)`.
    pub partial_tick: f32,
    /// Tick rate multiplier.
    pub rate: f32,
}
