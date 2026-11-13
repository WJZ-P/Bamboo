import it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap;
import it.unimi.dsi.fastutil.longs.Long2ObjectMap;
import it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collection;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.ForkJoinPool;
import java.util.function.Consumer;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerRegistries;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.dedicated.DedicatedServer;
import net.minecraft.server.level.ChunkMap;
import net.minecraft.server.level.ServerChunkCache;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.stats.Stat;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.AbortableIterationConsumer;
import net.minecraft.util.RandomSource;
import net.minecraft.world.Difficulty;
import net.minecraft.world.attribute.EnvironmentAttributeSystem;
import net.minecraft.world.damagesource.DamageSource;
import net.minecraft.world.damagesource.DamageSources;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.boss.enderdragon.EnderDragonPart;
import net.minecraft.world.entity.item.FallingBlockEntity;
import net.minecraft.world.entity.item.ItemEntity;
import net.minecraft.world.entity.item.PrimedTnt;
import net.minecraft.world.entity.player.Abilities;
import net.minecraft.world.entity.player.Inventory;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.entity.player.PlayerEquipment;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.shapes.VoxelShape;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.entity.HopperBlockEntity;
import net.minecraft.world.level.block.entity.ShulkerBoxBlockEntity;
import net.minecraft.world.level.block.ShulkerBoxBlock;
import net.minecraft.world.level.block.piston.MovingPistonBlock;
import net.minecraft.world.level.block.piston.PistonMovingBlockEntity;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.BlockGetter;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.SpeleothemBlock;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.SpeleothemThickness;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.entity.EntityTypeTest;
import net.minecraft.world.level.entity.LevelEntityGetter;
import net.minecraft.world.level.gameevent.GameEvent;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.util.debug.ServerDebugSubscribers;
import net.minecraft.world.level.StructureManager;
import net.minecraft.world.level.border.WorldBorder;
import net.minecraft.world.level.material.Fluid;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.redstone.CollectingNeighborUpdater;
import net.minecraft.world.level.storage.LevelData;
import net.minecraft.world.level.storage.loot.LootTable;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.ticks.LevelTicks;
import sun.misc.Unsafe;

/**
 * 官方下落方块和点燃 TNT 的物理。每个场景在假世界里跑，每 tick 打一行 JSON。
 *
 * Official falling-block and primed-TNT physics. Each scene runs in a fake world and prints one JSON line per tick.
 */
