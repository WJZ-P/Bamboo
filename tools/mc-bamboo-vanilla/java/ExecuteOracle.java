// 整条 `/execute` 命令的官方对拍。
// 在 JVM 内搭一个最小的 `ServerLevel`，用官方 `Commands.performPrefixedCommand`
// 跑固定脚本。每条命令都从干净场景开始；输出方块表、计分板、是否跑到 `run`
// 和反馈文本的哈希，Rust 侧在同样的场景里重放比较。
//
// Oracle for whole `/execute` commands. A minimal in-JVM `ServerLevel` and
// official `Commands.performPrefixedCommand`; each case starts from a clean
// scene. Emits blocks, score, whether `run` happened, and a feedback hash.

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
import java.util.List;
import java.util.concurrent.ForkJoinPool;
import net.minecraft.SharedConstants;
import net.minecraft.commands.CommandSource;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.commands.Commands;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.registries.Registries;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerRegistries;
import net.minecraft.util.AbortableIterationConsumer;
import net.minecraft.util.RandomSource;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.ServerScoreboard;
import net.minecraft.server.dedicated.DedicatedServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.Difficulty;
import net.minecraft.world.DifficultyInstance;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.animal.cow.Cow;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.effect.MobEffectInstance;
import net.minecraft.server.network.ServerGamePacketListenerImpl;
import net.minecraft.network.protocol.Packet;
import net.minecraft.server.network.ServerCommonPacketListenerImpl;
import net.minecraft.network.Connection;
import net.minecraft.network.protocol.PacketFlow;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.entity.LevelEntityGetter;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.level.storage.LevelData;
import net.minecraft.world.phys.Vec2;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.scores.Objective;
import net.minecraft.world.scores.ScoreHolder;
import net.minecraft.world.scores.criteria.ObjectiveCriteria;
import sun.misc.Unsafe;

