use super::*;

// TODO: should `set_if_neq` behavior be the default behavior for setters?
macro_rules! flags {
    (
        $(
            $component:path {
                $($flag:ident: $offset:literal),* $(,)?
            }
        )*

    ) => {
        $(
            impl $component {
                $(
                    #[doc = "Gets the bit at offset "]
                    #[doc = stringify!($offset)]
                    #[doc = "."]
                    #[inline]
                    pub const fn $flag(&self) -> bool {
                        (self.0 >> $offset) & 1 == 1
                    }

                    paste! {
                        #[doc = "Sets the bit at offset "]
                        #[doc = stringify!($offset)]
                        #[doc = "."]
                        #[inline]
                        pub fn [< set_$flag >] (&mut self, $flag: bool) {
                            self.0 = (self.0 & !(1 << $offset)) | ((i8::from($flag)) << $offset);
                        }
                    }
                )*
            }
        )*
    }
}

flags! {
    entity::DataSharedFlagsId {
        on_fire: 0,
        sneaking: 1,
        sprinting: 3,
        swimming: 4,
        invisible: 5,
        glowing: 6,
        fall_flying: 7,
    }
    abstract_arrow::IdFlags {
        critical: 0,
        no_clip: 1,
    }
    living::DataLivingEntityFlags {
        using_item: 0,
        off_hand_active: 1,
        using_riptide: 2,
    }
    avatar::DataPlayerModeCustomisation {
        cape: 0,
        jacket: 1,
        left_sleeve: 2,
        right_sleeve: 3,
        left_pants_leg: 4,
        right_pants_leg: 5,
        hat: 6,
    }
    armor_stand::DataClientFlags {
        small: 0,
        show_arms: 2,
        hide_base_plate: 3,
        marker: 4,
    }
    mob::DataMobFlagsId {
        ai_disabled: 0,
        left_handed: 1,
        attacking: 2,
    }
    bat::DataIdFlags {
        hanging: 0,
    }
    abstract_horse::DataIdFlags {
        tamed: 1,
        saddled: 2,
        bred: 3,
        eating_grass: 4,
        angry: 5,
        eating: 6,
    }
    fox::DataFlagsId {
        sitting: 0,
        crouching: 2,
        rolling_head: 3,
        chasing: 4,
        sleeping: 5,
        walking: 6,
        aggressive: 7,
    }
    panda::DataIdFlags {
        sneezing: 1,
        playing: 2,
        sitting: 3,
        lying_on_back: 4,
    }
    tamable_animal::DataFlagsId {
        sitting_pose: 0,
        tamed: 2,
    }
    iron_golem::DataFlagsId {
        player_created: 0,
    }
    snow_golem::DataPumpkinId {
        has_pumpkin: 4,
    }
    blaze::DataFlagsId {
        fire_active: 0,
    }
    vex::DataFlagsId {
        charging: 0,
    }
    spider::DataFlagsId {
        climbing_wall: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_set_flags() {
        let mut flags = entity::DataSharedFlagsId(0);

        flags.set_on_fire(true);
        let before = flags.clone();
        assert_ne!(flags.0, 0);
        flags.set_on_fire(true);
        assert_eq!(before, flags);
        flags.set_on_fire(false);
        assert_eq!(flags.0, 0);
    }
}
