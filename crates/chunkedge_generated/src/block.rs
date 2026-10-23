#![allow(clippy::all)] // TODO: block build script creates many warnings.

use std::fmt;
use std::fmt::Display;
use std::iter::FusedIterator;

use chunkedge_ident::{Ident, ident};

use crate::item::ItemKind;

include!(concat!(env!("OUT_DIR"), "/block.rs"));
include!(concat!(env!("OUT_DIR"), "/block_physics.rs"));
include!(concat!(env!("OUT_DIR"), "/block_tag.rs"));

impl BlockState {
    /// The offset vanilla adds to this block's shapes at the block position
    /// `(x, _, z)`, or zero if the block isn't offset (vanilla
    /// `BlockStateBase#getOffset`). The height of the position never matters.
    pub fn offset_at(self, x: i32, z: i32) -> chunkedge_math::DVec3 {
        self.to_kind()
            .offset()
            .map_or(chunkedge_math::DVec3::ZERO, |offset| offset.at(x, z))
    }
}

/// How a block's shapes are pseudo-randomly shifted depending on its position,
/// such as flowers, bamboo and pointed dripstone (vanilla
/// `BlockBehaviour.OffsetType`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BlockOffset {
    /// Shifted horizontally by at most `max_horizontal` on each axis.
    Xz { max_horizontal: f32 },
    /// Shifted horizontally like [`BlockOffset::Xz`] and down by at most
    /// `max_vertical`.
    Xyz {
        max_horizontal: f32,
        max_vertical: f32,
    },
}

impl BlockOffset {
    /// The offset at the block position `(x, _, z)`, bit-identical to the
    /// vanilla offset function.
    pub fn at(self, x: i32, z: i32) -> chunkedge_math::DVec3 {
        // Vanilla divides a 4-bit slice of the seed as `float`, then widens it
        // to `double` for the rest of the arithmetic.
        let nibble = |bits: i64| f64::from((bits & 15) as f32 / 15.0);

        let seed = position_seed(x, 0, z);
        let (max_horizontal, y) = match self {
            Self::Xz { max_horizontal } => (max_horizontal, 0.0),
            Self::Xyz {
                max_horizontal,
                max_vertical,
            } => (
                max_horizontal,
                (nibble(seed >> 4) - 1.0) * f64::from(max_vertical),
            ),
        };
        let max_horizontal = f64::from(max_horizontal);
        let horizontal =
            |bits: i64| ((nibble(bits) - 0.5) * 0.5).clamp(-max_horizontal, max_horizontal);

        chunkedge_math::DVec3::new(horizontal(seed), y, horizontal(seed >> 8))
    }
}

/// Vanilla `Mth.getSeed`. The `x` term is multiplied as a 32-bit `int` and
/// wraps before it is widened, unlike the `z` term.
fn position_seed(x: i32, y: i32, z: i32) -> i64 {
    let seed = i64::from(x.wrapping_mul(3_129_871))
        ^ i64::from(z).wrapping_mul(116_129_781)
        ^ i64::from(y);
    seed.wrapping_mul(seed)
        .wrapping_mul(42_317_861)
        .wrapping_add(seed.wrapping_mul(11))
        >> 16
}

/// A block's collision shape in vanilla's voxel representation: a grid whose
/// lines on each axis are given by a sorted coordinate list, plus the set of
/// full cells in that grid.
///
/// Vanilla's collision code scans this grid cell by cell, so lines inside a
/// solid region (for example the 1/8 steps of a snow layer) can stop movement.
/// Reproducing vanilla collisions exactly needs this representation rather
/// than the boxes from [`BlockState::collision_shapes`].
#[derive(Debug, PartialEq)]
pub struct VoxelShape {
    x: &'static [f64],
    y: &'static [f64],
    z: &'static [f64],
    full: &'static [u8],
}

