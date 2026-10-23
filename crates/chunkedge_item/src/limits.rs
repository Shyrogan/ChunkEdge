//! Limits applied while decoding items from untrusted input.

use std::mem;

use chunkedge_binary::{Decode, VarInt};

/// Vanilla `ItemContainerContents` list limit.
pub(crate) const MAX_CONTAINER_ITEMS: usize = 256;
/// Vanilla `ChargedProjectiles` list limit.
pub(crate) const MAX_CHARGED_PROJECTILES: usize = 1024;

/// Memory one top-level item decode may spend on item stacks, boxed
/// components and block predicates.
///
/// A nested item stack costs a few bytes on the wire but about 2 KiB in memory,
/// so without a budget a single 2 MiB packet of nested containers decodes into
/// gigabytes. 16 MiB leaves room for a shulker box full of bundles.
const MAX_DECODE_BYTES: usize = 16 * 1024 * 1024;

/// Remaining memory budget for one top-level item decode.
pub(crate) struct DecodeBudget(usize);

impl DecodeBudget {
    pub(crate) fn new() -> Self {
        Self(MAX_DECODE_BYTES)
    }

    /// Charges the memory of one `T` against the budget.
    pub(crate) fn charge<T>(&mut self) -> anyhow::Result<()> {
        self.0 = self
            .0
            .checked_sub(mem::size_of::<T>())
            .ok_or_else(|| anyhow::anyhow!("item exceeds decode memory budget"))?;
        Ok(())
    }
}

/// Reads a `VarInt` collection length, rejecting negative lengths and lengths
/// above `max` like vanilla `ByteBufCodecs.readCount`.
pub(crate) fn read_count(r: &mut &[u8], max: usize) -> anyhow::Result<usize> {
    let count = VarInt::decode(r)?.0;
    anyhow::ensure!(count >= 0, "negative collection length of {count}");
    let count = count as usize;
    anyhow::ensure!(count <= max, "{count} elements exceeded max size of {max}");
    Ok(count)
}

#[cfg(test)]
mod tests {
    use chunkedge_binary::Encode;
    use chunkedge_generated::item::ItemKind;
    use chunkedge_generated::item_component_type as component_id;

    use super::*;
    use crate::{HashedItemStack, ItemComponent, ItemStack};

    /// Writes the header of a one-item stack with a single added component.
    fn write_stack_with_component(w: &mut Vec<u8>, item: ItemKind, component: usize) {
        VarInt(1).encode(&mut *w).unwrap(); // Count
        item.encode(&mut *w).unwrap();
        VarInt(1).encode(&mut *w).unwrap(); // Added components
        VarInt(0).encode(&mut *w).unwrap(); // Removed components
        VarInt(component as i32).encode(&mut *w).unwrap();
    }

    fn decode_err(buf: &[u8]) -> String {
        ItemStack::decode(&mut &buf[..]).unwrap_err().to_string()
    }

    #[test]
    fn deep_block_predicate_nesting_is_rejected() {
        // CanPlaceOn -> predicate -> exact CanPlaceOn -> predicate -> ... never
        // goes through an item stack, so it used to skip the depth check and
        // overflow the stack.
        let mut buf = vec![];
        write_stack_with_component(&mut buf, ItemKind::Stone, component_id::CAN_PLACE_ON);
        for _ in 0..100_000 {
            VarInt(1).encode(&mut buf).unwrap(); // Predicates
            buf.extend([0, 0, 0]); // No blocks, properties or NBT
            VarInt(1).encode(&mut buf).unwrap(); // Exact components
            VarInt(component_id::CAN_PLACE_ON as i32)
                .encode(&mut buf)
                .unwrap();
        }

        assert!(decode_err(&buf).contains("recursion limit exceeded"));
    }

    #[test]
    fn negative_list_length_is_rejected() {
        let mut buf = vec![];
        write_stack_with_component(&mut buf, ItemKind::Bundle, component_id::BUNDLE_CONTENTS);
        VarInt(-1).encode(&mut buf).unwrap();

        assert!(decode_err(&buf).contains("negative collection length"));
    }

    #[test]
    fn many_cheap_nested_stacks_exhaust_the_budget() {
        // Each empty stack is one byte on the wire but a whole `ItemStack` in
        // memory.
        let count = 100_000;
        let mut buf = vec![];
        write_stack_with_component(&mut buf, ItemKind::Bundle, component_id::BUNDLE_CONTENTS);
        VarInt(count as i32).encode(&mut buf).unwrap();
        buf.extend(std::iter::repeat_n(0, count));

        assert!(decode_err(&buf).contains("decode memory budget"));
    }

    #[test]
    fn shulker_box_full_of_bundles_fits_the_budget() {
        let bundle_items = vec![ItemStack::new(ItemKind::Stone, 1); 64];
        let mut bundle = ItemStack::new(ItemKind::Bundle, 1);
        bundle.insert_component(ItemComponent::BundleContents(bundle_items));

        let mut shulker = ItemStack::new(ItemKind::ShulkerBox, 1);
        shulker.insert_component(ItemComponent::Container(vec![bundle; 27]));

        let mut buf = vec![];
        shulker.encode(&mut buf).unwrap();
        assert_eq!(ItemStack::decode(&mut buf.as_slice()).unwrap(), shulker);
    }

    #[test]
    fn out_of_range_count_is_rejected() {
        for count in [128, 300, i32::MAX] {
            let mut buf = vec![];
            VarInt(count).encode(&mut buf).unwrap();
            ItemKind::Stone.encode(&mut buf).unwrap();
            VarInt(0).encode(&mut buf).unwrap();
            VarInt(0).encode(&mut buf).unwrap();
            assert!(decode_err(&buf).contains("out of range"), "count {count}");

            let mut buf = vec![];
            true.encode(&mut buf).unwrap();
            ItemKind::Stone.encode(&mut buf).unwrap();
            VarInt(count).encode(&mut buf).unwrap();
            VarInt(0).encode(&mut buf).unwrap();
            VarInt(0).encode(&mut buf).unwrap();
            assert!(HashedItemStack::decode(&mut buf.as_slice()).is_err());
        }

        let mut buf = vec![];
        ItemStack::new(ItemKind::Stone, 127)
            .encode(&mut buf)
            .unwrap();
        assert_eq!(ItemStack::decode(&mut buf.as_slice()).unwrap().count, 127);
    }
}