public final class ExecuteOracle {
    private static final Unsafe UNSAFE = loadUnsafe();
    private static final BlockPos TEST_POS = new BlockPos(4, -60, 6);
    /** 设了 `EXECUTE_ORACLE_TRACE` 时把每条命令的反馈原文和异常栈打到 stderr。 */
    private static final boolean DEBUG = System.getenv("EXECUTE_ORACLE_TRACE") != null;
    /** `Bootstrap.bootStrap` 会把 System.err 接到日志上，这里先留一份原始的。 */
    private static final PrintStream RAW_ERR = System.err;
    /** 32 条整命令场景；`store bossbar`、`in`、`if score` 和 NBT 目标需要更完整的
     *  WorldData / 谓词管理器 / NBT 数据源，见 README 的已知差异。 */
    private static final String[] SCENARIOS = {
        // 基础 run / 条件反馈
        "execute positioned 4 -60 6 run setblock 4 -60 6 stone",
        "execute if block 4 -60 6 stone run setblock 4 -60 6 stone",
        "execute unless block 4 -60 6 stone run setblock 4 -60 6 stone",
        "execute if block 2 -60 2 stone run setblock 4 -60 6 stone",
        "execute unless block 2 -60 2 stone run setblock 4 -60 6 stone",
        "execute if entity @e[type=cow] run setblock 4 -60 6 stone",
        "execute unless entity @e[type=cow] run setblock 4 -60 6 stone",
        "execute unless entity @e[type=slime] run setblock 4 -60 6 stone",
        // as / at 分叉与顺序
        "execute as @e[type=cow] at @s run setblock ~ ~ ~ stone",
        "execute at @e[type=cow] run setblock ~ ~ ~ stone",
        "execute as @e[type=cow,sort=nearest,limit=2] at @s run setblock ~ ~ ~ stone",
        "execute as @e[type=cow,sort=nearest,limit=1] at @s run setblock 4 -60 6 stone",
        "execute as @e[type=cow,sort=furthest,limit=1] at @s run setblock 4 -60 6 stone",
        "execute as @s at @s run setblock 4 -60 6 stone",
        // positioned / rotated / facing / align / anchored / in
        "execute positioned 2 -60 2 run setblock ~ ~ ~ stone",
        "execute positioned as @e[type=cow,limit=1,sort=nearest] run setblock 4 -60 6 stone",
        "execute rotated 90 0 run setblock 4 -60 6 stone",
        "execute rotated as @s run setblock 4 -60 6 stone",
        "execute facing 4 -60 6 run setblock 4 -60 6 stone",
        "execute facing entity @e[type=cow,limit=1,sort=nearest] eyes run setblock 4 -60 6 stone",
        "execute facing entity @e[type=cow,limit=1,sort=nearest] feet run setblock 4 -60 6 stone",
        "execute align xyz run setblock 4 -60 6 stone",
        "execute anchored eyes run setblock 4 -60 6 stone",
        "execute anchored feet run setblock 4 -60 6 stone",
        // blocks all / masked
        "execute if blocks 2 -60 2 2 -60 2 6 -60 6 all run setblock 4 -60 6 stone",
        "execute unless blocks 2 -60 2 2 -60 2 6 -60 6 masked run setblock 4 -60 6 stone",
        // score 条件
        // store 写回次数与值
        "execute store result score #total result run setblock 4 -60 6 stone",
        "execute store success score #total result run setblock 4 -60 6 stone",
        "execute as @e[type=cow] at @s store success score #total result run setblock ~ ~ ~ stone",
        "execute as @e[type=cow] at @s store result score #total result run setblock ~ ~ ~ stone",
        // 条件失败时的返回值
        "execute if score #total result matches 0 run setblock 4 -60 6 stone",
        "execute unless score #total result matches 0 run setblock 4 -60 6 stone",
        "execute if score #total result = #total result run setblock 4 -60 6 stone",
        "execute if block 4 -60 6 stone store success score #total result run setblock 4 -60 6 stone",
        "execute unless block 4 -60 6 stone store result score #total result run setblock 4 -60 6 stone",
        // /data 整条命令与 /execute 的 NBT 数据源
        "data merge storage minecraft:test {Value:7}",
        "data get storage minecraft:test Value",
        "data remove storage minecraft:test Value",
        "data modify storage minecraft:test Other set value 3",
        "execute if data storage minecraft:test Value run setblock 4 -60 6 stone",
        "execute unless data storage minecraft:test Missing run setblock 4 -60 6 stone",
        "execute store result storage minecraft:test Value int 1 run setblock 4 -60 6 stone",
        // /effect give|clear 的副作用与反馈（单/多目标、非生物目标、infinite、
        // hideParticles、强度上限、时长上限、清除指定效果、无可清效果）
        "effect give @s minecraft:speed 30 1",
        "effect give @s minecraft:speed infinite 2",
        "effect give @s minecraft:speed 1000000 255",
        "effect give @s minecraft:speed 3 0 true",
        "effect give @e[type=cow] minecraft:regeneration 12 1",
        "effect give @s minecraft:instant_health 5 2",
        "effect clear @s",
        "effect clear @s minecraft:speed",
        "effect clear @e[type=cow]",
        "effect give @s minecraft:speed 30 1 ;; effect clear @s minecraft:haste",
        "effect give @s minecraft:speed 30 1 ;; effect clear @s",
        "effect give @s minecraft:speed 30 1 ;; effect clear @s minecraft:speed",
        "effect give @e[type=item] minecraft:speed 10",
        // 条件写在链尾：官方走条件节点自己的 executes
        "execute if block 4 -60 6 air",
        "execute unless block 4 -60 6 air",
        "execute if entity @e[type=cow]",
        "execute unless entity @e[type=cow]",
        "execute unless entity @e[type=slime]",
        "execute if blocks 2 -60 2 2 -60 2 6 -60 6 all",
        "execute unless blocks 2 -60 2 2 -60 2 6 -60 6 masked",
        "execute if score #total result matches 0",
        "execute unless score #total result matches 0",
        "execute as @e[type=cow] if entity @s",
        // biome / dimension / loaded / stopwatch
        "execute if biome 4 -60 6 minecraft:plains run setblock 4 -60 6 stone",
        "execute unless biome 4 -60 6 minecraft:plains run setblock 4 -60 6 stone",
        "execute if biome 4 -60 6 minecraft:desert run setblock 4 -60 6 stone",
        "execute if biome 4 -60 6 #minecraft:is_overworld run setblock 4 -60 6 stone",
        "execute if biome 4 -60 6 #minecraft:is_nether run setblock 4 -60 6 stone",
        "execute unless biome 4 -60 6 #minecraft:is_nether run setblock 4 -60 6 stone",
        "execute if biome 4 -60 6 minecraft:plains",
        "execute if biome 4 -60 6 minecraft:desert",
        "execute if dimension minecraft:overworld run setblock 4 -60 6 stone",
        "execute unless dimension minecraft:overworld run setblock 4 -60 6 stone",
        "execute if dimension minecraft:nope run setblock 4 -60 6 stone",
        "execute if dimension minecraft:overworld",
        "execute if loaded 4 -60 6 run setblock 4 -60 6 stone",
        "execute unless loaded 4 -60 6 run setblock 4 -60 6 stone",
        "execute if loaded 4 -60 6",
        "execute if stopwatch minecraft:nope 0.. run setblock 4 -60 6 stone",
        "execute if dimension minecraft:nope",
        "execute if stopwatch minecraft:nope 0..",
        "execute as @e[type=cow] run setblock 4 -60 6 stone",
        "execute as @e[type=cow] if dimension minecraft:nope run setblock 4 -60 6 stone",
        "execute as @e[type=cow] unless entity @s",
        // if predicate：引用、实体类型、效果、NBT、队伍、闪电、玩家
        "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{}} run setblock 4 -60 6 stone",
        "execute unless predicate {type:\"entity_properties\",entity:\"this\",predicate:{}} run setblock 4 -60 6 stone",
        "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{}}",
        "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/lightning\":{}}}",
        "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{team:\"red\"}}",
        "execute unless predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{\"minecraft:speed\":{}}}}",
        "execute if predicate {type:\"entity_scores\",entity:\"this\",scores:{result:{min:-9}}}",
        "execute if predicate {type:\"location_check\",predicate:{fluid:{fluids:\"#minecraft:water\"}}}",
        "execute if predicate {type:\"location_check\",predicate:{biomes:\"#minecraft:is_nether\"}}",
        "execute unless predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/player\":{gamemode:[\"survival\"]}}}",
        "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"minecraft:entity_type\":\"minecraft:player\"}} run setblock 4 -60 6 stone",
        "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{entity_type:\"cow\"}} run setblock 4 -60 6 stone",
        "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{entity_type:\"cow\"}} run setblock ~ ~ ~ stone",
        "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{entity_type:\"#minecraft:undead\"}} run setblock ~ ~ ~ stone",
        "effect give @e[type=cow,sort=nearest,limit=1] minecraft:speed 30 1 ;; execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{\"minecraft:speed\":{}}}} run setblock ~ ~ ~ stone",
        "effect give @e[type=cow,sort=nearest,limit=1] minecraft:speed 30 1 ;; execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{\"minecraft:speed\":{amplifier:1}}}} run setblock ~ ~ ~ stone",
        "effect give @e[type=cow,sort=nearest,limit=1] minecraft:speed 30 1 ;; execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{\"minecraft:speed\":{amplifier:{min:2}}}}} run setblock ~ ~ ~ stone",
        "effect give @e[type=cow,sort=nearest,limit=1] minecraft:speed 30 1 ;; execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{\"minecraft:speed\":{duration:{min:100},ambient:false,visible:true}}}} run setblock ~ ~ ~ stone",
        "effect give @e[type=cow,sort=nearest,limit=1] minecraft:speed 30 1 ;; execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{\"minecraft:speed\":{duration:{max:100}}}}} run setblock ~ ~ ~ stone",
        "effect give @e[type=cow,sort=nearest,limit=1] minecraft:speed 30 1 ;; execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{\"minecraft:speed\":{},\"minecraft:haste\":{}}}} run setblock ~ ~ ~ stone",
        "effect give @e[type=cow,sort=nearest,limit=1] minecraft:speed 30 1 ;; execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{}}} run setblock ~ ~ ~ stone",
        "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{\"minecraft:speed\":{}}}} run setblock 4 -60 6 stone",
        "effect give @s minecraft:speed 30 0 true ;; execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{effects:{speed:{visible:false}}}} run setblock 4 -60 6 stone",
        "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{nbt:\"{Health:10.0f}\"}} run setblock ~ ~ ~ stone",
        "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{nbt:{Health:10.0f}}} run setblock ~ ~ ~ stone",
        "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{nbt:\"{Health:10}\"}} run setblock ~ ~ ~ stone",
        "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{nbt:\"{Health:11.0f}\"}} run setblock ~ ~ ~ stone",
        "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{nbt:\"{}\"}} run setblock ~ ~ ~ stone",
        "team add red ;; team join red @s ;; execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{team:\"red\"}} run setblock 4 -60 6 stone",
        "team add red ;; team join red @s ;; execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{team:\"blue\"}} run setblock 4 -60 6 stone",
        "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{team:\"red\"}} run setblock 4 -60 6 stone",
        "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/lightning\":{}}} run setblock ~ ~ ~ stone",
        "execute as @e[type=cow] at @s if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"minecraft:type_specific/player\":{}}} run setblock ~ ~ ~ stone",
        "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/player\":{}}} run setblock 4 -60 6 stone",
        "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/player\":{gamemode:[\"creative\"],level:0,food:{level:{min:20},saturation:{max:5.0}}}}} run setblock 4 -60 6 stone",
        "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/player\":{gamemode:[\"survival\",\"adventure\"]}}} run setblock 4 -60 6 stone",
        "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/player\":{level:{min:1}}}} run setblock 4 -60 6 stone",
        "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/player\":{food:{level:{max:19}}}}} run setblock 4 -60 6 stone",
        "execute if predicate {type:\"entity_properties\",entity:\"this\",predicate:{\"type_specific/lightning\":{}}} run setblock 4 -60 6 stone",
        // entity_scores
        "scoreboard players set @s result 5 ;; execute if predicate {type:\"entity_scores\",entity:\"this\",scores:{result:{min:1,max:9}}} run setblock 4 -60 6 stone",
        "scoreboard players set @s result 5 ;; execute if predicate {type:\"minecraft:entity_scores\",entity:\"this\",scores:{result:5}} run setblock 4 -60 6 stone",
        "scoreboard players set @s result 5 ;; execute if predicate {type:\"entity_scores\",entity:\"this\",scores:{result:7}} run setblock 4 -60 6 stone",
        "scoreboard players set @s result 5 ;; execute if predicate {type:\"entity_scores\",entity:\"this\",scores:{result:{min:6}}} run setblock 4 -60 6 stone",
        "scoreboard players set @s result 5 ;; execute if predicate {type:\"entity_scores\",entity:\"this\",scores:{missing:5}} run setblock 4 -60 6 stone",
        "execute if predicate {type:\"entity_scores\",entity:\"this\",scores:{result:{min:-9}}} run setblock 4 -60 6 stone",
        "execute if predicate {type:\"entity_scores\",entity:\"this\",scores:{}} run setblock 4 -60 6 stone",
        "execute as @e[type=cow] at @s if predicate {type:\"entity_scores\",entity:\"this\",scores:{result:{min:-9}}} run setblock ~ ~ ~ stone",
        // location_check：流体和群系
        "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{fluids:\"#minecraft:water\"}}} run setblock 4 -60 6 stone",
        "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{fluids:\"minecraft:water\"}}} run setblock 4 -60 6 stone",
        "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{fluids:\"minecraft:flowing_water\"}}} run setblock 4 -60 6 stone",
        "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{fluids:[\"minecraft:lava\",\"minecraft:flowing_lava\"]}}} run setblock 4 -60 6 stone",
        "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{state:{falling:\"false\"}}}} run setblock 4 -60 6 stone",
        "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{state:{falling:\"true\"}}}} run setblock 4 -60 6 stone",
        "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{state:{level:\"8\"}}}} run setblock 4 -60 6 stone",
        "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{fluids:\"minecraft:water\",state:{falling:{min:\"false\",max:\"true\"}}}}} run setblock 4 -60 6 stone",
        "setblock 2 -60 2 water strict ;; execute positioned 2 -60 2 if predicate {type:\"location_check\",predicate:{fluid:{}}} run setblock 4 -60 6 stone",
        "execute if predicate {type:\"location_check\",predicate:{fluid:{fluids:\"minecraft:empty\"}}} run setblock 4 -60 6 stone",
        "execute if predicate {type:\"location_check\",predicate:{fluid:{fluids:\"#minecraft:water\"}}} run setblock 4 -60 6 stone",
        "execute if predicate {type:\"location_check\",predicate:{fluid:{state:{falling:\"false\"}}}} run setblock 4 -60 6 stone",
        "execute if predicate {type:\"location_check\",predicate:{biomes:\"minecraft:plains\"}} run setblock 4 -60 6 stone",
        "execute if predicate {type:\"location_check\",predicate:{biomes:\"#minecraft:is_nether\"}} run setblock 4 -60 6 stone",
        "setblock 2 -60 2 stone ;; execute positioned 2 -59 2 if predicate {type:\"location_check\",offsetY:-1,predicate:{block:{blocks:\"minecraft:stone\"}}} run setblock 4 -60 6 stone",
        // if|unless items|slots（字面量槽位范围、实体访问器、链尾数值条件）
        "execute if items entity @e[type=cow,limit=1,sort=nearest] weapon.mainhand minecraft:stone run setblock 4 -60 6 stone",
        "execute unless items entity @e[type=cow,limit=1,sort=nearest] weapon.mainhand minecraft:stone run setblock 4 -60 6 stone",
        "execute if slots entity @e[type=cow,limit=1,sort=nearest] weapon.mainhand run setblock 4 -60 6 stone",
        "execute unless slots entity @e[type=cow,limit=1,sort=nearest] weapon.mainhand run setblock 4 -60 6 stone",
        "execute if slots entity @e[type=cow,limit=1,sort=nearest] armor.head run setblock 4 -60 6 stone",
        "execute if items entity @e[type=cow,limit=1,sort=nearest] container.0 minecraft:stone run setblock 4 -60 6 stone",
        "execute if slots entity @e[type=cow] weapon.mainhand run setblock 4 -60 6 stone",
        "execute if items entity @e[type=cow,limit=1,sort=nearest] weapon.mainhand minecraft:stone",
        "execute unless items entity @e[type=cow,limit=1,sort=nearest] weapon.mainhand minecraft:stone",
        "execute if slots entity @e[type=cow,limit=1,sort=nearest] weapon.mainhand",
        "execute unless slots entity @e[type=cow,limit=1,sort=nearest] weapon.mainhand",
        "execute if slots entity @e[type=cow,limit=1,sort=nearest] weapon.* run setblock 4 -60 6 stone",
        "execute if items entity @e[type=cow,limit=1,sort=nearest] weapon.* minecraft:stone",
        // execute on（来源实体没有这些关系，全部空分支）
        "execute as @e[type=cow,limit=1,sort=nearest] on vehicle run setblock 4 -60 6 stone",
        "execute as @e[type=cow,limit=1,sort=nearest] on passengers run setblock 4 -60 6 stone",
        "execute as @e[type=cow,limit=1,sort=nearest] on owner run setblock 4 -60 6 stone",
        "execute as @e[type=cow,limit=1,sort=nearest] on attacker run setblock 4 -60 6 stone",
        "execute as @e[type=cow,limit=1,sort=nearest] on target run setblock 4 -60 6 stone",
        "execute as @e[type=cow,limit=1,sort=nearest] on leasher run setblock 4 -60 6 stone",
        "execute as @e[type=cow,limit=1,sort=nearest] on controller run setblock 4 -60 6 stone",
        "execute as @e[type=cow,limit=1,sort=nearest] on origin run setblock 4 -60 6 stone",
        // positioned over / summon
        "execute positioned over world_surface run setblock 4 -60 6 stone",
        "execute positioned over motion_blocking run setblock 4 -60 6 stone",
        "execute summon minecraft:cow run setblock 4 -60 6 stone",
        "execute summon minecraft:pig at @s run setblock 4 -60 6 stone",
    };

