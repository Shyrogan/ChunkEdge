use std::collections::BTreeMap;

use anyhow::Context;
use chunkedge_build_utils::{ident, rerun_if_changed};
use heck::ToPascalCase;
use proc_macro2::TokenStream;
use quote::quote;

/// Tags as extracted: registry name → tag name → raw entry IDs. Tag includes
/// are already resolved by the extractor, so every list is flat.
type Registries = BTreeMap<String, BTreeMap<String, Vec<u16>>>;

pub(crate) fn build() -> anyhow::Result<TokenStream> {
    rerun_if_changed(["extracted/tags.json"]);

    let registries: Registries = serde_json::from_str(include_str!("../extracted/tags.json"))?;

    // Only the `minecraft` namespace is vanilla. The `c` convention tags come
    // from Fabric API, which is loaded in the extractor's environment.
    let tags = registries
        .get("minecraft:block")
        .context("tags.json has no minecraft:block registry")?
        .iter()
        .filter_map(|(name, ids)| Some((name.strip_prefix("minecraft:")?, ids)))
        .map(|(path, ids)| {
            let mut ids = ids.clone();
            ids.sort_unstable();
            ids.dedup();
            (path, ident(path.replace('/', "_").to_pascal_case()), ids)
        })
        .collect::<Vec<_>>();

    let variants = tags
        .iter()
        .map(|(_, variant, _)| variant)
        .collect::<Vec<_>>();
    let tag_count = tags.len();

    let from_str_arms = tags
        .iter()
        .map(|(path, variant, _)| quote!(#path => Some(Self::#variant),))
        .collect::<TokenStream>();

    let to_str_arms = tags
        .iter()
        .map(|(path, variant, _)| quote!(Self::#variant => #path,))
        .collect::<TokenStream>();

    let members = tags
        .iter()
        .map(|(_, _, ids)| quote!(&[#(block_kind(#ids),)*]))
        .collect::<Vec<_>>();

    Ok(quote! {
        /// A vanilla block tag, such as `minecraft:climbable`.
        ///
        /// Membership reflects the tags Minecraft ships with. Datapacks can
        /// redefine tags at runtime, and the tags actually sent to clients live
        /// in `chunkedge_registry`'s `TagsRegistry` resource; this lookup does
        /// not observe changes made there.
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub enum BlockTag {
            #(#variants,)*
        }

        impl BlockTag {
            /// Construct a block tag from its path without the `minecraft:`
            /// namespace, e.g. `climbable` or `mineable/pickaxe`.
            ///
            /// Returns `None` if the name is not a vanilla block tag.
            pub fn from_str(name: &str) -> Option<Self> {
                match name {
                    #from_str_arms
                    _ => None,
                }
            }

            /// Get the path of this tag without the `minecraft:` namespace.
            pub const fn to_str(self) -> &'static str {
                match self {
                    #to_str_arms
                }
            }

            /// The block kinds in this tag, sorted by raw ID.
            pub const fn blocks(self) -> &'static [BlockKind] {
                BLOCK_TAG_MEMBERS[self as usize]
            }

            /// Returns whether `kind` is in this tag.
            pub const fn contains(self, kind: BlockKind) -> bool {
                let id = kind.to_raw() as usize;
                BLOCK_TAG_BITSETS[self as usize][id / 64] >> (id % 64) & 1 == 1
            }

            /// An array of all vanilla block tags.
            pub const ALL: [Self; #tag_count] = [#(Self::#variants,)*];
        }

        impl BlockKind {
            /// Returns whether this block kind is in the vanilla block tag `tag`.
            pub const fn is_in(self, tag: BlockTag) -> bool {
                tag.contains(self)
            }
        }

        impl BlockState {
            /// Returns whether this state's block kind is in the vanilla block
            /// tag `tag`.
            pub const fn is_in(self, tag: BlockTag) -> bool {
                self.to_kind().is_in(tag)
            }
        }

        const BLOCK_TAG_MEMBERS: [&[BlockKind]; #tag_count] = [#(#members,)*];

        const BLOCK_TAG_BITSETS: [[u64; BlockKind::ALL.len().div_ceil(64)]; #tag_count] = {
            let mut bitsets = [[0; BlockKind::ALL.len().div_ceil(64)]; #tag_count];
            let mut tag = 0;
            while tag < BLOCK_TAG_MEMBERS.len() {
                let members = BLOCK_TAG_MEMBERS[tag];
                let mut i = 0;
                while i < members.len() {
                    let id = members[i].to_raw() as usize;
                    bitsets[tag][id / 64] |= 1 << (id % 64);
                    i += 1;
                }
                tag += 1;
            }
            bitsets
        };

        /// Resolves an extracted raw ID at compile time. An ID that matches no
        /// block kind fails the build instead of silently dropping out of a tag.
        const fn block_kind(id: u16) -> BlockKind {
            match BlockKind::from_raw(id) {
                Some(kind) => kind,
                None => panic!("tags.json references an unknown block ID"),
            }
        }
    })
}
