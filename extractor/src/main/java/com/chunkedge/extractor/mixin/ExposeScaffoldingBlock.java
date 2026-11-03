package com.chunkedge.extractor.mixin;

import net.minecraft.world.level.block.ScaffoldingBlock;
import net.minecraft.world.phys.shapes.VoxelShape;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.gen.Accessor;

@Mixin(ScaffoldingBlock.class)
public interface ExposeScaffoldingBlock {
    @Accessor("SHAPE_STABLE")
    static VoxelShape getStableShape() {
        throw new AssertionError();
    }

    @Accessor("SHAPE_UNSTABLE_BOTTOM")
    static VoxelShape getUnstableBottomShape() {
        throw new AssertionError();
    }
}