/// An axis of a [`VoxelShape`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl VoxelShape {
    /// The grid coordinates along `axis`, ascending, in block-local space.
    /// There is one more coordinate than there are cells on that axis.
    pub const fn coords(&self, axis: Axis) -> &'static [f64] {
        match axis {
            Axis::X => self.x,
            Axis::Y => self.y,
            Axis::Z => self.z,
        }
    }

    /// The number of cells along `axis`.
    pub const fn size(&self, axis: Axis) -> usize {
        self.coords(axis).len() - 1
    }

    /// Whether the cell at the given grid indices is full. Indices outside the
    /// grid are never full, like vanilla's `DiscreteVoxelShape#isFullWide`.
    pub fn is_full(&self, x: i32, y: i32, z: i32) -> bool {
        let (Ok(x), Ok(y), Ok(z)) = (usize::try_from(x), usize::try_from(y), usize::try_from(z))
        else {
            return false;
        };
        let (size_x, size_y, size_z) = (self.size(Axis::X), self.size(Axis::Y), self.size(Axis::Z));
        if x >= size_x || y >= size_y || z >= size_z {
            return false;
        }

        let index = (x * size_y + y) * size_z + z;
        self.full[index / 8] & (1 << (index % 8)) != 0
    }

    /// Whether no cell is full.
    pub fn is_empty(&self) -> bool {
        self.full.iter().all(|&byte| byte == 0)
    }
}

impl fmt::Debug for BlockState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt_block_state(*self, f)
    }
}

impl Display for BlockState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt_block_state(*self, f)
    }
}

