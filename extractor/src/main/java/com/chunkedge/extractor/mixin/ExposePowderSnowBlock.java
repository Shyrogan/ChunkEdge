package com.chunkedge.extractor.mixin;

import net.minecraft.world.level.block.PowderSnowBlock;
import net.minecraft.world.phys.shapes.VoxelShape;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.gen.Accessor;

@Mixin(PowderSnowBlock.class)
public interface ExposePowderSnowBlock {
    @Accessor("FALLING_COLLISION_SHAPE")
    static VoxelShape getFallingCollisionShape() {
        throw new AssertionError();
    }
}