    /** `/advancement` 的整条命令题：官方假世界里执行，比较返回值、反馈和进度状态。 */
    private static final String[] ADVANCEMENT_CASES = {
        "advancement grant @s only minecraft:story/root",
        "advancement grant @s only minecraft:story/root crafting_table",
        "advancement revoke @s everything ;; advancement grant @s only minecraft:story/root",
        "advancement revoke @s everything",
        "advancement grant @s from minecraft:story/root",
        "advancement revoke @s until minecraft:story/upgrade_tools",
        "advancement grant @s through minecraft:story/mine_stone",
        "advancement revoke @s only minecraft:story/mine_stone get_stone",
        "advancement grant @s only minecraft:story/root nope",
        "advancement grant @s only minecraft:does_not_exist",
        "advancement revoke @s everything ;; advancement revoke @s everything",
        "advancement grant @s only minecraft:story/root ;; advancement revoke @s only minecraft:story/root",
    };

    private ExecuteOracle() {
    }

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(args, log);
        } catch (Exception error) {
            error.printStackTrace(System.err);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(String[] args, PrintStream log) throws Exception {
        boolean advancementMode = args.length == 1 && args[0].equals("advancements");
        if (args.length != 0 && !advancementMode) {
            throw new IllegalArgumentException("unexpected arguments");
        }
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        System.setOut(log);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                        new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
                Writer out = new BufferedWriter(
                        new OutputStreamWriter(stdout, StandardCharsets.UTF_8), 1 << 20)) {
            Official official = Official.load(resources);
            out.write("{\"type\":\"header\",\"version\":\"");
            out.write(SharedConstants.getCurrentVersion().id());
            out.write("\"}\n");
            if (advancementMode) {
                for (String line : ADVANCEMENT_CASES) {
                    emitAdvancement(out, official, line);
                }
            } else {
                for (String line : SCENARIOS) {
                    emit(out, official, line);
                }
            }
            out.write("{\"type\":\"end\"}\n");
            out.flush();
        }
    }

    /** 官方 `AdvancementCommands` 在假玩家上跑整条命令，输出返回值、反馈和判据状态。 */
    private static void emitAdvancement(Writer out, Official official, String line)
            throws Exception {
        Scene scene = new Scene(official, true);
        var playerAdvancements = official.server.getAdvancements();
        // 每条题从一份干净的 `PlayerAdvancements` 开始；用 Unsafe 造一个并只填
        // `award` / `revoke` 会读写的字段，不读存档、不发包。
        var state = UNSAFE.allocateInstance(net.minecraft.server.PlayerAdvancements.class);
        set(net.minecraft.server.PlayerAdvancements.class, state, "progress",
                new java.util.LinkedHashMap<>());
        set(net.minecraft.server.PlayerAdvancements.class, state, "visible",
                new java.util.HashSet<>());
        set(net.minecraft.server.PlayerAdvancements.class, state, "progressChanged",
                new java.util.HashSet<>());
        set(net.minecraft.server.PlayerAdvancements.class, state, "rootsToUpdate",
                new java.util.HashSet<>());
        set(net.minecraft.server.PlayerAdvancements.class, state, "activeTriggers",
                new java.util.IdentityHashMap<>());
        set(net.minecraft.server.PlayerAdvancements.class, state, "isFirstPacket", true);
        set(net.minecraft.server.PlayerAdvancements.class, state, "tree",
                playerAdvancements.tree());
        set(net.minecraft.server.PlayerAdvancements.class, state, "player", scene.player);
        // 官方 `award` 完成时的公告走 `playerList.broadcastSystemMessage`；
        // 直接改 GameRuleMap 把这条规则关掉（不走会广播通知的 GameRules.set）。
        var ruleMap = field(official.rules, "rules", Object.class);
        ((net.minecraft.world.level.gamerules.GameRuleMap) ruleMap).set(
                net.minecraft.world.level.gamerules.GameRules.SHOW_ADVANCEMENT_MESSAGES, false);
        set(net.minecraft.server.level.ServerPlayer.class, scene.player, "advancements", state);
        String setup = null;
        String command = line;
        int split = line.indexOf(" ;; ");
        if (split >= 0) {
            setup = line.substring(0, split);
            command = line.substring(split + 4);
        }
        List<String> setupFeedback = new ArrayList<>();
        if (setup != null) {
            CommandSourceStack setupSource = scene.source.withSource(new Recording(setupFeedback));
            official.commands.performCommand(
                    official.commands.getDispatcher().parse(setup, setupSource), setup);
        }
        List<String> feedback = new ArrayList<>();
        if (System.getenv("ADV_ORACLE_DEBUG") != null) {
            setStatic(SharedConstants.class, "IS_RUNNING_IN_IDE", true);
        }
        CommandSourceStack measured = scene.source.withSource(new Recording(feedback));
        int result = 0;
        try {
            official.commands.performCommand(
                    official.commands.getDispatcher().parse(command, measured), command);
            // `Commands.performCommand` 返回 void；官方的返回值是 `CommandSourceStack`
            // 里回调记下的成功次数，用反馈条数当可比的返回值。
            result = feedback.size();
        } catch (Throwable error) {
            feedback.add("throw:" + error.getClass().getSimpleName());
            if (System.getenv("ADV_ORACLE_DEBUG") != null) {
                error.printStackTrace(System.err);
            }
            result = -1;
        }
        @SuppressWarnings("unchecked")
        var progress = (java.util.Map<net.minecraft.advancements.AdvancementHolder,
                net.minecraft.advancements.AdvancementProgress>) field(state, "progress", Object.class);
        List<String> done = new ArrayList<>();
        for (var entry : progress.entrySet()) {
            if (entry.getValue().isDone()) {
                done.add(entry.getKey().id().toString());
            }
        }
        java.util.Collections.sort(done);
        out.write("{\"type\":\"advancement\",\"input\":" + quote(line)
                + ",\"result\":" + result
                + ",\"feedback\":" + quote(String.join("|", feedback))
                + ",\"done\":" + quote(String.join(",", done)) + "}\n");
    }

    private static void emit(Writer out, Official official, String line) throws Exception {
        Scene scene = new Scene(official, false);
        // ` ;; ` 隔开的最后一段是被比较的命令，前面每一段都是准备命令。
        // The last ` ;; `-separated part is the measured command; every earlier part is setup.
        String[] parts = line.split(" ;; ");
        String command = parts[parts.length - 1];
        CommandSourceStack source = scene.source;
        for (int i = 0; i + 1 < parts.length; i++) {
            try {
                official.commands.performCommand(
                        official.commands.getDispatcher().parse(parts[i], source), parts[i]);
            } catch (Throwable ignored) {
            }
        }
        List<String> feedback = new ArrayList<>();
        CommandSourceStack measured = source.withSource(new Recording(feedback));
        if (DEBUG) {
            // 官方 `Commands.performCommand` 只在这个开关下把异常原因发给来源；只用来查假世界缺什么，
            // 开着时反馈会多一行，对拍结果不作数。
            // Official `Commands.performCommand` sends the exception text to the source only
            // under this switch. It is for finding what the fake world lacks; the extra
            // feedback line makes the comparison meaningless while it is on.
            setStatic(SharedConstants.class, "IS_RUNNING_IN_IDE", true);
        }
        try {
            official.commands.performCommand(
                    official.commands.getDispatcher().parse(command, measured), command);
        } catch (Throwable error) {
            feedback.add("L[throw:" + error.getClass().getSimpleName() + "]");
            if (DEBUG) {
                error.printStackTrace(RAW_ERR);
            }
        }
        if (DEBUG) {
            RAW_ERR.println("java feedback: " + line + " => " + feedback);
        }
        int ran = feedback.stream().anyMatch(text -> text.contains("commands.setblock.success")) ? 1 : 0;
        var info = scene.board.getPlayerScoreInfo(ScoreHolder.forNameOnly("#total"),
                scene.board.getObjective("result"));
        int score = info == null ? 0 : info.value();
        out.write("{\"type\":\"case\",\"input\":" + quote(line)
                + ",\"stone_4_6\":" + stone(scene, TEST_POS)
                + ",\"stone_cow_a\":" + stone(scene, new BlockPos(2, -60, 2))
                + ",\"stone_cow_b\":" + stone(scene, new BlockPos(6, -60, 6))
                + ",\"score\":" + score
                + ",\"ran\":" + ran
                + ",\"feedback\":" + hash(feedback)
                + ",\"effects\":" + effectState(scene)
                + "}\n");
    }

    /** 玩家 + 两头牛的 `activeEffects`：按实体 ID 和效果 ID 排序后逐字段哈希。 */
    private static int effectState(Scene scene) {
        var registry = scene.level.registryAccess().lookupOrThrow(Registries.MOB_EFFECT);
        StringBuilder all = new StringBuilder();
        // `tracked` 的顺序固定：玩家、牛 A、牛 B；两边都按这个顺序，
        // 不用实体 ID（多岛 / 全局分配器给牛的号两边本来就不同）。
        int index = 0;
        for (Entity entity : scene.tracked) {
            if (!(entity instanceof LivingEntity living)) continue;
            all.append(index++).append(':');
            List<MobEffectInstance> effects = new ArrayList<>(living.getActiveEffects());
            effects.sort(java.util.Comparator.comparingInt(
                    e -> registry.getId(e.getEffect().value())));
            for (MobEffectInstance e : effects) {
                all.append(registry.getId(e.getEffect().value())).append(',')
                        .append(e.getDuration()).append(',').append(e.getAmplifier())
                        .append(',').append(e.isAmbient() ? 1 : 0)
                        .append(',').append(e.isVisible() ? 1 : 0)
                        .append(',').append(e.showIcon() ? 1 : 0).append(';');
            }
            all.append('\n');
        }
        int hash = 0x811c9dc5;
        for (byte b : all.toString().getBytes(StandardCharsets.UTF_8)) {
            hash ^= b & 0xff;
            hash *= 0x01000193;
        }
        return hash;
    }

    private static int stone(Scene scene, BlockPos pos) {
        BlockState state = scene.blocks.get(pos.asLong());
        return state != null && state.is(Blocks.STONE) ? 1 : 0;
    }

    /** 递归展开 `Component`：翻译键 + 参数，字面文本，子组件；不查语言表。 */
    private static String decompose(Component component) {
        StringBuilder out = new StringBuilder();
        decomposeInto(component, out);
        return out.toString();
    }

    private static void decomposeInto(Component component, StringBuilder out) {
        var contents = component.getContents();
        if (contents instanceof net.minecraft.network.chat.contents.TranslatableContents translated) {
            String key = translated.getKey();
            out.append("T[").append(key).append(']');
            Object[] args = translated.getArgs();
            // Bamboo 的 `Component.translate` 用 NBT 列表存参数，混合类型时
            // `unify_elements` 会把数字转成 `{text:"n"}`；这里对混合参数做
            // 同样的归一化，纯数字参数（如 `commands.setblock.success`）仍按 S[]。
            boolean hasComponent = false;
            boolean hasPlain = false;
            for (Object arg : args) {
                if (arg instanceof Component || arg instanceof net.minecraft.network.chat.contents.PlainTextContents) {
                    hasComponent = true;
                } else {
                    hasPlain = true;
                }
            }
            boolean mixed = hasComponent && hasPlain;
            for (Object arg : args) {
                out.append('{');
                if (arg instanceof Component nested) {
                    decomposeInto(nested, out);
                } else if (arg instanceof net.minecraft.network.chat.contents.PlainTextContents plain) {
                    out.append("L[").append(plain.text()).append(']');
                } else if (arg instanceof String || mixed
                        || key.equals("arguments.nbtpath.nothing_found")) {
                    // Bamboo 的翻译参数里字符串一律是文本组件，数字才是原样的数。
                    // In Bamboo a string translation argument is always a text component;
                    // only numbers stay raw.
                    out.append("L[").append(String.valueOf(arg)).append(']');
                } else {
                    out.append("S[").append(String.valueOf(arg)).append(']');
                }
                out.append('}');
            }
        } else if (contents instanceof net.minecraft.network.chat.contents.PlainTextContents literal) {
            out.append("L[").append(literal.text()).append(']');
        } else {
            out.append("C[").append(contents.getClass().getSimpleName()).append(']');
        }
        for (Component sibling : component.getSiblings()) {
            out.append('+');
            decomposeInto(sibling, out);
        }
    }

    private static int hash(List<String> lines) {
        // 输入是逐条反馈的「翻译键 + 参数 + 子组件」展开，Rust 侧按同样格式展开。
        int hash = 0x811c9dc5;
        for (String line : lines) {
            for (byte b : line.getBytes(StandardCharsets.UTF_8)) {
                hash ^= b & 0xff;
                hash *= 0x01000193;
            }
            hash ^= '\n';
            hash *= 0x01000193;
        }
        return hash;
    }

    /** 一个玩家、两头牛、一个空的方块表和一个计分板目标。 */
    private static final class Scene {
        final Long2ObjectOpenHashMap<BlockState> blocks = new Long2ObjectOpenHashMap<>();
        final List<Entity> tracked = new ArrayList<>();
        final ServerScoreboard board;
        final CommandSourceStack source;
        final ServerLevel level;
        final net.minecraft.server.level.ServerPlayer player;

        /**
         * `forAdvancements` 为真时给假玩家补一份全是 INT 槽的 `SynchedEntityData`，只够
         * `AdvancementRewards.grant` 读显示名。普通执行题不能装它：`effect give` 会往
         * 实体数据里写药水粒子列表这类非 INT 的值，类型对不上就会让整条命令失败。
         *
         * When `forAdvancements` is true the fake player gets an all-INT `SynchedEntityData`,
         * just enough for `AdvancementRewards.grant` to read the display name. The plain
         * execute cases must not install it: `effect give` writes non-INT values such as
         * the effect particle list, and the type mismatch fails the whole command.
         */
        Scene(Official official, boolean forAdvancements) throws Exception {
            level = allocate(FakeLevel.class);
            FakeLevel fake = (FakeLevel) level;
            fake.blocks = blocks;
            fake.tracked = tracked;
            fake.nextId = 1;
            set(Level.class, level, "dimensionTypeRegistration", official.overworld);
            set(Level.class, level, "dimension", Level.OVERWORLD);
            set(Level.class, level, "registryAccess", official.registries);
            set(Level.class, level, "palettedContainerFactory",
                    PalettedContainerFactory.create(official.registries));
            set(Level.class, level, "thread", Thread.currentThread());
            set(Level.class, level, "random", net.minecraft.util.RandomSource.create(1L));
            set(Level.class, level, "soundSeedGenerator", net.minecraft.util.RandomSource.create(1L));
            set(Level.class, level, "neighborUpdater",
                    new net.minecraft.world.level.redstone.CollectingNeighborUpdater(level, 1_000_000));
            // 官方 `SetBlockCommand` 在非 strict 模式还会更新邻居；这里只保留方块写入，
            // 不跑邻居更新（假世界没有邻域方块表）。
            set(ServerLevel.class, level, "server", official.server);
            set(ServerLevel.class, level, "players", new ArrayList<ServerPlayer>());
            board = new ServerScoreboard(official.server);
            official.server.sharedScoreboard = board;
            official.server.currentLevel = level;
            var saved = UNSAFE.allocateInstance(net.minecraft.world.level.storage.SavedDataStorage.class);
            set(net.minecraft.world.level.storage.SavedDataStorage.class, saved, "cache",
                    new java.util.HashMap<>());
            official.server.commandStorage =
                    new net.minecraft.world.level.storage.CommandStorage(
                            (net.minecraft.world.level.storage.SavedDataStorage) saved);
            board.addObjective("result", ObjectiveCriteria.DUMMY, Component.literal("result"),
                    ObjectiveCriteria.RenderType.INTEGER, false, null);
            board.getOrCreatePlayerScore(ScoreHolder.forNameOnly("#total"),
                    board.getObjective("result")).set(0);
            fake.board = board;
            var chunkMap = allocate(net.minecraft.server.level.ChunkMap.class);
            set(net.minecraft.server.level.ChunkMap.class, chunkMap, "entityMap",
                    new it.unimi.dsi.fastutil.ints.Int2ObjectOpenHashMap<>());
            FakeChunkCache chunkCache = allocate(FakeChunkCache.class);
            chunkCache.owner = fake;
            fake.chunkSource = chunkCache;
            set(net.minecraft.server.level.ServerChunkCache.class, fake.chunkSource, "chunkMap", chunkMap);
            set(ServerLevel.class, level, "chunkSource", fake.chunkSource);

            ServerPlayer player = allocate(ServerPlayer.class);
            this.player = player;
            set(net.minecraft.server.level.ServerPlayer.class, player, "server", official.server);
            // `allocate(ServerPlayer)` 不跑构造函数；`/effect` 的
            // `onEffectAdded` 会用到这些字段，逐个补最小初始化。
            var advancements = UNSAFE.allocateInstance(net.minecraft.server.PlayerAdvancements.class);
            set(net.minecraft.server.PlayerAdvancements.class, advancements, "activeTriggers",
                    new java.util.HashMap<>());
            set(ServerPlayer.class, player, "advancements", advancements);
            set(LivingEntity.class, player, "activeEffects", new java.util.HashMap<>());
            set(LivingEntity.class, player, "attributes",
                    new net.minecraft.world.entity.ai.attributes.AttributeMap(
                            LivingEntity.createLivingAttributes().build()));
            set(Entity.class, player, "type", EntityTypes.PLAYER);
            // `onEffectAdded` 会遍历 `getPassengers()` 把效果发给乘客，不能是 null。
            // `onEffectAdded` walks `getPassengers()` to send the effect to riders; it must not be null.
            set(Entity.class, player, "passengers", com.google.common.collect.ImmutableList.of());
            // `AdvancementRewards.grant` 走到 `ServerPlayer.getDisplayName` 时会读实体数据；
            // 补一个带空表的 `SynchedEntityData`。只在进度题里装，见构造函数说明。
            if (forAdvancements) {
            var dataClass = Class.forName("net.minecraft.network.syncher.SynchedEntityData$DataItem");
            var arrayType = java.lang.reflect.Array.newInstance(dataClass, 0).getClass();
            // 玩家实体数据的槽位从 0 开始，`ServerPlayer.getDisplayName` 读 18 号；
            // 造足够长的数组，每项都带一个该下标的 accessor 和 null 值。
            Object[] items = new Object[32];
            for (int i = 0; i < items.length; i++) {
                var accessor = new net.minecraft.network.syncher.EntityDataAccessor<>(i,
                        net.minecraft.network.syncher.EntityDataSerializers.INT);
                items[i] = dataClass
                        .getDeclaredConstructor(net.minecraft.network.syncher.EntityDataAccessor.class,
                                Object.class)
                        .newInstance(accessor, 0);
            }
            Object dataItems = java.lang.reflect.Array.newInstance(dataClass, items.length);
            for (int i = 0; i < items.length; i++) {
                java.lang.reflect.Array.set(dataItems, i, items[i]);
            }
            var dataCtor = net.minecraft.network.syncher.SynchedEntityData.class
                    .getDeclaredConstructor(net.minecraft.network.syncher.SyncedDataHolder.class,
                            arrayType);
            dataCtor.setAccessible(true);
            set(Entity.class, player, "entityData", dataCtor.newInstance(player, dataItems));
            }

            set(net.minecraft.world.entity.player.Player.class, player, "gameProfile",
                    new com.mojang.authlib.GameProfile(
                            java.util.UUID.fromString("00000000-0000-0000-0000-000000000001"),
                            "Kitten"));
            var connection = allocate(SilentListener.class);
            set(ServerGamePacketListenerImpl.class, connection, "player", player);
            set(ServerCommonPacketListenerImpl.class, connection, "server", official.server);
            set(ServerCommonPacketListenerImpl.class, connection, "connection",
                    new Connection(PacketFlow.CLIENTBOUND));
            set(ServerPlayer.class, player, "connection", connection);
            set(net.minecraft.world.entity.player.Player.class, player, "inventory",
                    new net.minecraft.world.entity.player.Inventory(player, new net.minecraft.world.entity.EntityEquipment()));
            var inventoryMenu = UNSAFE.allocateInstance(
                    net.minecraft.world.inventory.InventoryMenu.class);
            set(net.minecraft.world.entity.player.Player.class, player, "inventoryMenu",
                    inventoryMenu);
            set(net.minecraft.world.entity.player.Player.class, player, "containerMenu",
                    inventoryMenu);
            // `PlayerPredicate` 读饥饿值和游戏模式；Rust 侧的玩家是创造模式。
            // `PlayerPredicate` reads food and game mode; the Rust-side player is in creative.
            set(net.minecraft.world.entity.player.Player.class, player, "foodData",
                    new net.minecraft.world.food.FoodData());
            var gameMode = UNSAFE.allocateInstance(
                    net.minecraft.server.level.ServerPlayerGameMode.class);
            set(net.minecraft.server.level.ServerPlayerGameMode.class, gameMode,
                    "gameModeForPlayer", net.minecraft.world.level.GameType.CREATIVE);
            set(ServerPlayer.class, player, "gameMode", gameMode);
            set(Entity.class, player, "id", 1);
            set(Entity.class, player, "level", level);
            set(Entity.class, player, "position", new Vec3(8.5, -60.0, 8.5));
            player.setUUID(java.util.UUID.fromString("00000000-0000-0000-0000-000000000001"));
            level.players().add(player);
            tracked.add(player);
            for (int i = 0; i < 2; i++) {
                Cow cow = new Cow(EntityTypes.COW, level);
                cow.setId((1 << 24) + i);
                cow.setPos(i == 0 ? 2.5 : 6.5, -60.0, i == 0 ? 2.5 : 6.5);
                tracked.add(cow);
            }
            source = new CommandSourceStack(
                    CommandSource.NULL,
                    new Vec3(8.5, -60.0, 8.5),
                    Vec2.ZERO,
                    level,
                    LevelBasedPermissionSet.GAMEMASTER,
                    official.server,
                    player);
        }
    }

    /** `/effect` 发包用的空连接：`send` 什么都不做。 */
    private static final class SilentListener extends ServerGamePacketListenerImpl {
        private SilentListener() {
            super(null, null, null, null);
        }

        @Override
        public void send(Packet<?> packet) {}

        @Override
        public void send(Packet<?> packet, io.netty.channel.ChannelFutureListener listener) {}
    }

    private static final class Recording implements CommandSource {
        private final List<String> lines;

        Recording(List<String> lines) {
            this.lines = lines;
        }

        @Override
        public void sendSystemMessage(Component message) {
            lines.add(decompose(message));
        }

        @Override
        public boolean acceptsSuccess() {
            return true;
        }

        @Override
        public boolean acceptsFailure() {
            return true;
        }

        @Override
        public boolean shouldInformAdmins() {
            return false;
        }
    }

    private static final class Official {
        final net.minecraft.core.RegistryAccess registries;
        final Holder<DimensionType> overworld;
        final FakeServer server;
        final Commands commands;
        final GameRules rules = new GameRules(FeatureFlags.DEFAULT_FLAGS);

        private Official(ReloadableServerResources managers) {
            this.registries = (net.minecraft.core.RegistryAccess) managers.fullRegistries().lookup();
            this.overworld = registries.lookupOrThrow(Registries.DIMENSION_TYPE)
                    .getOrThrow(BuiltinDimensionTypes.OVERWORLD);
            this.server = allocate(FakeServer.class);
            set(FakeServer.class, server, "managers", managers);
            set(FakeServer.class, server, "rules", rules);
            this.commands = managers.getCommands();
        }

        static Official load(CloseableResourceManager resources) throws Exception {
            LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
            List<Registry.PendingTags<?>> staticTags =
                    TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
            HolderLookup.Provider context = initial.getAccessForLoading(RegistryLayer.WORLD);
            List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(
                    (net.minecraft.core.RegistryAccess.Frozen) context, staticTags);
            net.minecraft.core.RegistryAccess.Frozen world = RegistryDataLoader
                    .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool())
                    .join();
            LayeredRegistryAccess<RegistryLayer> layers = initial.replaceFrom(RegistryLayer.WORLD, world);
            ReloadableServerResources managers = ReloadableServerResources.loadResources(
                            resources, layers, staticTags, FeatureFlags.DEFAULT_FLAGS,
                            Commands.CommandSelection.DEDICATED, LevelBasedPermissionSet.GAMEMASTER,
                            ForkJoinPool.commonPool(), Runnable::run)
                    .get();
            managers.updateComponentsAndStaticRegistryTags();
            managers.getRecipeManager().finalizeRecipeLoading(FeatureFlags.DEFAULT_FLAGS);
            return new Official(managers);
        }
    }

    private static final class FakeServer extends DedicatedServer {
        ReloadableServerResources managers;
        GameRules rules;
        ServerScoreboard sharedScoreboard;
        ServerLevel currentLevel;
        net.minecraft.world.level.storage.CommandStorage commandStorage;
        net.minecraft.world.level.storage.WorldData worldData;

        private FakeServer() {
            super(null, null, null, null, null, null, null, null, null, null);
        }

        @Override
        public GameRules getGameRules() {
            return rules;
        }

        @Override
        public ReloadableServerRegistries.Holder reloadableRegistries() {
            return managers.fullRegistries();
        }

        @Override
        public net.minecraft.server.ServerAdvancementManager getAdvancements() {
            return managers.getAdvancements();
        }

        @Override
        public net.minecraft.world.item.crafting.RecipeManager getRecipeManager() {
            return managers.getRecipeManager();
        }

        @Override
        public ServerScoreboard getScoreboard() {
            return sharedScoreboard;
        }

        @Override
        public Iterable<ServerLevel> getAllLevels() {
            return List.of(currentLevel);
        }

        @Override
        public net.minecraft.core.RegistryAccess.Frozen registryAccess() {
            return (net.minecraft.core.RegistryAccess.Frozen) managers.fullRegistries().lookup();
        }

        @Override
        public net.minecraft.world.level.storage.CommandStorage getCommandStorage() {
            return commandStorage;
        }

        @Override
        public net.minecraft.world.level.storage.WorldData getWorldData() {
            return null;
        }

        /** 只有主世界；别的名字官方 `DimensionArgument` 报 `argument.dimension.invalid`。 */
        @Override
        public ServerLevel getLevel(net.minecraft.resources.ResourceKey<Level> dimension) {
            return dimension == Level.OVERWORLD ? currentLevel : null;
        }

        /** 一张空的秒表表：`if stopwatch` 查不到就报不存在。 */
        @Override
        public net.minecraft.world.Stopwatches getStopwatches() {
            try {
                var constructor = net.minecraft.world.Stopwatches.class.getDeclaredConstructor();
                constructor.setAccessible(true);
                return constructor.newInstance();
            } catch (ReflectiveOperationException error) {
                throw new IllegalStateException(error);
            }
        }

        @Override
        public void sendSystemMessage(Component message) {
        }

        /** `ServerPlayer` 构造函数问强制游戏模式；假世界没有 server.properties。 */
        @Override
        public net.minecraft.world.level.GameType getForcedGameType() {
            return null;
        }
    }

    /** `if loaded` 和 `Level.isLoaded` 要问的区块源：区块都在，状态是 ENTITY_TICKING。 */
    private static final class FakeChunkCache extends net.minecraft.server.level.ServerChunkCache {
        FakeLevel owner;

        private FakeChunkCache() {
            super(null, null, null, null, null, null, 0, 0, false, null);
        }

        @Override
        public net.minecraft.world.level.chunk.LevelChunk getChunkNow(int x, int z) {
            return owner.getChunk(x, z);
        }

        @Override
        public boolean hasChunk(int x, int z) {
            return true;
        }
    }

    private static final class FakeLevel extends ServerLevel {
        Long2ObjectOpenHashMap<BlockState> blocks;
        List<Entity> tracked;
        ServerScoreboard board;
        net.minecraft.server.level.ServerChunkCache chunkSource;
        // 这个类是 Unsafe 分配的，字段初始化不会跑，用到时再建。
        // The class is Unsafe-allocated, so field initializers never run; build it on first use.
        it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap<
                net.minecraft.world.level.chunk.ChunkAccess> chunkCache;
        int nextId = 1;
        // Unsafe 分配的实例不跑字段初始化器，第一次用到时再建。
        // Unsafe-allocated instances skip field initializers; build on first use.
        LevelData fixedLevelData;

        private FakeLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false);
        }

        @Override
        public BlockState getBlockState(BlockPos pos) {
            return blocks.getOrDefault(pos.asLong(), Blocks.AIR.defaultBlockState());
        }

        @Override
        public boolean setBlock(BlockPos pos, BlockState state, int flags, int recursion) {
            // 官方 `Level.setBlock`：状态没变返回 false（`/setblock` 据此报 `commands.setblock.failed`）。
            // Official `Level.setBlock` returns false when the state does not change, which is
            // what makes `/setblock` report `commands.setblock.failed`.
            BlockState previous = blocks.put(pos.asLong(), state);
            return state != (previous == null ? Blocks.AIR.defaultBlockState() : previous);
        }

        @Override
        protected LevelEntityGetter<Entity> getEntities() {
            return new Tracked(tracked);
        }

        @Override
        public boolean hasChunk(int x, int z) {
            return true;
        }

        @Override
        public net.minecraft.world.flag.FeatureFlagSet enabledFeatures() {
            return FeatureFlags.DEFAULT_FLAGS;
        }

        @Override
        public net.minecraft.world.level.chunk.LevelChunk getChunk(int x, int z) {
            return (net.minecraft.world.level.chunk.LevelChunk) getChunk(x, z,
                    net.minecraft.world.level.chunk.status.ChunkStatus.FULL, false);
        }

        @Override
        public net.minecraft.world.level.chunk.ChunkAccess getChunk(int x, int z,
                net.minecraft.world.level.chunk.status.ChunkStatus status, boolean load) {
            if (chunkCache == null) {
                chunkCache = new it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap<>();
            }
            long key = net.minecraft.world.level.ChunkPos.pack(x, z);
            net.minecraft.world.level.chunk.ChunkAccess cached = chunkCache.get(key);
            if (cached != null) {
                return cached;
            }
            net.minecraft.world.level.chunk.LevelChunk chunk =
                    new net.minecraft.world.level.chunk.LevelChunk(
                            this, new net.minecraft.world.level.ChunkPos(x, z));
            chunk.setFullStatus(() -> net.minecraft.server.level.FullChunkStatus.ENTITY_TICKING);
            chunkCache.put(key, chunk);
            return chunk;
        }

        @Override
        public LevelData getLevelData() {
            if (fixedLevelData == null) {
                fixedLevelData = new FixedLevelData();
            }
            return fixedLevelData;
        }

        /** 假世界是一整片平原，和 Rust 侧的超平坦世界一样。 */
        @Override
        public Holder<net.minecraft.world.level.biome.Biome> getBiome(BlockPos pos) {
            return registryAccess().lookupOrThrow(Registries.BIOME)
                    .getOrThrow(net.minecraft.world.level.biome.Biomes.PLAINS);
        }

        /** 方块存在假世界自己的表里，流体状态从那里的方块状态取。 */
        @Override
        public net.minecraft.world.level.material.FluidState getFluidState(BlockPos pos) {
            return getBlockState(pos).getFluidState();
        }

        @Override
        public boolean areEntitiesLoaded(long chunkKey) {
            return true;
        }

        // `execute summon` 走 `SummonCommand.createEntity`：假世界没有实体管理器、
        // 随机源和维度类型，补齐这三处让召唤本身能跑完，从而对拍重定向逻辑。
        // `execute summon` uses `SummonCommand.createEntity`: the fake world has no
        // entity manager, random source or dimension type, so stub these three to let
        // the spawn finish and compare the redirect behaviour.
        @Override
        public RandomSource getRandom() {
            return RandomSource.create();
        }

        @Override
        public DifficultyInstance getCurrentDifficultyAt(BlockPos pos) {
            return new DifficultyInstance(Difficulty.NORMAL, 0L, 0L, 0.0F);
        }

        @Override
        public boolean tryAddFreshEntityWithPassengers(Entity entity) {
            tracked.add(entity);
            return true;
        }

        @Override
        public ServerScoreboard getScoreboard() {
            return board;
        }
    }

    private static final class Tracked implements LevelEntityGetter<Entity> {
        private final List<Entity> entities;

        Tracked(List<Entity> entities) {
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
        public Entity get(java.util.UUID id) {
            for (Entity entity : entities) {
                if (entity.getUUID().equals(id)) {
                    return entity;
                }
            }
            return null;
        }

        @Override
        public <U extends Entity> void get(
                net.minecraft.world.level.entity.EntityTypeTest<Entity, U> test,
                AbortableIterationConsumer<U> consumer) {
            for (Entity entity : entities) {
                U cast = test.tryCast(entity);
                if (cast != null && consumer.accept(cast).shouldAbort()) {
                    return;
                }
            }
        }

        @Override
        public void get(net.minecraft.world.phys.AABB box, java.util.function.Consumer<Entity> output) {
            for (Entity entity : entities) {
                if (entity.getBoundingBox().intersects(box)) {
                    output.accept(entity);
                }
            }
        }

        @Override
        public <U extends Entity> void get(
                net.minecraft.world.level.entity.EntityTypeTest<Entity, U> test,
                net.minecraft.world.phys.AABB box,
                AbortableIterationConsumer<U> consumer) {
            for (Entity entity : entities) {
                U cast = test.tryCast(entity);
                if (cast != null && entity.getBoundingBox().intersects(box)
                        && consumer.accept(cast).shouldAbort()) {
                    return;
                }
            }
        }

        @Override
        public Iterable<Entity> getAll() {
            return entities;
        }
    }

    private static final class FixedLevelData implements LevelData {
        @Override
        public LevelData.RespawnData getRespawnData() {
            return LevelData.RespawnData.of(Level.OVERWORLD, new BlockPos(8, 0, 8), 0.0F, 0.0F);
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

    private static void setStatic(Class<?> owner, String name, Object value) {
        try {
            Field field = owner.getDeclaredField(name);
            field.setAccessible(true);
            field.set(null, value);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static Object field(Object owner, String name, Class<?> type) {
        try {
            Field field = owner.getClass().getDeclaredField(name);
            field.setAccessible(true);
            return type.cast(field.get(owner));
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
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

    @SuppressWarnings("unchecked")
    private static <T> T allocate(Class<T> type) {
        try {
            return (T) UNSAFE.allocateInstance(type);
        } catch (InstantiationException error) {
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

    private static String quote(String text) {
        StringBuilder out = new StringBuilder("\"");
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            switch (c) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\n' -> out.append("\\n");
                default -> out.append(c);
            }
        }
        return out.append('"').toString();
    }
}