fn fmt_block_state(bs: BlockState, f: &mut fmt::Formatter) -> fmt::Result {
    let kind = bs.to_kind();

    write!(f, "{}", kind.to_str())?;

    let props = kind.props();

    if !props.is_empty() {
        let mut list = f.debug_list();
        for &p in kind.props() {
            struct KeyVal<'a>(&'a str, &'a str);

            impl<'a> fmt::Debug for KeyVal<'a> {
                fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    write!(f, "{}={}", self.0, self.1)
                }
            }

            list.entry(&KeyVal(p.to_str(), bs.get(p).unwrap().to_str()));
        }
        list.finish()
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_set_consistency() {
        for kind in BlockKind::ALL {
            let block = kind.to_state();

            for &prop in kind.props() {
                let new_block = block.set(prop, block.get(prop).unwrap());
                assert_eq!(new_block, block);
            }
        }
    }

    #[test]
    fn blockstate_to_wall() {
        assert_eq!(BlockState::STONE.wall_block_id(), None);
        assert_eq!(
            BlockState::OAK_SIGN.wall_block_id(),
            Some(BlockState::OAK_WALL_SIGN)
        );
        assert_eq!(
            BlockState::GREEN_BANNER.wall_block_id(),
            Some(BlockState::GREEN_WALL_BANNER)
        );
        assert_ne!(
            BlockState::GREEN_BANNER.wall_block_id(),
            Some(BlockState::GREEN_BANNER)
        );
    }

    /// Lists every full cell as `(x, y, z)` grid indices.
    fn full_cells(shape: &VoxelShape) -> Vec<(i32, i32, i32)> {
        let size = |axis| i32::try_from(shape.size(axis)).unwrap();
        let mut cells = vec![];
        for x in 0..size(Axis::X) {
            for y in 0..size(Axis::Y) {
                for z in 0..size(Axis::Z) {
                    if shape.is_full(x, y, z) {
                        cells.push((x, y, z));
                    }
                }
            }
        }
        cells
    }

    #[test]
    fn voxel_shape_of_air_is_empty() {
        let shape = BlockState::AIR.collision_voxel_shape();
        assert!(shape.is_empty());
        assert_eq!(shape.coords(Axis::Y), &[0.0]);
        assert!(!shape.is_full(0, 0, 0));
    }

    #[test]
    fn voxel_shape_of_full_block() {
        let shape = BlockState::STONE.collision_voxel_shape();
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            assert_eq!(shape.coords(axis), &[0.0, 1.0]);
        }
        assert_eq!(full_cells(shape), [(0, 0, 0)]);
        assert!(!shape.is_full(-1, 0, 0));
        assert!(!shape.is_full(1, 0, 0));
    }

    #[test]
    fn voxel_shape_of_bottom_slab() {
        let slab = BlockState::SMOOTH_STONE_SLAB.set(PropName::Type, PropValue::Bottom);
        let shape = slab.collision_voxel_shape();
        assert_eq!(shape.coords(Axis::X), &[0.0, 1.0]);
        assert_eq!(shape.coords(Axis::Y), &[0.0, 0.5, 1.0]);
        assert_eq!(shape.coords(Axis::Z), &[0.0, 1.0]);
        assert_eq!(full_cells(shape), [(0, 0, 0)]);
    }

    #[test]
    fn voxel_shape_of_stairs() {
        let stairs = BlockState::OAK_STAIRS
            .set(PropName::Facing, PropValue::North)
            .set(PropName::Half, PropValue::Bottom)
            .set(PropName::Shape, PropValue::Straight);
        let shape = stairs.collision_voxel_shape();
        assert_eq!(shape.coords(Axis::X), &[0.0, 1.0]);
        assert_eq!(shape.coords(Axis::Y), &[0.0, 0.5, 1.0]);
        assert_eq!(shape.coords(Axis::Z), &[0.0, 0.5, 1.0]);
        // The full bottom half, then the upper step on the north (-z) side.
        assert_eq!(full_cells(shape), [(0, 0, 0), (0, 0, 1), (0, 1, 0)]);
    }

    #[test]
    fn voxel_shape_of_snow_keeps_eighth_grid_lines() {
        // Two snow layers collide as one eighth of a block, on an 8-cell grid.
        let snow = BlockState::SNOW.set(PropName::Layers, PropValue::_2);
        let shape = snow.collision_voxel_shape();
        assert_eq!(
            shape.coords(Axis::Y),
            &[0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0]
        );
        assert_eq!(full_cells(shape), [(0, 0, 0)]);

        let snow = BlockState::SNOW.set(PropName::Layers, PropValue::_4);
        assert_eq!(
            full_cells(snow.collision_voxel_shape()),
            [(0, 0, 0), (0, 1, 0), (0, 2, 0)]
        );
    }

    /// Expected values are the bits of the Java `float` literals, so a
    /// rounding mistake between the extractor and Rust can't hide behind a
    /// comparison against a Rust literal parsed the same wrong way.
    #[test]
    fn physics_properties_match_java_float_bits() {
        let cases = [
            (BlockKind::Stone.friction(), 0x3f19999a),
            (BlockKind::Stone.speed_factor(), 0x3f800000),
            (BlockKind::Stone.jump_factor(), 0x3f800000),
            (BlockKind::Stone.bounce_restitution(), 0x00000000),
            (BlockKind::Ice.friction(), 0x3f7ae148),
            (BlockKind::PackedIce.friction(), 0x3f7ae148),
            (BlockKind::BlueIce.friction(), 0x3f7d2f1b),
            (BlockKind::SlimeBlock.friction(), 0x3f4ccccd),
            (BlockKind::SlimeBlock.bounce_restitution(), 0x3f800000),
            (BlockKind::RedBed.bounce_restitution(), 0x3f400000),
            (BlockKind::SoulSand.speed_factor(), 0x3ecccccd),
            (BlockKind::HoneyBlock.speed_factor(), 0x3ecccccd),
            (BlockKind::HoneyBlock.jump_factor(), 0x3f000000),
        ];

        for (index, (value, bits)) in cases.into_iter().enumerate() {
            assert_eq!(value.to_bits(), bits, "case {index}");
        }
    }

    #[test]
    fn offset_definitions() {
        assert_eq!(BlockKind::Stone.offset(), None);
        assert_eq!(
            BlockKind::Bamboo.offset(),
            Some(BlockOffset::Xz {
                max_horizontal: 0.25
            })
        );
        assert_eq!(
            BlockKind::PointedDripstone.offset(),
            Some(BlockOffset::Xz {
                max_horizontal: 0.125
            })
        );
        assert_eq!(
            BlockKind::SmallDripleaf.offset(),
            Some(BlockOffset::Xyz {
                max_horizontal: 0.25,
                max_vertical: 0.1
            })
        );
    }

    #[test]
    fn bamboo_offsets() {
        // (x, z, offset x, offset z), computed by vanilla 26.2.
        let cases = [
            (0, 0, -0.25, -0.25),
            (1, 2, -0.11666665971279144, -0.18333332985639572),
            (-7, 13, -0.04999999701976776, -0.11666665971279144),
            (100000, -3, -0.11666665971279144, 0.0833333432674408),
        ];

        for (x, z, offset_x, offset_z) in cases {
            let offset = BlockState::BAMBOO.offset_at(x, z);
            assert_eq!(offset.x.to_bits(), f64::to_bits(offset_x), "({x}, {z})");
            assert_eq!(offset.y.to_bits(), 0.0_f64.to_bits(), "({x}, {z})");
            assert_eq!(offset.z.to_bits(), f64::to_bits(offset_z), "({x}, {z})");
        }
        assert_eq!(
            BlockState::STONE.offset_at(1, 2),
            chunkedge_math::DVec3::ZERO
        );
    }

    /// Checks every offset block against the offsets vanilla computed for it
    /// while extracting.
    #[test]
    fn offsets_match_vanilla_samples() {
        #[derive(serde::Deserialize)]
        struct TopLevel<'a> {
            #[serde(borrow)]
            blocks: Vec<Block<'a>>,
        }

        #[derive(serde::Deserialize)]
        struct Block<'a> {
            name: &'a str,
            #[serde(borrow)]
            offset: Option<Offset<'a>>,
        }

        #[derive(serde::Deserialize)]
        struct Offset<'a> {
            #[serde(borrow)]
            samples: Vec<Sample<'a>>,
        }

        #[derive(serde::Deserialize)]
        struct Sample<'a> {
            pos: [i32; 3],
            #[serde(borrow)]
            offset: [&'a serde_json::value::RawValue; 3],
        }

        let TopLevel { blocks } =
            serde_json::from_str(include_str!("../extracted/block_physics.json")).unwrap();

        let mut checked = 0;
        for block in blocks {
            let kind = BlockKind::from_str(block.name).unwrap();
            let Some(offset) = block.offset else {
                assert_eq!(kind.offset(), None, "{}", block.name);
                continue;
            };

            for sample in offset.samples {
                let [x, _, z] = sample.pos;
                let actual = kind.offset().unwrap().at(x, z);
                // Parse the decimal text directly: `Double.toString` round-trips.
                let expected = sample.offset.map(|raw| raw.get().parse::<f64>().unwrap());
                assert_eq!(
                    actual.to_array().map(f64::to_bits),
                    expected.map(f64::to_bits),
                    "{} at {:?}",
                    block.name,
                    sample.pos
                );
                checked += 1;
            }
        }
        assert!(checked > 0);
    }

    #[test]
    fn block_tag_membership() {
        for kind in [BlockKind::Ladder, BlockKind::Vine, BlockKind::Scaffolding] {
            assert!(kind.is_in(BlockTag::Climbable), "{kind:?}");
        }
        assert!(BlockKind::OakFence.is_in(BlockTag::Fences));
        assert!(BlockKind::NetherBrickFence.is_in(BlockTag::Fences));
        assert!(BlockKind::CobblestoneWall.is_in(BlockTag::Walls));
        assert!(BlockState::OAK_FENCE_GATE.is_in(BlockTag::FenceGates));

        for tag in [
            BlockTag::Climbable,
            BlockTag::Fences,
            BlockTag::Walls,
            BlockTag::FenceGates,
        ] {
            assert!(!BlockKind::Stone.is_in(tag), "{tag:?}");
        }
    }

    #[test]
    fn block_tag_bitset_matches_members() {
        for tag in BlockTag::ALL {
            for kind in BlockKind::ALL {
                assert_eq!(
                    tag.contains(kind),
                    tag.blocks().contains(&kind),
                    "{tag:?} {kind:?}"
                );
            }
        }
    }

    #[test]
    fn block_tag_str_round_trip() {
        for tag in BlockTag::ALL {
            assert_eq!(BlockTag::from_str(tag.to_str()), Some(tag));
        }
        assert_eq!(
            BlockTag::from_str("mineable/pickaxe"),
            Some(BlockTag::MineablePickaxe)
        );
        assert_eq!(BlockTag::from_str("fences/wooden"), None);
    }
}
