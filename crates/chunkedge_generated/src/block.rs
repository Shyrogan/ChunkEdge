#![allow(clippy::all)] // TODO: block build script creates many warnings.

use std::fmt;
use std::fmt::Display;
use std::iter::FusedIterator;

use chunkedge_ident::{Ident, ident};

use crate::item::ItemKind;

include!(concat!(env!("OUT_DIR"), "/block.rs"));
include!(concat!(env!("OUT_DIR"), "/block_tag.rs"));

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