public final class EntityOracle {
    private static final int MIN_Y = -64;
    private static final int HEIGHT = 384;
    private static final Unsafe UNSAFE = loadUnsafe();

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(log);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(PrintStream log) throws Exception {
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        System.setOut(log);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                        new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
                Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8))) {
            Official official = Official.load(resources);
            out.write("{\"type\":\"header\",\"version\":\"");
            out.write(SharedConstants.getCurrentVersion().id());
            out.write("\",\"protocol\":");
            out.write(Integer.toString(SharedConstants.getCurrentVersion().protocolVersion()));
            out.write("}\n");
            int cases = 0;
            cases += sandOnStone(official, out);
            cases += sandOnTorch(official, out);
            cases += concreteIntoWater(official, out);
            cases += dripstoneTip(official, out);
            cases += anvilCrack(official, out);
            cases += fallingDrops(official, out);
            cases += tntInAir(official, out);
            cases += tntBreaksStone(official, out);
            cases += tntChain(official, out);
            cases += itemFall(official, out);
            cases += itemWater(official, out);
            cases += itemLava(official, out);
            cases += itemMerge(official, out);
            cases += itemHopper(official, out);
            cases += itemAboveHopper(official, out);
            cases += itemPickup(official, out);
            cases += itemPickupPartial(official, out);
            cases += itemPickupCreativeFull(official, out);
            cases += itemPickupSpectator(official, out);
            cases += itemPickupDelay(official, out);
            cases += itemPickupInfinite(official, out);
            cases += movingPistonCollision(official, out);
            cases += dynamicShapeCollision(official, out);
            out.write("{\"type\":\"end\",\"cases\":" + cases + "}\n");
            out.flush();
        }
    }

    private static int sandOnStone(Official official, Writer out) throws Exception {
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 64, 0), Blocks.STONE.defaultBlockState());
        blocks.put(BlockPos.asLong(0, 70, 0), Blocks.SAND.defaultBlockState());
        play(official, out, "sand_on_stone", "falling", Blocks.SAND.defaultBlockState(), 0, 70, 0, blocks, 1L, 1L, null, null, 40);
        return 1;
    }

    private static int sandOnTorch(Official official, Writer out) throws Exception {
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 64, 0), Blocks.STONE.defaultBlockState());
        blocks.put(BlockPos.asLong(0, 65, 0), Blocks.TORCH.defaultBlockState());
        blocks.put(BlockPos.asLong(0, 69, 0), Blocks.SAND.defaultBlockState());
        play(official, out, "sand_on_torch", "falling", Blocks.SAND.defaultBlockState(), 0, 69, 0, blocks, 1L, 1L, null, null, 40);
        return 1;
    }

    private static int concreteIntoWater(Official official, Writer out) throws Exception {
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 64, 0), Blocks.WATER.defaultBlockState());
        blocks.put(BlockPos.asLong(0, 68, 0), Blocks.CONCRETE_POWDER.white().defaultBlockState());
        play(
                official,
                out,
                "concrete_into_water",
                "falling",
                Blocks.CONCRETE_POWDER.white().defaultBlockState(),
                0,
                68,
                0,
                blocks,
                1L,
                1L,
                null,
                null,
                40);
        return 1;
    }

    private static int dripstoneTip(Official official, Writer out) throws Exception {
        BlockState tip = Blocks.POINTED_DRIPSTONE
                .defaultBlockState()
                .setValue(SpeleothemBlock.TIP_DIRECTION, Direction.DOWN)
                .setValue(SpeleothemBlock.THICKNESS, SpeleothemThickness.TIP);
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 64, 0), Blocks.STONE.defaultBlockState());
        blocks.put(BlockPos.asLong(0, 70, 0), tip);
        play(official, out, "dripstone_tip", "falling", tip, 0, 70, 0, blocks, 1L, 1L, null, null, 40);
        return 1;
    }

    private static int anvilCrack(Official official, Writer out) throws Exception {
        long seed = crackingSeed(official);
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 64, 0), Blocks.STONE.defaultBlockState());
        blocks.put(BlockPos.asLong(0, 80, 0), Blocks.ANVIL.defaultBlockState());
        play(
                official,
                out,
                "anvil_crack",
                "falling",
                Blocks.ANVIL.defaultBlockState(),
                0,
                80,
                0,
                blocks,
                1L,
                seed,
                new float[] {2.0F, 40.0F},
                null,
                80);
        return 1;
    }

    /** 找一颗会把铁砧摔裂的实体随机种子。找不到就用 1，两边仍然要一致。 */
    private static long crackingSeed(Official official) {
        for (long seed = 1; seed <= 64; seed++) {
            Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
            blocks.put(BlockPos.asLong(0, 64, 0), Blocks.STONE.defaultBlockState());
            blocks.put(BlockPos.asLong(0, 80, 0), Blocks.ANVIL.defaultBlockState());
            try {
                Scene scene = Scene.start(
                        official, "falling", Blocks.ANVIL.defaultBlockState(), 0, 80, 0, blocks, 1L, seed, new float[] {2.0F, 40.0F}, null, 1);
                for (int tick = 0; tick < 80 && !scene.active.isEmpty(); tick++) {
                    scene.tick();
                }
                BlockState landed = scene.level.getBlockState(new BlockPos(0, 65, 0));
                if (landed.is(Blocks.CHIPPED_ANVIL)) {
                    return seed;
                }
            } catch (RuntimeException ignored) {
                return 1L;
            }
        }
        return 1L;
    }

    /** 下落方块在放不回去、掉出世界和超过 600 刻时生成的物品，以及 `entity_drops` 关掉之后的样子。 */
    private static int fallingDrops(Official official, Writer out) throws Exception {
        Long2ObjectOpenHashMap<BlockState> torch = new Long2ObjectOpenHashMap<>();
        torch.put(BlockPos.asLong(0, 64, 0), Blocks.STONE.defaultBlockState());
        torch.put(BlockPos.asLong(0, 65, 0), Blocks.TORCH.defaultBlockState());
        torch.put(BlockPos.asLong(0, 69, 0), Blocks.SAND.defaultBlockState());
        torch.put(BlockPos.asLong(0, 70, 0), Blocks.GLASS.defaultBlockState());
        playDrop(official, out, "falling_on_torch", Blocks.SAND.defaultBlockState(), 0, 69, 0, torch, true, 12);
        playDrop(official, out, "falling_on_torch_no_drops", Blocks.SAND.defaultBlockState(), 0, 69, 0, torch, false, 12);

        Long2ObjectOpenHashMap<BlockState> anvil = new Long2ObjectOpenHashMap<>();
        anvil.put(BlockPos.asLong(0, 64, 0), Blocks.STONE.defaultBlockState());
        anvil.put(BlockPos.asLong(0, 65, 0), Blocks.TORCH.defaultBlockState());
        anvil.put(BlockPos.asLong(0, 69, 0), Blocks.ANVIL.defaultBlockState());
        anvil.put(BlockPos.asLong(0, 70, 0), Blocks.GLASS.defaultBlockState());
        playDrop(official, out, "falling_anvil_on_torch", Blocks.ANVIL.defaultBlockState(), 0, 69, 0, anvil, true, 12);

        Long2ObjectOpenHashMap<BlockState> voidFall = new Long2ObjectOpenHashMap<>();
        voidFall.put(BlockPos.asLong(0, -60, 0), Blocks.SAND.defaultBlockState());
        playDrop(official, out, "falling_out_of_world", Blocks.SAND.defaultBlockState(), 0, -60, 0, voidFall, true, 20);
        playDrop(official, out, "falling_out_of_world_no_drops", Blocks.SAND.defaultBlockState(), 0, -60, 0, voidFall, false, 20);

        Long2ObjectOpenHashMap<BlockState> high = new Long2ObjectOpenHashMap<>();
        high.put(BlockPos.asLong(0, 300, 0), Blocks.SAND.defaultBlockState());
        playDrop(official, out, "falling_timeout", Blocks.SAND.defaultBlockState(), 0, 300, 0, high, true, 20);
        return 6;
    }

    private static void playDrop(
            Official official,
            Writer out,
            String name,
            BlockState state,
            int x,
            int y,
            int z,
            Long2ObjectOpenHashMap<BlockState> blocks,
            boolean entityDrops,
            int ticks)
            throws Exception {
        official.gameRules.set(GameRules.ENTITY_DROPS, entityDrops, null);
        try {
            // 下落方块和它掉出来的物品都从同一个 ID 起，Bamboo 那边也用这个起点。
            // The falling block and its item start at the same id on both sides.
            Scene scene = Scene.start(official, "falling", state, x, y, z, blocks, 1L, 1L, null, null, FIRST_ITEM_ID);
            scene.fallingDrops = true;
            // 超时题直接把 `time` 挪到边界前，省掉几百行一样的下落快照。
            if (name.startsWith("falling_out_of_world") || name.equals("falling_timeout")) {
                // 从 590 开始，几拍之后就能看到超时那一支；两边都写进题目。
                // Starting at 590 exposes the timeout branch after a few ticks on both sides.
                ((FallingBlockEntity) scene.active.get(0)).time = 590;
            }
            StringBuilder steps = new StringBuilder();
            steps.append('[');
            scene.writeStep(steps);
            for (int tick = 0; tick < ticks; tick++) {
                steps.append(',');
                scene.tick();
                scene.writeStep(steps);
            }
            steps.append(']');
            out.write("{\"type\":\"case\",\"name\":\"");
            out.write(name);
            out.write("\",\"kind\":\"falling\",\"state\":");
            out.write(Integer.toString(Block.getId(state)));
            out.write(",\"x\":");
            out.write(Integer.toString(x));
            out.write(",\"y\":");
            out.write(Integer.toString(y));
            out.write(",\"z\":");
            out.write(Integer.toString(z));
            out.write(",\"levelSeed\":1,\"entitySeed\":1,\"entityDrops\":");
            out.write(Boolean.toString(entityDrops));
            out.write(",\"firstEntityId\":");
            out.write(Integer.toString(1 << 24));
            out.write(",\"startTime\":");
            out.write(name.startsWith("falling_out_of_world") || name.equals("falling_timeout") ? "590" : "0");
            out.write(",\"compareDrops\":true,\"hurts\":null,\"fuse\":null,\"blocks\":");
            writeBlocks(out, blocks);
            out.write(",\"steps\":");
            out.write(steps.toString());
            out.write("}\n");
        } finally {
            official.gameRules.set(GameRules.ENTITY_DROPS, true, null);
        }
    }

    private static int tntInAir(Official official, Writer out) throws Exception {
        play(official, out, "tnt_in_air", "tnt", Blocks.TNT.defaultBlockState(), 0, 80, 0, new Long2ObjectOpenHashMap<>(), 1L, 1L, null, null, 30);
        return 1;
    }

    private static int tntBreaksStone(Official official, Writer out) throws Exception {
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        for (int x = -2; x <= 2; x++) {
            for (int z = -2; z <= 2; z++) {
                blocks.put(BlockPos.asLong(x, 64, z), Blocks.STONE.defaultBlockState());
            }
        }
        play(official, out, "tnt_breaks_stone", "tnt", Blocks.TNT.defaultBlockState(), 0, 65, 0, blocks, 3L, 1L, null, 4, 8);
        return 1;
    }

    private static int tntChain(Official official, Writer out) throws Exception {
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(1, 65, 0), Blocks.TNT.defaultBlockState());
        blocks.put(BlockPos.asLong(-1, 65, 0), Blocks.TNT.defaultBlockState());
        play(official, out, "tnt_chain", "tnt", Blocks.TNT.defaultBlockState(), 0, 65, 0, blocks, 5L, 1L, null, 4, 8);
        return 1;
    }

    private static final int FIRST_ITEM_ID = 1 << 24;
    private static final long ENTITY_RANDOM_SEED = 0x0DDB1A5E5BAD5EEDL;

    /**
     * 移动中的活塞：`moving_piston` 的碰撞形状来自方块实体，不推进动画（不登记 ticker），
     * 逐刻考察实体在这一份形状上的落地、滑落和空形状穿越。
     *
     * Moving pistons: the shape comes from the block entity. The animation is frozen (no ticker),
     * so each tick tests landing, sliding and no-block-entity pass-through against one shape.
     */
    private static int movingPistonCollision(Official official, Writer out) throws Exception {
        BlockState moving = Blocks.MOVING_PISTON.defaultBlockState()
                .setValue(MovingPistonBlock.FACING, Direction.UP);
        // 伸出中：被推的石头按 getExtendedProgress(0.5) 往下挪半格。
        Long2ObjectOpenHashMap<BlockState> extending = new Long2ObjectOpenHashMap<>();
        extending.put(BlockPos.asLong(0, 64, 0), Blocks.STONE.defaultBlockState());
        extending.put(BlockPos.asLong(0, 65, 0), moving);
        playPistonFalling(official, out, "piston_extending_falling", extending,
                Blocks.SAND.defaultBlockState(), 0, 68, 0, new BlockPos(0, 65, 0),
                Blocks.STONE.defaultBlockState(), Direction.UP, true, false, 0.5f, 20);
        playPistonItem(official, out, "piston_extending_item", extending,
                new BlockPos(0, 65, 0), Blocks.STONE.defaultBlockState(), Direction.UP,
                true, false, 0.5f, 20);

        // 缩回中的源活塞：底座的伸出形状 + 活塞头按 getExtendedProgress(0.5) 往上挪半格。
        BlockState base = Blocks.PISTON.defaultBlockState()
                .setValue(net.minecraft.world.level.block.piston.PistonBaseBlock.FACING, Direction.UP);
        Long2ObjectOpenHashMap<BlockState> retracting = new Long2ObjectOpenHashMap<>();
        retracting.put(BlockPos.asLong(0, 64, 0), Blocks.STONE.defaultBlockState());
        retracting.put(BlockPos.asLong(0, 65, 0), moving);
        playPistonFalling(official, out, "piston_retracting_falling", retracting,
                Blocks.SAND.defaultBlockState(), 0, 68, 0, new BlockPos(0, 65, 0),
                base, Direction.UP, false, true, 0.5f, 20);
        playPistonItem(official, out, "piston_retracting_item", retracting,
                new BlockPos(0, 65, 0), base, Direction.UP, false, true, 0.5f, 20);
        return 4;
    }

    /**
     * 开着盖子的潜影盒，以及细雪上的下落方块：形状由方块实体和实体上下文决定。
     *
     * An open shulker box and a falling block in powder snow: the shapes come from
     * a block entity and the entity collision context respectively.
     */
    private static int dynamicShapeCollision(Official official, Writer out) throws Exception {
        BlockPos boxPos = new BlockPos(0, 65, 0);
        BlockState shulker = Blocks.SHULKER_BOX.defaultBlockState()
                .setValue(ShulkerBoxBlock.FACING, Direction.UP);
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 64, 0), Blocks.STONE.defaultBlockState());
        blocks.put(boxPos.asLong(), shulker);
        blocks.put(BlockPos.asLong(0, 68, 0), Blocks.POWDER_SNOW.defaultBlockState());
        Scene scene = Scene.start(official, "falling", Blocks.SAND.defaultBlockState(),
                0, 70, 0, blocks, 1L, 1L, null, null, 1);
        attachShulker(scene, boxPos, shulker, 1.0f);
        StringBuilder steps = new StringBuilder();
        steps.append('[');
        scene.writeStep(steps);
        for (int tick = 0; tick < 20; tick++) {
            steps.append(',');
            scene.tick();
            scene.writeStep(steps);
        }
        steps.append(']');
        out.write("{\"type\":\"case\",\"name\":\"dynamic_shapes_falling\"");
        out.write(",\"kind\":\"falling\",\"state\":");
        out.write(Integer.toString(Block.getId(Blocks.SAND.defaultBlockState())));
        out.write(",\"x\":0,\"y\":70,\"z\":0");
        out.write(",\"levelSeed\":1,\"entitySeed\":1,\"hurts\":null,\"fuse\":null");
        out.write(",\"shulkers\":[{\"x\":0,\"y\":65,\"z\":0,\"progress\":1.0}]");
        out.write(",\"blocks\":");
        writeBlocks(out, blocks);
        out.write(",\"steps\":");
        out.write(steps.toString());
        out.write("}\n");
        return 1;
    }

    private static void attachShulker(
            Scene scene, BlockPos pos, BlockState state, float progress) throws Exception {
        ShulkerBoxBlockEntity shulker = new ShulkerBoxBlockEntity(pos, state);
        set(ShulkerBoxBlockEntity.class, shulker, "progress", progress);
        set(ShulkerBoxBlockEntity.class, shulker, "progressOld", progress);
        shulker.setLevel(scene.level);
        // 只放进 `getBlockEntity` 用的表，不登记 ticker：官方场景不跑方块实体。
        scene.level.blockEntities.put(pos.asLong(), shulker);
    }

    private static void attachPiston(
            Scene scene,
            BlockPos pos,
            BlockState moved,
            Direction facing,
            boolean extending,
            boolean source,
            float progress) {
        PistonMovingBlockEntity piston = (PistonMovingBlockEntity) MovingPistonBlock.newMovingBlockEntity(
                pos, scene.level.getBlockState(pos), moved, facing, extending, source);
        set(PistonMovingBlockEntity.class, piston, "progress", progress);
        set(PistonMovingBlockEntity.class, piston, "progressO", progress);
        piston.setLevel(scene.level);
        // 只放进 `getBlockEntity` 用的表，不登记 ticker：官方 `Scene.tick` 不跑方块实体。
        scene.level.blockEntities.put(pos.asLong(), piston);
    }

    private static void playPistonFalling(
            Official official,
            Writer out,
            String name,
            Long2ObjectOpenHashMap<BlockState> blocks,
            BlockState state,
            int x,
            int y,
            int z,
            BlockPos pistonPos,
            BlockState moved,
            Direction facing,
            boolean extending,
            boolean source,
            float progress,
            int ticks)
            throws Exception {
        Scene scene = Scene.start(official, "falling", state, x, y, z, blocks, 1L, 1L, null, null, 1);
        attachPiston(scene, pistonPos, moved, facing, extending, source, progress);
        StringBuilder steps = new StringBuilder();
        steps.append('[');
        scene.writeStep(steps);
        for (int tick = 0; tick < ticks; tick++) {
            steps.append(',');
            scene.tick();
            scene.writeStep(steps);
        }
        steps.append(']');
        out.write("{\"type\":\"case\",\"name\":\"");
        out.write(name);
        out.write("\",\"kind\":\"falling\",\"state\":");
        out.write(Integer.toString(Block.getId(state)));
        out.write(",\"x\":");
        out.write(Integer.toString(x));
        out.write(",\"y\":");
        out.write(Integer.toString(y));
        out.write(",\"z\":");
        out.write(Integer.toString(z));
        out.write(",\"levelSeed\":1,\"entitySeed\":1,\"hurts\":null,\"fuse\":null");
        out.write(",\"shapes\":");
        writePistonShapes(out, pistonPos, (PistonMovingBlockEntity) scene.level.getBlockEntity(pistonPos));
        out.write(",\"pistons\":");
        writePiston(out, pistonPos, moved, facing, extending, source, progress);
        out.write(",\"blocks\":");
        writeBlocks(out, blocks);
        out.write(",\"steps\":");
        out.write(steps.toString());
        out.write("}\n");
    }

    private static void playPistonItem(
            Official official,
            Writer out,
            String name,
            Long2ObjectOpenHashMap<BlockState> blocks,
            BlockPos pistonPos,
            BlockState moved,
            Direction facing,
            boolean extending,
            boolean source,
            float progress,
            int ticks)
            throws Exception {
        Scene scene = Scene.items(official, blocks, false);
        attachPiston(scene, pistonPos, moved, facing, extending, source, progress);
        scene.spawnItem(0.5, 68.0, 0.5, new ItemStack(Items.COBBLESTONE, 1), 10);
        StringBuilder steps = new StringBuilder();
        steps.append('[');
        scene.writeStep(steps);
        for (int tick = 0; tick < ticks; tick++) {
            steps.append(',');
            scene.tick();
            scene.writeStep(steps);
        }
        steps.append(']');
        out.write("{\"type\":\"case\",\"name\":\"");
        out.write(name);
        out.write("\",\"kind\":\"item\",\"state\":0,\"x\":0,\"y\":0,\"z\":0");
        out.write(",\"levelSeed\":1,\"entitySeed\":1,\"hurts\":null,\"fuse\":null");
        out.write(",\"item\":");
        out.write(Integer.toString(BuiltInRegistries.ITEM.getId(Items.COBBLESTONE)));
        out.write(",\"count\":1,\"px\":0.5,\"py\":68.0,\"pz\":0.5,\"pickupDelay\":10");
        out.write(",\"hopper\":false");
        out.write(",\"shapes\":");
        writePistonShapes(out, pistonPos, (PistonMovingBlockEntity) scene.level.getBlockEntity(pistonPos));
        out.write(",\"pistons\":");
        writePiston(out, pistonPos, moved, facing, extending, source, progress);
        out.write(",\"blocks\":");
        writeBlocks(out, blocks);
        out.write(",\"steps\":");
        out.write(steps.toString());
        out.write("}\n");
    }

    private static void writePistonShapes(
            Writer out,
            BlockPos pos,
            PistonMovingBlockEntity piston)
            throws Exception {
        VoxelShape shape = piston.getCollisionShape(null, pos);
        out.write("[");
        boolean first = true;
        for (AABB box : shape.toAabbs()) {
            if (!first) {
                out.write(',');
            }
            first = false;
            out.write("[");
            out.write(Double.toString(box.minX));
            out.write(",");
            out.write(Double.toString(box.minY));
            out.write(",");
            out.write(Double.toString(box.minZ));
            out.write(",");
            out.write(Double.toString(box.maxX));
            out.write(",");
            out.write(Double.toString(box.maxY));
            out.write(",");
            out.write(Double.toString(box.maxZ));
            out.write("]");
        }
        out.write("]");
    }

    private static void writePiston(
            Writer out,
            BlockPos pos,
            BlockState moved,
            Direction facing,
            boolean extending,
            boolean source,
            float progress)
            throws Exception {
        out.write("[{\"x\":");
        out.write(Integer.toString(pos.getX()));
        out.write(",\"y\":");
        out.write(Integer.toString(pos.getY()));
        out.write(",\"z\":");
        out.write(Integer.toString(pos.getZ()));
        out.write(",\"state\":");
        out.write(Integer.toString(Block.getId(moved)));
        out.write(",\"facing\":");
        out.write(Integer.toString(facing.get3DDataValue()));
        out.write(",\"extending\":");
        out.write(Boolean.toString(extending));
        out.write(",\"source\":");
        out.write(Boolean.toString(source));
        out.write(",\"progress\":");
        out.write(Float.toString(progress));
        out.write("}]");
    }

    /** 物品从空中落到石头上。速度用实体自己的随机源，和 `spawnAtLocation` 一样。 */
    private static int itemFall(Official official, Writer out) throws Exception {
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 64, 0), Blocks.STONE.defaultBlockState());
        playItem(official, out, "item_fall", Items.COBBLESTONE, 1, 0.5, 70.0, 0.5, 10, blocks, false, 40);
        return 1;
    }

    /** 水里上浮。 */
    private static int itemWater(Official official, Writer out) throws Exception {
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 63, 0), Blocks.STONE.defaultBlockState());
        for (int y = 64; y <= 68; y++) {
            blocks.put(BlockPos.asLong(0, y, 0), Blocks.WATER.defaultBlockState());
        }
        playItem(official, out, "item_water", Items.COBBLESTONE, 1, 0.5, 66.0, 0.5, 10, blocks, false, 40);
        return 1;
    }

    /** 岩浆里上浮，并按官方扣血。 */
    private static int itemLava(Official official, Writer out) throws Exception {
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 63, 0), Blocks.STONE.defaultBlockState());
        for (int y = 64; y <= 68; y++) {
            blocks.put(BlockPos.asLong(0, y, 0), Blocks.LAVA.defaultBlockState());
        }
        playItem(official, out, "item_lava", Items.COBBLESTONE, 1, 0.5, 66.0, 0.5, 10, blocks, false, 30);
        return 1;
    }

    /** 两堆同类物品靠在一起，按官方合并。 */
    private static int itemMerge(Official official, Writer out) throws Exception {
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 64, 0), Blocks.STONE.defaultBlockState());
        playItem(
                official,
                out,
                "item_merge",
                Items.COBBLESTONE,
                20,
                0.5,
                65.2,
                0.5,
                10,
                blocks,
                false,
                45,
                new double[] {0.55, 65.2, 0.5, 10});
        return 1;
    }

    /** 物品和漏斗方块重叠，走 `HopperBlock.entityInside`。 */
    private static int itemHopper(Official official, Writer out) throws Exception {
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 64, 0), Blocks.HOPPER.defaultBlockState());
        playItem(official, out, "item_hopper", Items.COBBLESTONE, 1, 0.5, 64.5, 0.5, 10, blocks, true, 8);
        return 1;
    }

    /** 物品只在吸取盒上半截，等漏斗 tick 的 `suckInItems`。 */
    private static int itemAboveHopper(Official official, Writer out) throws Exception {
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 64, 0), Blocks.HOPPER.defaultBlockState());
        playItem(official, out, "item_above_hopper", Items.COBBLESTONE, 1, 0.5, 65.2, 0.5, 0, blocks, true, 8);
        return 1;
    }

    /** 生存模式，延迟为 0，站在物品上就捡走。 */
    private static int itemPickup(Official official, Writer out) throws Exception {
        playPickup(official, out, "item_pickup", 5, 0, false, false, 0, 4);
        return 1;
    }

    /** 快捷栏已经有 63 个，再捡 5 个，剩下的留在地上。 */
    private static int itemPickupPartial(Official official, Writer out) throws Exception {
        playPickup(official, out, "item_pickup_partial", 5, 0, false, false, 63, 4);
        return 1;
    }

    /** 创造模式背包全满时，捡不进去的物品被清掉，地上也不留。 */
    private static int itemPickupCreativeFull(Official official, Writer out) throws Exception {
        playPickup(official, out, "item_pickup_creative_full", 1, 0, true, false, -1, 4);
        return 1;
    }

    /** 旁观模式不捡。 */
    private static int itemPickupSpectator(Official official, Writer out) throws Exception {
        playPickup(official, out, "item_pickup_spectator", 1, 0, false, true, 0, 20);
        return 1;
    }

    /** 默认延迟 10。捡起发生在延迟减到 0 的下一拍。 */
    private static int itemPickupDelay(Official official, Writer out) throws Exception {
        playPickup(official, out, "item_pickup_delay", 1, 10, false, false, 0, 16);
        return 1;
    }

    /** `INFINITE_PICKUP_DELAY` 不减，也捡不起来。 */
    private static int itemPickupInfinite(Official official, Writer out) throws Exception {
        playPickup(official, out, "item_pickup_infinite", 1, 32767, false, false, 0, 8);
        return 1;
    }

    private static void playPickup(
            Official official,
            Writer out,
            String name,
            int count,
            int pickupDelay,
            boolean creative,
            boolean spectator,
            int prefill,
            int ticks)
            throws Exception {
        Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        blocks.put(BlockPos.asLong(0, 63, 0), Blocks.STONE.defaultBlockState());
        Scene scene = Scene.items(official, blocks, false);
        scene.player = touchPlayer(creative);
        scene.spectator = spectator;
        if (prefill < 0) {
            ItemStack stone = new ItemStack(Items.STONE, Items.STONE.getDefaultMaxStackSize());
            for (int slot = 0; slot < 36; slot++) {
                scene.player.getInventory().setItem(slot, stone.copy());
            }
        } else if (prefill > 0) {
            scene.player.getInventory().setItem(0, new ItemStack(Items.COBBLESTONE, prefill));
        }
        scene.spawnItem(0.5, 64.5, 0.5, new ItemStack(Items.COBBLESTONE, count), pickupDelay, true);
        StringBuilder steps = new StringBuilder();
        steps.append('[');
        scene.writeStep(steps);
        for (int tick = 0; tick < ticks; tick++) {
            steps.append(',');
            scene.tick();
            scene.writeStep(steps);
        }
        steps.append(']');
        out.write("{\"type\":\"case\",\"name\":\"");
        out.write(name);
        out.write("\",\"kind\":\"item\",\"state\":0,\"x\":0,\"y\":0,\"z\":0");
        out.write(",\"levelSeed\":1,\"entitySeed\":1,\"hurts\":null,\"fuse\":null");
        out.write(",\"item\":");
        out.write(Integer.toString(BuiltInRegistries.ITEM.getId(Items.COBBLESTONE)));
        out.write(",\"count\":");
        out.write(Integer.toString(count));
        out.write(",\"px\":0.5,\"py\":64.5,\"pz\":0.5,\"pickupDelay\":");
        out.write(Integer.toString(pickupDelay));
        out.write(",\"hopper\":false,\"player\":{\"creative\":");
        out.write(Boolean.toString(creative));
        out.write(",\"spectator\":");
        out.write(Boolean.toString(spectator));
        out.write(",\"prefill\":");
        out.write(Integer.toString(prefill));
        Item fill = prefill < 0 ? Items.STONE : Items.COBBLESTONE;
        int fillCount = prefill < 0 ? Items.STONE.getDefaultMaxStackSize() : Math.max(prefill, 0);
        out.write(",\"fillItem\":");
        out.write(Integer.toString(BuiltInRegistries.ITEM.getId(fill)));
        out.write(",\"fillCount\":");
        out.write(Integer.toString(fillCount));
        out.write("},\"blocks\":");
        writeBlocks(out, blocks);
        out.write(",\"steps\":");
        out.write(steps.toString());
        out.write("}\n");
    }

    /** 不跑构造函数。`take`、`awardStat`、`onItemPickup` 都是空的，避免假玩家碰到空的统计和区块。 */
    private static TouchPlayer touchPlayer(boolean creative) {
        TouchPlayer player = allocate(TouchPlayer.class);
        Abilities abilities = new Abilities();
        abilities.instabuild = creative;
        put(player, Player.class, "abilities", abilities);
        Inventory inventory = new Inventory(player, new PlayerEquipment(player));
        put(player, Player.class, "inventory", inventory);
        return player;
    }

    private static void playItem(
            Official official,
            Writer out,
            String name,
            net.minecraft.world.item.Item item,
            int count,
            double x,
            double y,
            double z,
            int pickupDelay,
            Long2ObjectMap<BlockState> blocks,
            boolean hopper,
            int ticks,
            double... extra)
            throws Exception {
        Scene scene = Scene.items(official, blocks, hopper);
        scene.spawnItem(x, y, z, new ItemStack(item, count), pickupDelay);
        if (extra.length == 4) {
            scene.spawnItem(extra[0], extra[1], extra[2], new ItemStack(item, (int) extra[3]), pickupDelay);
        }
        StringBuilder steps = new StringBuilder();
        steps.append('[');
        scene.writeStep(steps);
        for (int tick = 0; tick < ticks; tick++) {
            steps.append(',');
            scene.tick();
            scene.writeStep(steps);
        }
        steps.append(']');
        out.write("{\"type\":\"case\",\"name\":\"");
        out.write(name);
        out.write("\",\"kind\":\"item\",\"state\":0,\"x\":0,\"y\":0,\"z\":0");
        out.write(",\"levelSeed\":1,\"entitySeed\":1,\"hurts\":null,\"fuse\":null");
        out.write(",\"item\":");
        out.write(Integer.toString(BuiltInRegistries.ITEM.getId(item)));
        out.write(",\"count\":");
        out.write(Integer.toString(count));
        out.write(",\"px\":");
        out.write(Double.toString(x));
        out.write(",\"py\":");
        out.write(Double.toString(y));
        out.write(",\"pz\":");
        out.write(Double.toString(z));
        out.write(",\"pickupDelay\":");
        out.write(Integer.toString(pickupDelay));
        out.write(",\"hopper\":");
        out.write(Boolean.toString(hopper));
        if (extra.length == 4) {
            out.write(",\"extras\":[{\"item\":");
            out.write(Integer.toString(BuiltInRegistries.ITEM.getId(item)));
            out.write(",\"count\":");
            out.write(Integer.toString((int) extra[3]));
            out.write(",\"x\":");
            out.write(Double.toString(extra[0]));
            out.write(",\"y\":");
            out.write(Double.toString(extra[1]));
            out.write(",\"z\":");
            out.write(Double.toString(extra[2]));
            out.write("}]");
        }
        out.write(",\"blocks\":");
        writeBlocks(out, blocks);
        out.write(",\"steps\":");
        out.write(steps.toString());
        out.write("}\n");
    }

    private static void play(
            Official official,
            Writer out,
            String name,
            String kind,
            BlockState state,
            int x,
            int y,
            int z,
            Long2ObjectMap<BlockState> blocks,
            long levelSeed,
            long entitySeed,
            float[] hurts,
            Integer fuse,
            int ticks)
            throws Exception {
        Scene scene = Scene.start(official, kind, state, x, y, z, blocks, levelSeed, entitySeed, hurts, fuse, 1);
        StringBuilder steps = new StringBuilder();
        steps.append('[');
        scene.writeStep(steps);
        for (int tick = 0; tick < ticks; tick++) {
            steps.append(',');
            scene.tick();
            scene.writeStep(steps);
        }
        steps.append(']');
        out.write("{\"type\":\"case\",\"name\":\"");
        out.write(name);
        out.write("\",\"kind\":\"");
        out.write(kind);
        out.write("\",\"state\":");
        out.write(Integer.toString(Block.getId(state)));
        out.write(",\"x\":");
        out.write(Integer.toString(x));
        out.write(",\"y\":");
        out.write(Integer.toString(y));
        out.write(",\"z\":");
        out.write(Integer.toString(z));
        out.write(",\"levelSeed\":");
        out.write(Long.toString(levelSeed));
        out.write(",\"entitySeed\":");
        out.write(Long.toString(entitySeed));
        out.write(",\"hurts\":");
        if (hurts == null) {
            out.write("null");
        } else {
            out.write("[" + hurts[0] + "," + hurts[1] + "]");
        }
        out.write(",\"fuse\":");
        out.write(fuse == null ? "null" : Integer.toString(fuse));
        out.write(",\"blocks\":");
        writeBlocks(out, blocks);
        out.write(",\"steps\":");
        out.write(steps.toString());
        out.write("}\n");
    }

    private static void writeBlocks(Writer out, Long2ObjectMap<BlockState> blocks) throws Exception {
        long[] keys = blocks.keySet().toLongArray();
        Arrays.sort(keys);
        out.write("[");
        boolean first = true;
        for (long key : keys) {
            BlockState state = blocks.get(key);
            if (state.isAir()) {
                continue;
            }
            if (!first) {
                out.write(',');
            }
            first = false;
            BlockPos pos = BlockPos.of(key);
            out.write("[" + pos.getX() + "," + pos.getY() + "," + pos.getZ() + "," + Block.getId(state) + "]");
        }
        out.write("]");
    }

    private static final class Scene {
        final FakeLevel level;
        final List<Entity> active = new ArrayList<>();
        /**
         * tick 结束时的实体，包括这一拍刚丢掉的。
         *
         * Entities at the end of the tick, including ones discarded during it.
         */
        final List<Entity> snapshot = new ArrayList<>();
        final List<HopperBlockEntity> hoppers = new ArrayList<>();
        TouchPlayer player;
        boolean spectator;
        boolean ticked;
        /** 官方的 `spawnAtLocation` 已经设过拾取延迟；这一拍新生成的物品不用重取速度。 */
        boolean fallingDrops;

        private Scene(FakeLevel level) {
            this.level = level;
        }

        /** 物品场景。实体 ID 从 `1 << 24` 起，和 Bamboo 的第一号非玩家实体一样。 */
        static Scene items(Official official, Long2ObjectMap<BlockState> blocks, boolean hopper) {
            FakeLevel level = FakeLevel.create(official, 1L);
            level.nextId = FIRST_ITEM_ID;
            level.blocks.putAll(blocks);
            Scene scene = new Scene(level);
            if (hopper) {
                long[] keys = blocks.keySet().toLongArray();
                Arrays.sort(keys);
                for (long key : keys) {
                    BlockState state = blocks.get(key);
                    if (state.getBlock() == Blocks.HOPPER) {
                        BlockPos pos = BlockPos.of(key);
                        HopperBlockEntity entity = new HopperBlockEntity(pos, state);
                        level.blockEntities.put(key, entity);
                        scene.hoppers.add(entity);
                    }
                }
            }
            return scene;
        }

        void spawnItem(double x, double y, double z, ItemStack stack, int pickupDelay) {
            spawnItem(x, y, z, stack, pickupDelay, false);
        }

        void spawnItem(double x, double y, double z, ItemStack stack, int pickupDelay, boolean exactDelay) {
            ItemEntity item = new ItemEntity(level, x, y, z, stack);
            level.addFreshEntity(item);
            // 构造函数先用了未设种子的随机源。换成和 Bamboo 一样的 `id ^ seed` 之后，把朝向和速度重新取一遍。
            long seed = (long) item.getId() ^ ENTITY_RANDOM_SEED;
            setRandom(item, seed);
            RandomSource random = item.getRandom();
            random.nextFloat();
            item.setYRot(random.nextFloat() * 360.0F);
            item.setDeltaMovement(random.nextDouble() * 0.2 - 0.1, 0.2, random.nextDouble() * 0.2 - 0.1);
            if (exactDelay) {
                item.setPickUpDelay(pickupDelay);
            } else if (pickupDelay == 0) {
                item.setNoPickUpDelay();
            } else {
                item.setDefaultPickUpDelay();
            }
            active.add(item);
            level.fresh.clear();
        }

        static Scene start(
                Official official,
                String kind,
                BlockState state,
                int x,
                int y,
                int z,
                Long2ObjectMap<BlockState> blocks,
                long levelSeed,
                long entitySeed,
                float[] hurts,
                Integer fuse,
                int firstId) {
            FakeLevel level = FakeLevel.create(official, levelSeed);
            level.nextId = firstId;
            level.blocks.putAll(blocks);
            BlockPos pos = new BlockPos(x, y, z);
            Entity entity;
            if ("falling".equals(kind)) {
                entity = FallingBlockEntity.fall(level, pos, state);
            } else {
                entity = new PrimedTnt(level, x + 0.5, y, z + 0.5, null);
                level.addFreshEntity(entity);
            }
            setRandom(entity, entitySeed);
            if (hurts != null && entity instanceof FallingBlockEntity falling) {
                falling.setHurtsEntities(hurts[0], (int) hurts[1]);
            }
            if (fuse != null && entity instanceof PrimedTnt tnt) {
                tnt.setFuse(fuse);
            }
            Scene scene = new Scene(level);
            scene.active.addAll(level.fresh);
            level.fresh.clear();
            return scene;
        }

        void tick() {
            // 玩家不在实体名单里。官方玩家通常先 tick，捡起看到的是物品移动前的位置。
            // The player is not in the entity list. Vanilla players usually tick first, so pickup
            // sees the item where it was before this tick's movement.
            if (player != null && !spectator) {
                AABB reach = new AABB(0.2, 64.0, 0.2, 0.8, 65.8, 0.8).inflate(1.0, 0.5, 1.0);
                for (Entity entity : new ArrayList<>(active)) {
                    if (entity instanceof ItemEntity item && item.getBoundingBox().intersects(reach)) {
                        item.playerTouch(player);
                    }
                }
            }
            List<Entity> ticking = new ArrayList<>(active);
            level.fresh.clear();
            snapshot.clear();
            for (Entity entity : ticking) {
                if (!entity.isRemoved()) {
                    entity.commonTick();
                    entity.tick();
                }
                // 丢掉的实体这一拍还要留下位置，下一拍才从名单里消失。
                // A discarded entity still reports its position for this tick.
                snapshot.add(entity);
            }
            // 这一拍新生成的下落方块、TNT，以及下落方块掉出来的物品都要留在快照里。
            // Fresh falling blocks, TNT and items dropped by falling blocks all stay in the snapshot.
            for (Entity spawned : level.fresh) {
                if (spawned instanceof ItemEntity item) {
                    if (!fallingDrops) {
                        continue;
                    }
                    // `RandomSource.create()` 每次运行都换种子。改成和 Bamboo 一样的
                    // `id ^ seed`，再按官方构造顺序重放朝向和速度。
                    // `RandomSource.create()` uses a fresh seed every run. Reseed by
                    // `id ^ seed` as Bamboo does, then replay the facing and velocity draws.
                    long seed = (long) item.getId() ^ ENTITY_RANDOM_SEED;
                    setRandom(item, seed);
                    RandomSource random = item.getRandom();
                    random.nextFloat();
                    item.setYRot(random.nextFloat() * 360.0F);
                    item.setDeltaMovement(
                            random.nextDouble() * 0.2 - 0.1,
                            0.2,
                            random.nextDouble() * 0.2 - 0.1);
                    item.setDefaultPickUpDelay();
                    snapshot.add(spawned);
                    active.add(spawned);
                } else if (spawned instanceof FallingBlockEntity || spawned instanceof PrimedTnt) {
                    snapshot.add(spawned);
                    active.add(spawned);
                }
            }
            // 官方 `ServerLevel.tick`：实体之后才是方块实体。漏斗在这里吸上方的物品。
            // 被漏斗丢掉的物品要等这一拍结束才离开名单，下一拍不再出现。
            // Vanilla ticks block entities after entities. An item the hopper discards leaves the list
            // at the end of this tick, and is absent next tick.
            level.tracked = active;
            for (HopperBlockEntity hopper : hoppers) {
                BlockPos pos = hopper.getBlockPos();
                HopperBlockEntity.pushItemsTick(level, pos, level.getBlockState(pos), hopper);
            }
            active.removeIf(Entity::isRemoved);
            ticked = true;
        }

        void writeStep(StringBuilder out) {
            List<Entity> snapped = ticked ? snapshot : active;
            out.append("{\"entities\":[");
            for (int i = 0; i < snapped.size(); i++) {
                if (i > 0) {
                    out.append(',');
                }
                writeEntity(out, snapped.get(i));
            }
            out.append("],\"blocks\":");
            out.append(blockArray(level.blocks));
            out.append(",\"containers\":");
            out.append(containerArray(hoppers));
            if (player != null) {
                out.append(",\"inventory\":");
                out.append(inventoryArray(player.getInventory()));
            }
            out.append('}');
        }
    }

    private static String containerArray(List<HopperBlockEntity> hoppers) {
        StringBuilder out = new StringBuilder();
        out.append('[');
        boolean first = true;
        for (HopperBlockEntity hopper : hoppers) {
            BlockPos pos = hopper.getBlockPos();
            for (int slot = 0; slot < hopper.getContainerSize(); slot++) {
                ItemStack stack = hopper.getItem(slot);
                if (stack.isEmpty()) {
                    continue;
                }
                if (!first) {
                    out.append(',');
                }
                first = false;
                out.append('[')
                        .append(pos.getX())
                        .append(',')
                        .append(pos.getY())
                        .append(',')
                        .append(pos.getZ())
                        .append(',')
                        .append(BuiltInRegistries.ITEM.getId(stack.getItem()))
                        .append(',')
                        .append(stack.getCount())
                        .append(']');
            }
        }
        out.append(']');
        return out.toString();
    }

    private static void writeEntity(StringBuilder out, Entity entity) {
        Vec3 pos = entity.position();
        Vec3 delta = entity.getDeltaMovement();
        String kind = entity instanceof ItemEntity ? "item" : entity instanceof PrimedTnt ? "tnt" : "falling";
        int block = entity instanceof FallingBlockEntity falling ? Block.getId(falling.getBlockState()) : -1;
        int fuse = entity instanceof PrimedTnt tnt ? tnt.getFuse() : -1;
        int item = -1;
        int count = 0;
        int age = -1;
        int pickup = -1;
        int health = -1;
        if (entity instanceof ItemEntity ground) {
            ItemStack stack = ground.getItem();
            item = BuiltInRegistries.ITEM.getId(stack.getItem());
            count = stack.getCount();
            age = ground.getAge();
            pickup = readInt(ground, "pickupDelay");
            health = readInt(ground, "health");
        }
        out.append("{\"kind\":\"").append(kind).append('"');
        out.append(",\"removed\":").append(entity.isRemoved());
        out.append(",\"onGround\":").append(entity.onGround());
        out.append(",\"x\":").append(Double.doubleToRawLongBits(pos.x));
        out.append(",\"y\":").append(Double.doubleToRawLongBits(pos.y));
        out.append(",\"z\":").append(Double.doubleToRawLongBits(pos.z));
        out.append(",\"dx\":").append(Double.doubleToRawLongBits(delta.x));
        out.append(",\"dy\":").append(Double.doubleToRawLongBits(delta.y));
        out.append(",\"dz\":").append(Double.doubleToRawLongBits(delta.z));
        out.append(",\"block\":").append(block);
        out.append(",\"fuse\":").append(fuse);
        out.append(",\"item\":").append(item);
        out.append(",\"count\":").append(count);
        out.append(",\"age\":").append(age);
        out.append(",\"pickup\":").append(pickup);
        out.append(",\"health\":").append(health);
        out.append('}');
    }

    private static int readInt(Object target, String name) {
        try {
            Field field = ItemEntity.class.getDeclaredField(name);
            field.setAccessible(true);
            return field.getInt(target);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static String blockArray(Long2ObjectMap<BlockState> blocks) {
        long[] keys = blocks.keySet().toLongArray();
        Arrays.sort(keys);
        StringBuilder out = new StringBuilder();
        out.append('[');
        boolean first = true;
        for (long key : keys) {
            BlockState state = blocks.get(key);
            if (state.isAir()) {
                continue;
            }
            if (!first) {
                out.append(',');
            }
            first = false;
            BlockPos pos = BlockPos.of(key);
            out.append('[').append(pos.getX()).append(',').append(pos.getY()).append(',').append(pos.getZ());
            out.append(',').append(Block.getId(state)).append(']');
        }
        out.append(']');
        return out.toString();
    }

    /**
     * 把实体自己的随机源换成固定种子。这个字段是 final，普通反射写不进去。
     *
     * Replace the entity's own random source with a fixed seed. The field is final, so plain reflection does not stick.
     */
    private static void setRandom(Entity entity, long seed) {
        try {
            Field field = Entity.class.getDeclaredField("random");
            field.setAccessible(true);
            UNSAFE.putObject(entity, UNSAFE.objectFieldOffset(field), RandomSource.create(seed));
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static Unsafe loadUnsafe() {
        try {
            Field field = Unsafe.class.getDeclaredField("theUnsafe");
            field.setAccessible(true);
            return (Unsafe) field.get(null);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    @SuppressWarnings("unchecked")
    private static <T> T allocate(Class<T> type) {
        try {
            return (T) UNSAFE.allocateInstance(type);
        } catch (InstantiationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static String inventoryArray(Inventory inventory) {
        StringBuilder out = new StringBuilder();
        out.append('[');
        boolean first = true;
        for (int slot = 0; slot < 36; slot++) {
            first = appendSlot(out, first, slot, inventory.getItem(slot));
        }
        appendSlot(out, first, 40, inventory.getItem(40));
        out.append(']');
        return out.toString();
    }

    private static boolean appendSlot(StringBuilder out, boolean first, int slot, ItemStack stack) {
        if (stack.isEmpty()) {
            return first;
        }
        if (!first) {
            out.append(',');
        }
        out.append('[')
                .append(slot)
                .append(',')
                .append(BuiltInRegistries.ITEM.getId(stack.getItem()))
                .append(',')
                .append(stack.getCount())
                .append(']');
        return false;
    }

    /** `abilities` 和 `inventory` 是 final，普通反射写不进去。 */
    private static void put(Object target, Class<?> owner, String name, Object value) {
        try {
            Field field = owner.getDeclaredField(name);
            field.setAccessible(true);
            UNSAFE.putObject(target, UNSAFE.objectFieldOffset(field), value);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static final class TouchPlayer extends ServerPlayer {
        private TouchPlayer() {
            super(null, null, null, null);
        }

        @Override
        public void take(Entity entity, int orgCount) {}

        @Override
        public void awardStat(Stat<?> stat, int count) {}

        @Override
        public void onItemPickup(ItemEntity entity) {}
    }

    private static void set(Class<?> owner, Object target, String name, Object value) {
        try {
            Field field = owner.getDeclaredField(name);
            field.setAccessible(true);
            field.set(target, value);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static final class Official {
        final RegistryAccess registries;
        final Holder<DimensionType> overworld;
        final Holder<Biome> plains;
        final DamageSources damageSources;
        final FakeServer server;
        final GameRules gameRules = new GameRules(FeatureFlags.DEFAULT_FLAGS);
        final WorldBorder border = new WorldBorder();
        final LevelData levelData = new FixedLevelData();
        EnvironmentAttributeSystem attributes;

        private Official(ReloadableServerResources managers) {
            this.registries = (RegistryAccess) managers.fullRegistries().lookup();
            this.overworld = registries.lookupOrThrow(Registries.DIMENSION_TYPE).getOrThrow(BuiltinDimensionTypes.OVERWORLD);
            this.plains = registries.lookupOrThrow(Registries.BIOME).getOrThrow(Biomes.PLAINS);
            this.damageSources = new DamageSources(registries);
            this.server = FakeServer.create(managers, gameRules);
        }

        static Official load(CloseableResourceManager resources) throws Exception {
            var initial = RegistryLayer.createRegistryAccess();
            List<Registry.PendingTags<?>> staticTags =
                    TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
            RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
            List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
            RegistryAccess.Frozen world = RegistryDataLoader
                    .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool())
                    .join();
            var layers = initial.replaceFrom(RegistryLayer.WORLD, world);
            ReloadableServerResources managers = ReloadableServerResources
                    .loadResources(
                            resources,
                            layers,
                            staticTags,
                            FeatureFlags.DEFAULT_FLAGS,
                            net.minecraft.commands.Commands.CommandSelection.DEDICATED,
                            LevelBasedPermissionSet.GAMEMASTER,
                            ForkJoinPool.commonPool(),
                            Runnable::run)
                    .get();
            managers.updateComponentsAndStaticRegistryTags();
            managers.getRecipeManager().finalizeRecipeLoading(FeatureFlags.DEFAULT_FLAGS);
            Official official = new Official(managers);
            FakeLevel sample = FakeLevel.create(official, 0L);
            official.attributes = EnvironmentAttributeSystem.builder().addStaticLayers(sample).build();
            set(ServerLevel.class, sample, "environmentAttributes", official.attributes);
            return official;
        }
    }

    private static final class FakeServer extends DedicatedServer {
        private ReloadableServerResources managers;
        private GameRules rules;
        private ServerDebugSubscribers debugSubscribers;

        private FakeServer() {
            super(null, null, null, null, null, null, null, null, null, null);
        }

        static FakeServer create(ReloadableServerResources managers, GameRules rules) {
            FakeServer server = allocate(FakeServer.class);
            server.managers = managers;
            server.rules = rules;
            server.debugSubscribers = new ServerDebugSubscribers(server);
            return server;
        }

        @Override
        public ServerDebugSubscribers debugSubscribers() {
            return debugSubscribers;
        }

        @Override
        public net.minecraft.world.item.crafting.RecipeManager getRecipeManager() {
            return managers.getRecipeManager();
        }

        @Override
        public ReloadableServerRegistries.Holder reloadableRegistries() {
            return new NoLoot(managers.fullRegistries().lookup());
        }

        @Override
        public GameRules getGameRules() {
            return rules;
        }
    }

    /** 落地时 onPlace 会排计划刻，只需要游戏时间，别的都用不到。 */
    private static final class FixedLevelData implements LevelData {
        @Override
        public LevelData.RespawnData getRespawnData() {
            return LevelData.RespawnData.DEFAULT;
        }

        @Override
        public long getGameTime() {
            return 0L;
        }

        @Override
        public boolean isHardcore() {
            return false;
        }

        @Override
        public Difficulty getDifficulty() {
            return Difficulty.NORMAL;
        }

        @Override
        public boolean isDifficultyLocked() {
            return false;
        }
    }

    /** 空的战利品表，这样爆炸破坏方块时不从世界随机源取掉落用的数。 */
    private static final class NoLoot extends ReloadableServerRegistries.Holder {
        NoLoot(HolderLookup.Provider registries) {
            super(registries);
        }

        @Override
        public LootTable getLootTable(ResourceKey<LootTable> id) {
            return LootTable.EMPTY;
        }
    }

    private static final class FakeLevel extends ServerLevel {
        private Official official;
        private Long2ObjectOpenHashMap<BlockState> blocks;
        private List<Entity> fresh;
        private List<Entity> tracked = new ArrayList<>();
        private Long2ObjectOpenHashMap<BlockEntity> blockEntities;
        private LevelTicks<Block> blockTicks;
        private LevelTicks<Fluid> fluidTicks;
        private ServerChunkCache chunkSource;
        private int nextId = 1;
        private FluidChunk fluidChunk;

        private FakeLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false);
        }

        static FakeLevel create(Official official, long seed) {
            FakeLevel level = allocate(FakeLevel.class);
            level.official = official;
            level.fresh = new ArrayList<>();
            level.tracked = new ArrayList<>();
            level.blockEntities = new Long2ObjectOpenHashMap<>();
            level.nextId = 1;
            level.blocks = new Long2ObjectOpenHashMap<>();
            level.blockTicks = new LevelTicks<>(key -> true);
            level.fluidTicks = new LevelTicks<>(key -> true);
            level.chunkSource = chunkSource();
            set(Level.class, level, "neighborUpdater", new CollectingNeighborUpdater(level, 1_000_000));
            set(Level.class, level, "random", RandomSource.create(seed));
            set(Level.class, level, "soundSeedGenerator", RandomSource.createThreadSafe());
            set(Level.class, level, "dimensionTypeRegistration", official.overworld);
            set(Level.class, level, "dimension", Level.OVERWORLD);
            set(Level.class, level, "registryAccess", official.registries);
            set(Level.class, level, "palettedContainerFactory", net.minecraft.world.level.chunk.PalettedContainerFactory.create(official.registries));
            set(Level.class, level, "damageSources", official.damageSources);
            set(Level.class, level, "thread", Thread.currentThread());
            set(ServerLevel.class, level, "players", new ArrayList<ServerPlayer>());
            set(ServerLevel.class, level, "server", official.server);
            set(ServerLevel.class, level, "chunkSource", level.chunkSource);
            if (official.attributes != null) {
                set(ServerLevel.class, level, "environmentAttributes", official.attributes);
            }
            return level;
        }

        private static ServerChunkCache chunkSource() {
            ServerChunkCache source = allocate(ServerChunkCache.class);
            ChunkMap map = allocate(ChunkMap.class);
            set(ChunkMap.class, map, "entityMap", new Int2ObjectOpenHashMap<>());
            set(ServerChunkCache.class, source, "chunkMap", map);
            return source;
        }

        @Override
        public BlockState getBlockState(BlockPos pos) {
            if (!isInsideBuildHeight(pos)) {
                return Blocks.VOID_AIR.defaultBlockState();
            }
            BlockState state = blocks.get(pos.asLong());
            return state != null ? state : Blocks.AIR.defaultBlockState();
        }

        @Override
        public BlockEntity getBlockEntity(BlockPos pos) {
            return blockEntities.get(pos.asLong());
        }

        @Override
        public FluidState getFluidState(BlockPos pos) {
            return isInsideBuildHeight(pos) ? getBlockState(pos).getFluidState() : net.minecraft.world.level.material.Fluids.EMPTY.defaultFluidState();
        }

        @Override
        public boolean setBlock(BlockPos pos, BlockState state, int flags, int limit) {
            if (!isInsideBuildHeight(pos)) {
                return false;
            }
            BlockState old = getBlockState(pos);
            if (old == state) {
                return false;
            }
            blocks.put(pos.asLong(), state);
            boolean movedByPiston = (flags & 64) != 0;
            if (!old.is(state.getBlock()) && ((flags & 1) != 0 || movedByPiston)) {
                old.affectNeighborsAfterRemoval(this, pos, movedByPiston);
            }
            if (!getBlockState(pos).is(state.getBlock())) {
                return false;
            }
            if ((flags & 512) == 0) {
                state.onPlace(this, pos, old, movedByPiston);
            }
            if (getBlockState(pos) == state && (flags & 1) != 0) {
                updateNeighborsAt(pos, old.getBlock());
                if (state.hasAnalogOutputSignal()) {
                    updateNeighbourForOutputSignal(pos, state.getBlock());
                }
            }
            if ((flags & 16) == 0 && limit > 0) {
                int neighbourFlags = flags & -34;
                old.updateIndirectNeighbourShapes(this, pos, neighbourFlags, limit - 1);
                state.updateNeighbourShapes(this, pos, neighbourFlags, limit - 1);
                state.updateIndirectNeighbourShapes(this, pos, neighbourFlags, limit - 1);
            }
            return true;
        }

        @Override
        public boolean addFreshEntity(Entity entity) {
            // 构造函数里已经调用过 `getNextEntityId`。这里再分配一次会把 ID 错开一号。
            // The constructor already called `getNextEntityId`. Assigning again would shift the id by one.
            fresh.add(entity);
            return true;
        }

        @Override
        public int getNextEntityId() {
            return nextId++;
        }

        @Override
        public int getMinY() {
            return MIN_Y;
        }

        @Override
        public int getHeight() {
            return HEIGHT;
        }

        @Override
        public int getSeaLevel() {
            return 63;
        }

        @Override
        public LevelTicks<Block> getBlockTicks() {
            return blockTicks;
        }

        @Override
        public LevelTicks<Fluid> getFluidTicks() {
            return fluidTicks;
        }

        @Override
        public ServerChunkCache getChunkSource() {
            return chunkSource;
        }

        @Override
        public BlockGetter getChunkForCollisions(int chunkX, int chunkZ) {
            return this;
        }

        @Override
        public ChunkAccess getChunk(int chunkX, int chunkZ, ChunkStatus status, boolean loadOrGenerate) {
            // 流体扫描先问区块的 section 有没有流体。不加载的那一次只为了这个检查。
            // The fluid scan asks whether a chunk section contains fluid. The non-loading call is only that check.
            if (loadOrGenerate) {
                return null;
            }
            if (fluidChunk == null) {
                fluidChunk = new FluidChunk(this);
            }
            return fluidChunk;
        }

        /** 漏斗 `setChanged` 会来问区块。假世界没有区块，标记存档这一步空着。 */
        @Override
        public void blockEntityChanged(BlockPos pos) {}

        @Override
        public boolean hasChunk(int chunkX, int chunkZ) {
            return true;
        }

        @Override
        public boolean isLoaded(BlockPos pos) {
            return isInsideBuildHeight(pos);
        }

        @Override
        public GameRules getGameRules() {
            return official.gameRules;
        }

        @Override
        public WorldBorder getWorldBorder() {
            return official.border;
        }

        @Override
        public LevelData getLevelData() {
            return official.levelData;
        }

        @Override
        public DedicatedServer getServer() {
            return official.server;
        }

        @Override
        public net.minecraft.world.flag.FeatureFlagSet enabledFeatures() {
            return FeatureFlags.DEFAULT_FLAGS;
        }

        @Override
        public EnvironmentAttributeSystem environmentAttributes() {
            return official.attributes;
        }

        @Override
        public Holder<Biome> getNoiseBiome(int quartX, int quartY, int quartZ) {
            return official.plains;
        }

        @Override
        public Holder<Biome> getUncachedNoiseBiome(int quartX, int quartY, int quartZ) {
            return official.plains;
        }

        @Override
        public Difficulty getDifficulty() {
            return Difficulty.NORMAL;
        }

        @Override
        public void gameEvent(Holder<GameEvent> gameEvent, Vec3 position, GameEvent.Context context) {}

        @Override
        public void sendBlockUpdated(BlockPos pos, BlockState old, BlockState current, int flags) {}

        @Override
        public void levelEvent(Entity source, int type, BlockPos pos, int data) {}

        @Override
        public void playSeededSound(
                Entity except, double x, double y, double z, Holder<net.minecraft.sounds.SoundEvent> sound,
                net.minecraft.sounds.SoundSource source, float volume, float pitch, long seed) {}

        @Override
        public void playSeededSound(
                Entity except, Entity sourceEntity, Holder<net.minecraft.sounds.SoundEvent> sound,
                net.minecraft.sounds.SoundSource source, float volume, float pitch, long seed) {}

        @Override
        public Collection<EnderDragonPart> dragonParts() {
            return List.of();
        }

        @Override
        protected LevelEntityGetter<Entity> getEntities() {
            return new TrackedEntities(tracked);
        }

        @Override
        public void updatePOIOnBlockStateChange(BlockPos pos, BlockState oldState, BlockState newState) {}

        @Override
        public StructureManager structureManager() {
            throw new UnsupportedOperationException("structure");
        }
    }

    private static final class TrackedEntities implements LevelEntityGetter<Entity> {
        private final List<Entity> entities;

        TrackedEntities(List<Entity> entities) {
            this.entities = entities;
        }

        @Override
        public Entity get(int id) {
            for (Entity entity : entities) {
                if (entity.getId() == id) {
                    return entity;
                }
            }
            return null;
        }

        @Override
        public Entity get(UUID id) {
            for (Entity entity : entities) {
                if (entity.getUUID().equals(id)) {
                    return entity;
                }
            }
            return null;
        }

        @Override
        public Iterable<Entity> getAll() {
            return entities;
        }

        @Override
        public <U extends Entity> void get(EntityTypeTest<Entity, U> type, AbortableIterationConsumer<U> consumer) {
            for (Entity entity : entities) {
                U cast = type.tryCast(entity);
                if (cast != null && consumer.accept(cast).shouldAbort()) {
                    return;
                }
            }
        }

        @Override
        public void get(AABB box, Consumer<Entity> output) {
            for (Entity entity : entities) {
                if (entity.getBoundingBox().intersects(box)) {
                    output.accept(entity);
                }
            }
        }

        @Override
        public <U extends Entity> void get(EntityTypeTest<Entity, U> type, AABB box, AbortableIterationConsumer<U> consumer) {
            for (Entity entity : entities) {
                U cast = type.tryCast(entity);
                if (cast != null && entity.getBoundingBox().intersects(box) && consumer.accept(cast).shouldAbort()) {
                    return;
                }
            }
        }
    }

    /**
     * 流体扫描只看 section 的 `hasFluid`。方块本身仍由 {@code FakeLevel} 回答。
     *
     * The fluid scan only checks a section's `hasFluid`. Block lookups still go to {@code FakeLevel}.
     */
    private static final class FluidChunk extends ChunkAccess {
        FluidChunk(Level level) {
            super(
                    new net.minecraft.world.level.ChunkPos(0, 0),
                    net.minecraft.world.level.chunk.UpgradeData.EMPTY,
                    level,
                    level.palettedContainerFactory(),
                    0L,
                    null,
                    null);
            try {
                Field field = net.minecraft.world.level.chunk.LevelChunkSection.class.getDeclaredField("fluidCount");
                field.setAccessible(true);
                for (net.minecraft.world.level.chunk.LevelChunkSection section : getSections()) {
                    field.setShort(section, (short) 1);
                }
            } catch (ReflectiveOperationException error) {
                throw new IllegalStateException(error);
            }
        }

        @Override
        public BlockEntity getBlockEntity(BlockPos pos) {
            return null;
        }

        @Override
        public BlockState getBlockState(BlockPos pos) {
            return Blocks.AIR.defaultBlockState();
        }

        @Override
        public net.minecraft.world.level.material.FluidState getFluidState(BlockPos pos) {
            return net.minecraft.world.level.material.Fluids.EMPTY.defaultFluidState();
        }

        @Override
        public BlockState setBlockState(BlockPos pos, BlockState state, int flags) {
            return null;
        }

        @Override
        public void setBlockEntity(BlockEntity blockEntity) {}

        @Override
        public void addEntity(Entity entity) {}

        @Override
        public ChunkStatus getPersistedStatus() {
            return ChunkStatus.FULL;
        }

        @Override
        public void removeBlockEntity(BlockPos pos) {}

        @Override
        public net.minecraft.nbt.CompoundTag getBlockEntityNbtForSaving(BlockPos pos, HolderLookup.Provider registries) {
            return null;
        }

        @Override
        public net.minecraft.world.ticks.TickContainerAccess<Block> getBlockTicks() {
            return null;
        }

        @Override
        public net.minecraft.world.ticks.TickContainerAccess<net.minecraft.world.level.material.Fluid> getFluidTicks() {
            return null;
        }

        @Override
        public PackedTicks getTicksForSerialization(long gameTime) {
            return null;
        }
    }

    private EntityOracle() {}
}
