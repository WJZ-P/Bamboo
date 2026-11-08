import com.google.common.collect.ImmutableList;
import com.mojang.authlib.GameProfile;
import it.unimi.dsi.fastutil.longs.Long2ObjectMap;
import it.unimi.dsi.fastutil.longs.Long2ObjectOpenHashMap;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collection;
import java.util.Collections;
import java.util.Comparator;
import java.util.Deque;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.OptionalInt;
import java.util.Random;
import java.util.Set;
import java.util.TreeMap;
import java.util.TreeSet;
import java.util.UUID;
import java.util.concurrent.ForkJoinPool;
import java.util.function.BiConsumer;
import java.util.function.Consumer;
import java.util.function.Function;
import java.util.function.Predicate;
import net.minecraft.SharedConstants;
import net.minecraft.advancements.CriterionTriggerInstance;
import net.minecraft.advancements.triggers.CriterionTrigger;
import net.minecraft.commands.Commands;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderSet;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.NonNullList;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.particles.ExplosionParticleInfo;
import net.minecraft.core.particles.ParticleOptions;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.network.chat.Component;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.game.ClientGamePacketListener;
import net.minecraft.network.syncher.EntityDataAccessor;
import net.minecraft.network.syncher.SynchedEntityData;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.PlayerAdvancements;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.ReloadableServerRegistries;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.dedicated.DedicatedPlayerList;
import net.minecraft.server.dedicated.DedicatedServer;
import net.minecraft.server.ServerScoreboard;
import net.minecraft.server.level.ServerChunkCache;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.level.ServerPlayerGameMode;
import net.minecraft.server.network.ServerGamePacketListenerImpl;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.server.permissions.PermissionSet;
import net.minecraft.server.players.NameAndId;

import net.minecraft.sounds.SoundEvent;
import net.minecraft.sounds.SoundSource;
import net.minecraft.stats.Stat;
import net.minecraft.tags.BlockTags;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.AbortableIterationConsumer;
import net.minecraft.util.RandomSource;
import net.minecraft.util.random.WeightedList;
import net.minecraft.world.Container;
import net.minecraft.world.Difficulty;
import net.minecraft.world.DifficultyInstance;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.InteractionResult;
import net.minecraft.world.MenuProvider;
import net.minecraft.world.attribute.EnvironmentAttributeSystem;
import net.minecraft.world.damagesource.DamageSource;
import net.minecraft.world.damagesource.DamageSources;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.Pose;
import net.minecraft.world.entity.PositionMoveRotation;
import net.minecraft.world.entity.Relative;
import net.minecraft.world.entity.ai.attributes.AttributeMap;
import net.minecraft.world.entity.ai.attributes.DefaultAttributes;
import net.minecraft.world.entity.boss.enderdragon.EnderDragonPart;
import net.minecraft.world.entity.player.Abilities;
import net.minecraft.world.entity.player.Inventory;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.entity.player.PlayerEquipment;
import net.minecraft.world.food.FoodData;
import net.minecraft.world.inventory.PlayerEnderChestContainer;
import net.minecraft.world.flag.FeatureFlagSet;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.BlockItem;
import net.minecraft.world.item.BucketItem;
import net.minecraft.world.item.GameMasterBlockItem;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemCooldowns;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.PlaceOnWaterBlockItem;
import net.minecraft.world.item.SolidBucketItem;
import net.minecraft.world.item.StandingAndWallBlockItem;
import net.minecraft.world.item.context.UseOnContext;
import net.minecraft.world.item.crafting.RecipeManager;
import net.minecraft.world.level.BlockEventData;
import net.minecraft.world.level.BlockGetter;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.ExplosionDamageCalculator;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LightLayer;
import net.minecraft.world.level.StructureManager;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.block.BaseEntityBlock;
import net.minecraft.world.level.block.BaseRailBlock;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.CakeBlock;
import net.minecraft.world.level.block.ChestBlock;
import net.minecraft.world.level.block.ChiseledBookShelfBlock;
import net.minecraft.world.level.block.ComposterBlock;
import net.minecraft.world.level.block.DirectionalBlock;
import net.minecraft.world.level.block.DispenserBlock;
import net.minecraft.world.level.block.EndPortalFrameBlock;
import net.minecraft.world.level.block.EntityBlock;
import net.minecraft.world.level.block.Fallable;
import net.minecraft.world.level.block.FlowerPotBlock;
import net.minecraft.world.level.block.HopperBlock;
import net.minecraft.world.level.block.HorizontalDirectionalBlock;
import net.minecraft.world.level.block.JukeboxBlock;
import net.minecraft.world.level.block.LecternBlock;
import net.minecraft.world.level.block.LiquidBlock;
import net.minecraft.world.level.block.VegetationBlock;
import net.minecraft.world.level.block.entity.BarrelBlockEntity;
import net.minecraft.world.level.block.entity.BaseContainerBlockEntity;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.entity.ChestBlockEntity;
import net.minecraft.world.level.block.entity.ChiseledBookShelfBlockEntity;
import net.minecraft.world.level.block.entity.DispenserBlockEntity;
import net.minecraft.world.level.block.entity.HopperBlockEntity;
import net.minecraft.world.level.block.entity.JukeboxBlockEntity;
import net.minecraft.world.level.block.entity.ShulkerBoxBlockEntity;
import net.minecraft.world.level.block.entity.LecternBlockEntity;
import net.minecraft.world.level.block.piston.PistonBaseBlock;
import net.minecraft.world.level.block.piston.PistonHeadBlock;
import net.minecraft.world.level.block.piston.PistonMovingBlockEntity;
import net.minecraft.world.level.block.state.BlockBehaviour;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BedPart;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;
import net.minecraft.world.level.block.state.properties.ChestType;
import net.minecraft.world.level.block.state.properties.ComparatorMode;
import net.minecraft.world.level.block.state.properties.DoubleBlockHalf;
import net.minecraft.world.level.block.state.properties.PistonType;
import net.minecraft.world.level.border.WorldBorder;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.ChunkGenerator;
import net.minecraft.world.level.chunk.LevelChunk;
import net.minecraft.world.level.chunk.LevelChunkSection;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.entity.EntityInLevelCallback;
import net.minecraft.world.level.entity.EntityTypeTest;
import net.minecraft.world.level.entity.LevelEntityGetter;
import net.minecraft.world.level.gameevent.GameEvent;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.PositionalRandomFactory;
import net.minecraft.world.level.levelgen.structure.Structure;
import net.minecraft.world.level.levelgen.structure.StructureStart;
import net.minecraft.world.level.material.Fluid;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.material.Fluids;
import net.minecraft.world.level.redstone.CollectingNeighborUpdater;
import net.minecraft.world.level.storage.LevelData;
import net.minecraft.world.level.storage.loot.LootTable;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.BlockHitResult;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.ticks.LevelChunkTicks;
import net.minecraft.world.ticks.LevelTicks;
import net.minecraft.world.ticks.ScheduledTick;
import sun.misc.Unsafe;

/**
 * 方块行为的 oracle：官方代码出题、给答案，tools/mc-bamboo-vanilla/src/oracle.rs 用 mc-bamboo-blocks 跑同样的题来比较。
 *
 * 和 Extractor 一样只在 JVM 里加载官方类，不启动服务端，不涉及 EULA。假世界 FakeLevel 继承 ServerLevel，假玩家 FakePlayer 继承
 * ServerPlayer，都用 Unsafe.allocateInstance 跳过构造函数。方块和物品的代码、ServerPlayerGameMode、CollectingNeighborUpdater、
 * 标签、配方、战利品表全是官方的，假的只有方块的存储和世界外的一切。
 *
 * 运行（工作目录和 classpath 与 Probe.java 相同，Java 25 或更新）：
 *
 *     java -cp <target/vanilla 下全部 jar> BlockOracle.java [--seed S] [--cases N] [--only X] [--avoid A,B] [--untick A,B] [--case I]
 *                 [--explain]
 *
 *     --seed S    默认 1。参数相同时两次运行的输出逐字节相同。
 *     --cases N   题目总数，默认 10000。题目在 place、use、destroy、tick、flow、item 之间按 3:3:1:1:1:1 轮转，方块和物品也是
 *                 轮转的，20 万题时每个方块物品至少放几十次，每个方块类至少几百题。
 *     --only X    只出和 X 有关的题。X 是方块 ID、物品 ID（minecraft: 可省略）或者方块类、物品类的简单名，继承链上的类都算，
 *                 比如 StairBlock、DoubleHighBlockItem。X 是不能放置的物品（flint_and_steel、ShearsItem）时只出 use 题，
 *                 拿着它点各种方块，其中大约一半是先试出来的、它会改的方块（斧子的原木、锄头的泥土）。
 *                 带 block_transformer 组件的物品有时副手拿盾，这时主手的右键是举盾。末影之眼点框架的题有一半
 *                 在一圈朝里的框架里，装上就可能打开末地传送门。
 *                 X 是用 use_item 的物品（桶、细雪桶、荷叶、青蛙卵）时还出看着各种方块用它的 item 题。
 *                 点的是活塞、活塞头或者移动活塞时，除了随机题还出几道固定的多刻活塞题，见第三节。
 *     --avoid A,B 这些类（方块类的简单名）的方块不进场景，手里也不拿会放下它们的物品，题目的主角不受影响。水永远可以出现：
 *                 第 4 章要求的场景里有水。新版本多了没移植的类时，用它单独检查一个类。
 *                 花盆和堆肥桶例外：站着用主手点中它们时可以拿着这些物品，这时点击一定被方块接住，轮不到物品放置。
 *     --untick A,B 这些类（简单名，继承链上的类都算）的计划刻不在 mc-bamboo-blocks 的范围里。flow 题往后跑的时候要执行
 *                 其中一个，这一题写成 skip，原因是 untick:<类名>。
 *     --case I    只跑第 I 题，用来重现一个失败的场景。--seed、--only、--avoid 和 --untick 要和原来相同。
 *                 显式活塞题的 id 从 1000000 开始，也能这样单独跑。
 *     --explain   每个 case 行多一个 text 字段，把场景和变化写成人能读的文字。它不属于格式，读取时忽略。
 *
 * JDK 26 启动时会警告修改了 final 字段，这是给假世界补字段造成的，没有影响。加 --enable-final-field-mutation=ALL-UNNAMED 能消除，
 * 这个选项 Java 25 不认识，所以启动命令里没有加。
 *
 * stdout 只有 JSON 行，大缓冲写出。日志和结尾的汇总都在 stderr：每个类每种题的数量、改变世界的题数、skip 的原因、异常、
 * 随机数调用点、没有放下过的类和原因。
 *
 *
 * 一、行的格式
 *
 * 一行一个 JSON 对象，type 区分四种行。第一行是 header，最后一行是 end，读不到 end 说明进程中途死了。
 *
 *     {"type":"header","version":"26.3","protocol":777,"seed":1,"cases":200000,"maxChainedUpdates":1000000,"gameTime":100000,"minY":-64,"maxY":319}
 *     {"type":"skip","id":7,"kind":"place","block":"minecraft:kelp","class":"KelpBlock","reason":"random:..."}
 *     {"type":"case","id":3,"block":"minecraft:oak_door","class":"DoorBlock","scene":[...],"action":{...},"outcome":{...}}
 *     {"type":"end","cases":199153,"skipped":847}
 *
 * case 行：
 *
 *     id        题号，从 0 开始，--case 用它。
 *     block     这道题针对的方块。放置题是物品放的主方块，比如 torch 的题 block 是 minecraft:torch，放下的可能是 wall_torch。
 *     class     block 的 getClass().getSimpleName()，按类汇总用它。
 *     scene     [[x,y,z,state],...] 题目开始时的世界。state 是全局方块状态 ID，和 mc_bamboo_data::blocks 的常量是同一套数字。
 *               列表以外的格子全是空气，y 超出 minY..=maxY 的格子是 void_air，超出世界的格子写不进去。顺序没有含义。
 *     contents  [[x,y,z,slot,item,count],...] 场景里容器方块的内容，只写非空的格子，按坐标和槽位排序。item 是物品的注册表 ID，
 *               物品是默认组件的。没写的槽是空的。只有箱子（含双箱的两半）、木桶、潜影盒、漏斗、发射器和投掷器会写。
 *     action    题目，见下面。
 *     outcome   官方的答案，见下面。
 *
 * action：
 *
 *     kind      "place"、"use"、"destroy"、"tick"、"flow"、"item"。flow 跑 pos 这一格的流体刻，再按 ServerLevel.tick 的
 *               顺序往后跑 steps 刻。item 是 use_item 包：ServerPlayerGameMode.useItem，物品自己从眼睛看出去找目标。
 *     main      主手物品的注册表 ID，空手是 "minecraft:air"。物品是默认组件的 ItemStack，数量 1。
 *     off       副手物品，同上。
 *     hand      "main" 或 "off"，这次动作用哪只手。destroy、tick 和 flow 永远是 "main"。
 *     itemClass 这只手里物品的 Java 类名，比如 BlockItem、ShearsItem、AirItem。
 *     itemUseOn 这只手里的物品不是方块物品，但是有自己的 useOn：类覆盖了 Item.useOn，或者有 block_transformer 组件（斧子去皮、
 *               锄头耕地这些，它们的类就是 Item）。方块自己不处理点击时结果来自物品，这部分不在 mc-bamboo-blocks 的范围里。
 *     pos       [x,y,z]。place、use 是点击的方块（BlockHitResult.getBlockPos），destroy、tick、flow 是目标方块，
 *               item 是出题时让玩家看着的方块，只是说明，真正的目标由视线决定。
 *     steps     只有 flow 有。第一个流体刻之后再跑多少刻，0 表示只跑那一个。
 *     face      只有 place、use 有。点击的面："down"、"up"、"north"、"south"、"west"、"east"。
 *     loc       只有 place、use 有。[x,y,z] 精确的点击位置（BlockHitResult.getLocation），double 的位，见第二节。
 *               它就是 pos + float 光标，和官方解码 use_item_on 包的算法相同，float 的光标用 (loc - pos) as f32 能还原。
 *     inside    只有 place、use 有。BlockHitResult.isInside。
 *     actor     [x,y,z] 玩家的脚，double 的位。
 *     yaw       玩家的 yRot，float 的位，已经在 [-180, 180) 里，这是服务端收到后 wrapDegrees 的结果。头的朝向和身体相同。
 *     pitch     玩家的 xRot，float 的位，在 [-90, 90] 里。
 *     sneak     蹲着，也就是 isSecondaryUseActive。
 *     box       [minX,minY,minZ,maxX,maxY,maxZ] 玩家的碰撞盒，double 的位。蹲下时是蹲姿的高度。
 *     others    [box,...] 场景里其他玩家的碰撞盒。玩家自己和他们都挡路：isUnobstructed 要和 box 和 others 里的每个盒子比。
 *     settle    动作做完之后再走多少个游戏刻，0 表示不走。每一刻先 gameTime += 1，再把到期的方块计划刻、流体计划刻、
 *               方块事件和移动活塞的方块实体按官方的顺序跑完，和 ServerLevel.tick 里 tickTime 之后那几步相同。
 *               中继器的延迟、火把烧坏、按钮弹回、活塞推方块要靠它才看得到。
 *
 * 官方比较盒子时走 Shapes.joinIsNotEmpty，重叠小于 1e-7 不算重叠。
 *
 * outcome：
 *
 *     result    place、use：useItemOn 返回的 InteractionResult，"success"（PREDICTED）、"success_server"（SERVER_ONLY）、
 *               "consume"（NONE），这三种都是 InteractionResult.Success，括号里是它的 swingSource。还有 "fail"、"pass"。
 *               "try_empty_hand" 只在 useItemOn 内部出现，理论上不会写出来，读到时照样比较。item 题是 useItem 的结果，写法相同。
 *               destroy：destroyBlock 返回 true 是 "destroyed"，false 是 "refused"（比如拿着剑、GameMaster 方块）。
 *               tick、flow：固定是 "ticked"。
 *     changes   [[x,y,z,state],...] 结束时和 scene 不同的每一格的最终状态，变成空气时 state 是 0。顺序没有含义。
 *     destroyed [[x,y,z,state,excluded],...] levelEvent(2001) 按发生顺序的列表。state 是事件的数据，也就是被破坏的方块状态。
 *               excluded 为 true 表示 source 是发起的玩家，官方广播时跳过他，对应 Effect::Destroyed 的 except_initiator。
 *     levelEvents [[type,x,y,z,data,excluded],...] 全部 levelEvent 按发生顺序的列表，包括 2001。excluded 的含义和 destroyed 相同。
 *     sounds    [[id,x,y,z,source,volume,pitch,excluded],...] playSeededSound 按发生顺序的列表。id 是 sound_event 注册表 ID，
 *               Holder.direct 的音效没有注册表 ID，写 -1。x、y、z 是 double 的位，source 是 SoundSource.ordinal()，
 *               volume、pitch 是 float 的位，excluded 表示 except 是玩家，官方广播时跳过他。
 *     ticks     [[x,y,z,"block"|"fluid",id,delay,priority],...] 动作和 settle 走完之后还排着的计划刻，按执行顺序
 *               （ScheduledTick.DRAIN_ORDER：触发时刻、优先级、排进来的次序），同一格同一类型重复的已经去掉，和
 *               LevelChunkTicks 的规则相同。id 是方块或流体的注册表 ID。先写方块的，再写流体的，各自按执行顺序。
 *               delay 相对结束时的游戏时间，flow 题的结束时间算上 steps，-1 表示延迟是随机的，见第四节。
 *               priority 是 TickPriority.getValue()，0 是 NORMAL。
 *     ran       [block id,...] settle 里真的运行过 tick 的方块的注册表 ID，去重，顺序没有含义。名单里有不在比较范围里的
 *               计划刻（比如方块下落）时，世界已经被它改过，重放的一方只能排除这道题。
 *     contents  [[x,y,z,slot,item,count],...] 结束时所有已建模容器的非空格子，写法同场景的 contents。箱子按两半分别列出，
 *               顺序是坐标再槽位。Bamboo 的容器内容、漏斗搬运和比较器读数就靠它比较。
 *     analogContent 动作或者 settle 改了方块实体的内容，旁边又有比较器读得到，而且那个方块实体不是已建模的容器（书架、
 *               箱子、木桶、潜影盒、漏斗、发射器、投掷器）。Bamboo 不保留那些方块实体的内容，比较器读到的模拟信号两边会
 *               不一样，重放的一方只能排除这道题。
 *     itemReached  place、use：方块没有吃掉这次点击，轮到了手里的物品（ServerPlayerGameMode 走到了 itemStack.useOn）。
 *               和 itemUseOn 同时为真时结果来自物品自己的 useOn，不在 mc-bamboo-blocks 的范围里。别的题永远是假。
 *     gained    [item id,...] 只有 item 题有内容：物品栏里新出现的物品。创造模式用空桶舀起东西时，装满的桶放进物品栏。
 *     spawned   [entity type id,...] 这次动作生成的实体，比如掉落物。只是记录，不用比较。
 *     rng       碰过随机数但是结果不依赖它，比如开门的音调。只是记录，不用比较。
 *
 * skip 行没有答案，表示这题只能排除。reason 的前缀：
 *
 *     random:<调用点>        换随机数结果就变，见第四节。
 *     random-loop:<调用点>   极端的随机数让官方代码取了十万次以上。
 *     random-positional:<调用点>  官方要一个按位置派生的随机源，目前没有出现过。
 *     explosion:<调用点>     走到了 level.explode，比如主世界用重生锚。
 *     worldgen:<调用点>      走到了世界生成，比如骨粉催熟树苗。
 *     impossible:<调用点>    残缺的多格结构让官方代码读了别的方块没有的属性，真实世界到不了这种状态。
 *     untick:<类名>          flow 题往后跑时要执行 --untick 里的类的计划刻。
 *     exception:<类名>       官方代码抛了别的异常。这是假世界的 bug，正常的运行里不该有，异常的栈写在 stderr。
 *
 *
 * 二、数字
 *
 * 整数是 JSON 数字。double 和 float 必须原样保留，写成 IEEE 754 位的十六进制字符串，高位在前：
 * double 固定 16 位，float 固定 8 位。不用十进制，serde_json 默认的浮点解析不保证往返。
 *
 *     f64::from_bits(u64::from_str_radix(text, 16)?)
 *     f32::from_bits(u32::from_str_radix(text, 16)?)
 *
 *
 * 三、假世界的设定
 *
 * 在 Rust 里重放时 Level 和 Actor 要和这些一致。
 *
 *     玩家      创造模式，没有任何权限，canUseGameMasterBlocks 是假，所以 GameMaster 方块放不了也破坏不了。没有在飞。
 *               交互距离是创造模式的 5 格。物品栏里只有两只手上的物品。
 *     世界      主世界，平原，白天，晴天，普通难度，默认游戏规则，世界边界是默认值。游戏时间从 header 的 gameTime 开始，
 *               settle 每走一刻加一，flow 题每往后跑一步也加一。
 *     流体形状  FlowingFluid.getShape 按流体状态缓存，第一次调用时的位置决定高度。启动时在上面没有同种流体的位置调用过，
 *               所以水源和岩浆源的形状是它们自己的高度 8/9。
 *     亮度      getRawBrightness、getBrightness 恒为 15。
 *     信号      官方真实的实现，方块状态里的 powered、power 真的会产生信号。
 *     方块实体  场景加载时新建，容器内容按 contents 填，别的为空。有三种方块的状态是内容的镜像，内容按状态补齐：
 *               chiseled_bookshelf 的书、jukebox 的唱片、lectern 的书。只有漏斗和移动活塞（moving_piston 上的
 *               PistonMovingBlockEntity）的 ticker 会跑：漏斗每刻按官方 HopperBlockEntity.pushItemsTick 搬物品，
 *               同一次 tick 里漏斗按登记顺序（坐标顺序）在前，活塞在后。别的方块实体都不 tick，所以阳光探测器不会
 *               随时间重算。
 *     界面      openMenu 什么也不做，所以打开木桶不会把 open 改成 true。
 *     掉落      Level.destroyBlock 的 dropResources 恒为假，不算掉落。Block.dropResources 直接调用时算，只进 spawned。
 *     实体      世界里只有场景的玩家。spawned 里的实体不进世界，不挡路。
 *     计划刻    场景开始时没有排队的计划刻。队列是官方的 LevelTicks。除了 flow 题，这次动作排的不运行。tick 题只运行
 *               目标方块自己的 BlockState.tick 一次。flow 题第一下直接运行目标格的 FluidState.tick，之后每往后走一步都
 *               按 ServerLevel.tick 的顺序跑：先跑到期的方块计划刻（ServerLevel.tickBlock），再跑流体的
 *               （ServerLevel.tickFluid），再跑方块事件，最后跑方块实体，每条队列每刻最多 65536 个。第一下那刻也一样跑
 *               方块事件和方块实体。settle 每刻也按同样的顺序完整跑一刻。
 *     方块事件  level.blockEvent 排进官方的顺序，settle 和 flow 每刻在计划刻之后、方块实体之前执行，同一项只排一次。
 *               动作期间排的也在第一刻执行，所以放下红石块活塞会在下一刻动。活塞的整个动画要走三刻：第一刻推方块并
 *               推进半格，第二刻再推进半格，第三刻把真正的方块放回去。
 *     活塞题    随机题之外另有几道固定的活塞题（推出、收回、粘性收回、粘液块分支、推不动、打掉火把），同样的场景和动作
 *               按 settle = 1..4 各出一遍，逐刻比较世界。它们排在随机题的号段后面，id 从 1000000 开始。 *     连锁      Level.setBlock 和 LevelChunk.setBlockState 按官方写的，包括 onPlace、affectNeighborsAfterRemoval、
 *               updateNeighborsAt、updateNeighbourShapes，邻居队列是真正的 CollectingNeighborUpdater，
 *               链长限制是 header 里的 maxChainedUpdates。
 *
 *
 * 四、随机数
 *
 * level.getRandom() 和 player.getRandom() 是同一个计数的随机源。一题没有碰过它时答案就是唯一的。碰过时换全取最小值、全取最大值、
 * 另一个种子再跑三遍：result、changes、destroyed、levelEvents、sounds、ran（不看顺序）、ticks（不看 delay，按格子和类型配对，随机的延迟会打乱执行顺序）
 * 有一项不同就写 skip，全部相同才写 case，rng 是真。只有 delay 不同时那一条写成 -1，比较时不看它。stderr 的汇总列出每个调用点改变和没有改变结果的次数。
 *
 *
 * 五、怎么重放一题
 *
 *     1. 按 scene 建世界，方块只写入，不触发任何连锁。
 *     2. place、use：use_item_on(actor, hand, BlockHit { pos, face, loc, inside })。两种题走同一个入口，区别只在出题时想考什么。
 *        destroy：destroy_block(actor, pos)。tick：直接运行 pos 这一格的方块的 tick。
 *        flow：运行 pos 这一格的流体刻，再把时钟一刻一刻往后拨 steps 次，每次跑到期的计划刻。item：use_item(actor, hand)。
 *     3. 再走 settle 刻：每刻先把时间加一，再按顺序跑完到期的方块和流体计划刻、方块事件、移动活塞的方块实体。
 *     4. 比较 result、changes、destroyed、ran、contents、analogContent、gained 和 ticks 里范围内的条目。
 *
 * 官方的 useItemOn 和 destroyBlock 是直接调用的。够距检查、方块高度检查、序列号在 ServerGamePacketListenerImpl 里，不在这里，
 * 所以 pos 可以在任何地方，loc 也不一定在玩家看得到的面上。世界最顶层的方块点上面（目标格已经出界）的情况是真的会发生的，题目里有。
 */
public final class BlockOracle {
    /** 假世界开始时的游戏时间，写在 header 里。settle 每走一刻加一。 */
    private static final long GAME_TIME = 100_000L;
    /** 官方 server.properties 的默认值。 */
    private static final int MAX_CHAINED_UPDATES = 1_000_000;
    private static final int FULL_BRIGHTNESS = 15;
    private static final int LEVEL_EVENT_BLOCK_DESTROYED = 2001;
    private static final int SEA_LEVEL = 63;
    private static final int OUTPUT_BUFFER = 4 << 20;
    private static final int MAX_RANDOM_DRAWS = 100_000;
    /** ServerLevel.tick 给每条计划刻队列的上限。 */
    private static final int MAX_TICKS_PER_TICK = 65_536;
    private static final Direction[] DIRECTIONS = Direction.values();
    private static final Direction[] HORIZONTAL = {Direction.NORTH, Direction.SOUTH, Direction.WEST, Direction.EAST};

    private static final Unsafe UNSAFE = loadUnsafe();
    private static final Map<String, Field> FIELDS = new HashMap<>();

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(args, log);
        } catch (Exception error) {
            error.printStackTrace(log);
            // 官方的后台线程不是守护线程，不主动退出进程就不会结束。
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(String[] args, PrintStream log) throws Exception {
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        // 官方日志默认写 stdout。日志框架初始化时记下 System.out，要抢在这之前换掉，否则日志会混进 JSON。
        System.setOut(log);
        Options options = Options.parse(args);

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                        new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
                Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), OUTPUT_BUFFER)) {
            Official official = Official.load(resources);
            new Oracle(official, options, out, log).run();
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

    /** 不运行构造函数：真正的 ServerLevel 和 ServerPlayer 要一个完整的服务端才能构造。 */
    @SuppressWarnings("unchecked")
    private static <T> T allocate(Class<T> type) {
        try {
            return (T) UNSAFE.allocateInstance(type);
        } catch (InstantiationException error) {
            throw new IllegalStateException(error);
        }
    }

    private static Field field(Class<?> owner, String name) {
        return FIELDS.computeIfAbsent(owner.getName() + '#' + name, key -> {
            try {
                Field field = owner.getDeclaredField(name);
                field.setAccessible(true);
                return field;
            } catch (NoSuchFieldException error) {
                throw new IllegalStateException(error);
            }
        });
    }

    private static void set(Class<?> owner, Object target, String name, Object value) {
        try {
            field(owner, name).set(target, value);
        } catch (IllegalAccessException error) {
            throw new IllegalStateException(error);
        }
    }

    @SuppressWarnings("unchecked")
    private static <T> T read(Class<?> owner, Object target, String name) {
        try {
            return (T) field(owner, name).get(target);
        } catch (IllegalAccessException error) {
            throw new IllegalStateException(error);
        }
    }

    private static Method method(Class<?> owner, String name, Class<?>... parameters) {
        try {
            Method method = owner.getDeclaredMethod(name, parameters);
            method.setAccessible(true);
            return method;
        } catch (NoSuchMethodException error) {
            throw new IllegalStateException(error);
        }
    }

    /** 不带参数的私有方法，比如 ServerPlayer.updatePlayerAttributes。 */
    private static void call(Class<?> owner, Object target, String name) {
        try {
            method(owner, name).invoke(target);
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
    }

    /** type 或者它的父类的简单名在 names 里时返回这个名字。 */
    private static String ancestorIn(Class<?> type, Set<String> names) {
        for (Class<?> current = type; current != null; current = current.getSuperclass()) {
            if (names.contains(current.getSimpleName())) {
                return current.getSimpleName();
            }
        }
        return null;
    }

    private static <T> T pick(Random random, List<T> list) {
        return list.get(random.nextInt(list.size()));
    }

    private static boolean chance(Random random, int percent) {
        return random.nextInt(100) < percent;
    }

    private static String id(Block block) {
        return BuiltInRegistries.BLOCK.getKey(block).toString();
    }

    private static String id(Item item) {
        return BuiltInRegistries.ITEM.getKey(item).toString();
    }

    private static String className(Block block) {
        return block.getClass().getSimpleName();
    }

    private enum Kind {
        PLACE("place"),
        USE("use"),
        DESTROY("destroy"),
        TICK("tick"),
        FLOW("flow"),
        ITEM("item"),
        RANDOM("random"),
        CHUNK("chunk");

        private final String label;

        Kind(String label) {
            this.label = label;
        }
    }

    private record Options(long seed, int cases, String only, Set<String> avoid, Set<String> untick, Set<String> unrandom, int single,
            boolean explain) {
        private static final String USAGE = "usage: BlockOracle [--seed S] [--cases N] [--only <block id, item id or class>] [--avoid <classes>]"
                + " [--untick <classes>] [--unrandom <classes>] [--case I] [--explain]";

        static Options parse(String[] args) {
            long seed = 1;
            int cases = 10_000;
            String only = null;
            Set<String> avoid = Set.of();
            Set<String> untick = Set.of();
            Set<String> unrandom = Set.of();
            int single = -1;
            boolean explain = false;
            for (int i = 0; i < args.length; i++) {
                switch (args[i]) {
                    case "--seed" -> seed = Long.parseLong(value(args, ++i));
                    case "--cases" -> cases = Integer.parseInt(value(args, ++i));
                    case "--only" -> only = value(args, ++i);
                    case "--avoid" -> avoid = Set.of(value(args, ++i).split(","));
                    case "--untick" -> untick = Set.of(value(args, ++i).split(","));
                    case "--unrandom" -> unrandom = Set.of(value(args, ++i).split(","));
                    case "--case" -> single = Integer.parseInt(value(args, ++i));
                    case "--explain" -> explain = true;
                    default -> throw new IllegalArgumentException(USAGE);
                }
            }
            return new Options(seed, cases, only, avoid, untick, unrandom, single, explain);
        }

        private static String value(String[] args, int index) {
            if (index >= args.length) {
                throw new IllegalArgumentException(USAGE);
            }
            return args[index];
        }
    }

    /** 这个场景只能排除，不能当作答案。抛出后这一行写成 skip。 */
    private static final class SkipCase extends RuntimeException {
        private final String reason;

        SkipCase(String reason) {
            super(reason, null, false, false);
            this.reason = reason;
        }
    }

    /** 官方资源和假服务端，所有场景共用。 */
    private static final class Official {
        final RegistryAccess registries;
        final Holder<DimensionType> overworld;
        final Holder<Biome> plains;
        final BiomeManager biomes;
        final DamageSources damageSources;
        final FakeServer server;
        final GameRules gameRules = new GameRules(FeatureFlags.DEFAULT_FLAGS);
        final WorldBorder border = new WorldBorder();
        final RandomSource soundSeeds = RandomSource.create(0);
        final GameProfile profile = new GameProfile(new UUID(0, 1), "Oracle");
        final PlayerAdvancements advancements = allocate(NoAdvancements.class);
        final ServerGamePacketListenerImpl connection = allocate(SilentConnection.class);
        final FakeChunkSource chunkSource = allocate(FakeChunkSource.class);
        final StructureManager structures = allocate(NoStructures.class);
        final ServerScoreboard scoreboard;
        EnvironmentAttributeSystem attributes;

        private Official(ReloadableServerResources managers) {
            this.registries = (RegistryAccess) managers.fullRegistries().lookup();
            this.overworld = registries.lookupOrThrow(Registries.DIMENSION_TYPE).getOrThrow(BuiltinDimensionTypes.OVERWORLD);
            this.plains = registries.lookupOrThrow(Registries.BIOME).getOrThrow(Biomes.PLAINS);
            this.biomes = new BiomeManager((x, y, z) -> plains, 0L);
            this.damageSources = new DamageSources(registries);
            this.server = FakeServer.create(managers, gameRules);
            this.scoreboard = new ServerScoreboard(server);
        }

        /** 和 WorldLoader.load 相同的流程，只是不加载维度：标签、物品组件、配方、战利品表都是真的。 */
        static Official load(CloseableResourceManager resources) throws Exception {
            LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
            List<Registry.PendingTags<?>> staticTags =
                    TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
            RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
            List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
            RegistryAccess.Frozen world = RegistryDataLoader
                    .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES, ForkJoinPool.commonPool())
                    .join();
            LayeredRegistryAccess<RegistryLayer> layers = initial.replaceFrom(RegistryLayer.WORLD, world);
            ReloadableServerResources managers = ReloadableServerResources
                    .loadResources(resources, layers, staticTags, FeatureFlags.DEFAULT_FLAGS,
                            Commands.CommandSelection.DEDICATED, LevelBasedPermissionSet.GAMEMASTER,
                            ForkJoinPool.commonPool(), Runnable::run)
                    .get();
            managers.updateComponentsAndStaticRegistryTags();
            // MinecraftServer 的构造函数做这一步。不做的话营火的 RecipePropertySet.CAMPFIRE_INPUT 是空的。
            managers.getRecipeManager().finalizeRecipeLoading(FeatureFlags.DEFAULT_FLAGS);

            Official official = new Official(managers);
            FakeLevel sample = FakeLevel.create(official, new OracleRandom(OracleRandom.Mode.LOW, 0));
            // 只有维度和生物群系这两层。时间线和天气依赖时钟，假世界永远是白天的晴天。
            official.attributes = EnvironmentAttributeSystem.builder().addStaticLayers(sample).build();
            return official;
        }
    }

    private static final class FakeServer extends DedicatedServer {
        private ReloadableServerResources managers;
        private GameRules rules;
        private DedicatedPlayerList players;

        private FakeServer() {
            super(null, null, null, null, null, null, null, null, null, null);
        }

        static FakeServer create(ReloadableServerResources managers, GameRules rules) {
            FakeServer server = allocate(FakeServer.class);
            server.managers = managers;
            server.rules = rules;
            server.players = allocate(FakePlayerList.class);
            return server;
        }

        @Override
        public RecipeManager getRecipeManager() {
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

        @Override
        public boolean isUnderSpawnProtection(ServerLevel level, BlockPos pos, Player player) {
            return false;
        }

        /** 只有掉落物的战利品表用它，掉落物不进答案。 */
        @Override
        public RandomSource getRandomSequence(Identifier key) {
            return RandomSource.create(0L);
        }

        @Override
        public DedicatedPlayerList getPlayerList() {
            return players;
        }
    }

    /**
     * 战利品表都是空的，世界里没有掉落物，和 Bamboo 一样。掉落时官方每一组物品都从世界的随机源取 5 个数
     * （位置的偏移和速度），组数又取决于战利品表的随机序列，留着它们，后面的随机数就和 Bamboo 对不上了。
     */
    private static final class NoLoot extends ReloadableServerRegistries.Holder {
        NoLoot(HolderLookup.Provider registries) {
            super(registries);
        }

        @Override
        public LootTable getLootTable(ResourceKey<LootTable> id) {
            return LootTable.EMPTY;
        }
    }

    private static final class FakePlayerList extends DedicatedPlayerList {
        private FakePlayerList() {
            super(null, null, null);
        }

        @Override
        public boolean isOp(NameAndId nameAndId) {
            return false;
        }
    }

    /** 走到世界生成或者要真正的区块时排除，这些都在 Bamboo 的范围外。 */
    private static final class FakeChunkSource extends ServerChunkCache {
        private FakeChunkSource() {
            super(null, null, null, null, null, null, 0, 0, false, null);
        }

        @Override
        public ChunkGenerator getGenerator() {
            throw new SkipCase("worldgen:" + OracleRandom.callSite());
        }

        @Override
        public void sendToTrackingPlayers(Entity entity, Packet<? super ClientGamePacketListener> packet) {
        }

        @Override
        public void sendToTrackingPlayersAndSelf(Entity entity, Packet<? super ClientGamePacketListener> packet) {
        }
    }

    /**
     * level.findBlocksIn 按区块段扫描，ServerLevel.tickChunk 按区块段挑随机刻的格子。这个区块只把段转给假世界，
     * 扫描和挑格子的顺序还是官方的。
     */
    private static final class FakeChunk extends LevelChunk {
        private FakeLevel level;
        private int chunkX;
        private int chunkZ;

        private FakeChunk() {
            super(null, null);
        }

        static FakeChunk of(FakeLevel level, int chunkX, int chunkZ) {
            FakeChunk chunk = allocate(FakeChunk.class);
            chunk.level = level;
            chunk.chunkX = chunkX;
            chunk.chunkZ = chunkZ;
            return chunk;
        }

        @Override
        public int getSectionIndexFromSectionY(int sectionY) {
            return sectionY;
        }

        @Override
        public LevelChunkSection getSection(int sectionY) {
            return FakeSection.of(level, new BlockPos(chunkX << 4, sectionY << 4, chunkZ << 4));
        }

        /** tickChunk 按下标遍历，下标 0 是世界最低的区块段。 */
        @Override
        public LevelChunkSection[] getSections() {
            LevelChunkSection[] sections = new LevelChunkSection[getSectionsCount()];
            for (int index = 0; index < sections.length; index++) {
                sections[index] = getSection(getSectionYFromSectionIndex(index));
            }
            return sections;
        }

        @Override
        public ChunkPos getPos() {
            return new ChunkPos(chunkX, chunkZ);
        }

        @Override
        public int getMinY() {
            return level.getMinY();
        }

        @Override
        public int getHeight() {
            return level.getHeight();
        }

        /** ChunkAccess.getHeight：这一列最高的、符合高度图条件的方块，没有时是世界底部下面一格。 */
        @Override
        public int getHeight(Heightmap.Types type, int x, int z) {
            for (int y = level.getMaxY(); y >= level.getMinY(); y--) {
                if (type.isOpaque().test(level.getBlockState(new BlockPos((chunkX << 4) + x, y, (chunkZ << 4) + z)))) {
                    return y;
                }
            }
            return level.getMinY() - 1;
        }

        @Override
        public BlockState getBlockState(BlockPos pos) {
            return level.getBlockState(pos);
        }

        @Override
        public FluidState getFluidState(BlockPos pos) {
            return level.getFluidState(pos);
        }

        @Override
        public void markPosForPostProcessing(BlockPos pos) {
        }
    }

    private static final class FakeSection extends LevelChunkSection {
        private FakeLevel level;
        private BlockPos origin;

        private FakeSection() {
            super(null, null);
        }

        static FakeSection of(FakeLevel level, BlockPos origin) {
            FakeSection section = allocate(FakeSection.class);
            section.level = level;
            section.origin = origin;
            return section;
        }

        @Override
        public boolean maybeHas(Predicate<BlockState> predicate) {
            return true;
        }

        @Override
        public BlockState getBlockState(int sectionX, int sectionY, int sectionZ) {
            BlockState state = level.getBlockState(origin.offset(sectionX, sectionY, sectionZ));
            level.checkRandomTick(state);
            return state;
        }

        /** LevelChunkSection.isRandomlyTicking：tickingBlockCount 或者 tickingFluidCount 不是 0。 */
        @Override
        public boolean isRandomlyTicking() {
            for (Long2ObjectMap.Entry<BlockState> entry : level.blocks.long2ObjectEntrySet()) {
                BlockPos pos = BlockPos.of(entry.getLongKey());
                BlockState state = entry.getValue();
                boolean inside = pos.getX() >> 4 == origin.getX() >> 4 && pos.getY() >> 4 == origin.getY() >> 4
                        && pos.getZ() >> 4 == origin.getZ() >> 4;
                if (inside && (state.isRandomlyTicking() || state.getFluidState().isRandomlyTicking())) {
                    return true;
                }
            }
            return false;
        }
    }

    /** 假世界里没有结构。 */
    private static final class NoStructures extends StructureManager {
        private NoStructures() {
            super(null, null, null);
        }

        @Override
        public StructureStart getStructureWithPieceAt(BlockPos pos, HolderSet<Structure> structures) {
            return StructureStart.INVALID_START;
        }

        @Override
        public StructureStart getStructureWithPieceAt(BlockPos pos, Predicate<Holder<Structure>> predicate) {
            return StructureStart.INVALID_START;
        }
    }

    /** 发给客户端的包都丢掉：告示牌编辑界面、传送、方块更新。 */
    private static final class SilentConnection extends ServerGamePacketListenerImpl {
        private SilentConnection() {
            super(null, null, null, null);
        }

        @Override
        public void send(Packet<?> packet) {
        }

        @Override
        public void teleport(PositionMoveRotation destination, Set<Relative> relatives) {
        }
    }

    /** 没有任何进度在监听，官方的触发器看到空表就直接返回。 */
    private static final class NoAdvancements extends PlayerAdvancements {
        private NoAdvancements() {
            super(null, null, null, null, null);
        }

        @Override
        public <T extends CriterionTriggerInstance> Map<PlayerAdvancements.TriggerInstanceKey, T> getTriggerMapForType(CriterionTrigger<T> type) {
            return null;
        }
    }

    /** 除了游戏时间都是固定的。时间从 {@link #GAME_TIME} 开始，settle 每走一刻调用一次 {@link #advance}。 */
    private static final class FixedLevelData implements LevelData {
        private long gameTime = GAME_TIME;

        /** ServerLevel.tickTime 里的 levelData.setGameTime(gameTime + 1)。 */
        void advance() {
            gameTime++;
        }

        @Override
        public LevelData.RespawnData getRespawnData() {
            return LevelData.RespawnData.DEFAULT;
        }

        @Override
        public long getGameTime() {
            return gameTime;
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

    /**
     * 记录每一次取随机数。假世界的 level.getRandom() 是一个，玩家的 getRandom() 是另一个。
     *
     * <p>SEEDED 就是官方 ServerLevel 用的 LegacyRandomSource，mc-bamboo-blocks 用同样的种子逐位重现它，所以世界的随机源
     * 不用排除，直接比较。玩家自己的随机源 Bamboo 没有，它碰过的场景会换几种随机数重跑，世界的结果变了才排除。
     * LOW 和 HIGH 是两个极端，能抓住 nextInt(5) == 0 这种小概率分支。
     */
    private static final class OracleRandom implements RandomSource {
        private static final StackWalker WALKER = StackWalker.getInstance();
        private static final int MAX_SITES = 8;
        private static final Set<String> PLUMBING = Set.of("net.minecraft.server.level.ServerLevel", "net.minecraft.world.level.Level",
                "net.minecraft.world.level.LevelReader", "net.minecraft.world.level.LevelAccessor");

        enum Mode {
            SEEDED,
            LOW,
            HIGH
        }

        private final Mode mode;
        private final RandomSource seeded;
        private final Set<String> sites = new LinkedHashSet<>();
        private int draws;

        OracleRandom(Mode mode, long seed) {
            this(mode, new LegacyRandomSource(seed));
        }

        private OracleRandom(Mode mode, RandomSource seeded) {
            this.mode = mode;
            this.seeded = seeded;
        }

        private void touch() {
            if (++draws > MAX_RANDOM_DRAWS) {
                throw new SkipCase("random-loop:" + callSite());
            }
            if (sites.size() < MAX_SITES) {
                sites.add(callSite());
            }
        }

        private static boolean isCaller(String name) {
            return name.startsWith("net.minecraft.") && !name.startsWith("net.minecraft.util.") && !PLUMBING.contains(name);
        }

        private static String callSite() {
            return WALKER.walk(frames -> frames
                    .filter(frame -> isCaller(frame.getClassName()))
                    .findFirst()
                    .map(frame -> {
                        String name = frame.getClassName();
                        return name.substring(name.lastIndexOf('.') + 1) + "." + frame.getMethodName();
                    })
                    .orElse("unknown"));
        }

        /** SEEDED 和官方一样取一个 long 做新源的种子，新源的取数照样记在这里。 */
        @Override
        public RandomSource fork() {
            touch();
            return mode == Mode.SEEDED ? new OracleRandom(mode, seeded.fork()) : this;
        }

        @Override
        public PositionalRandomFactory forkPositional() {
            throw new SkipCase("random-positional:" + callSite());
        }

        @Override
        public void setSeed(long seed) {
            touch();
            seeded.setSeed(seed);
        }

        @Override
        public int nextInt() {
            touch();
            return switch (mode) {
                case SEEDED -> seeded.nextInt();
                case LOW -> 0;
                case HIGH -> Integer.MAX_VALUE;
            };
        }

        @Override
        public int nextInt(int bound) {
            touch();
            return switch (mode) {
                case SEEDED -> seeded.nextInt(bound);
                case LOW -> 0;
                case HIGH -> bound - 1;
            };
        }

        @Override
        public long nextLong() {
            touch();
            return switch (mode) {
                case SEEDED -> seeded.nextLong();
                case LOW -> 0L;
                case HIGH -> Long.MAX_VALUE;
            };
        }

        @Override
        public boolean nextBoolean() {
            touch();
            return switch (mode) {
                case SEEDED -> seeded.nextBoolean();
                case LOW -> false;
                case HIGH -> true;
            };
        }

        @Override
        public float nextFloat() {
            touch();
            return switch (mode) {
                case SEEDED -> seeded.nextFloat();
                case LOW -> 0.0F;
                case HIGH -> Math.nextDown(1.0F);
            };
        }

        @Override
        public double nextDouble() {
            touch();
            return switch (mode) {
                case SEEDED -> seeded.nextDouble();
                case LOW -> 0.0;
                case HIGH -> Math.nextDown(1.0);
            };
        }

        @Override
        public double nextGaussian() {
            touch();
            return switch (mode) {
                case SEEDED -> seeded.nextGaussian();
                case LOW -> -3.0;
                case HIGH -> 3.0;
            };
        }
    }

    /**
     * 官方的 LevelTicks，区块的容器在第一次排刻时才建，这样哪里都排得进去。记下被接受的计划刻（同一格同一类型重复的会被
     * LevelChunkTicks 丢掉）。flow 题和 settle 用官方的 tick 方法真正跑它们。
     */
    private static final class RecordingTicks<T> extends LevelTicks<T> {
        private final Long2ObjectOpenHashMap<LevelChunkTicks<T>> containers = new Long2ObjectOpenHashMap<>();
        private final List<ScheduledTick<T>> accepted = new ArrayList<>();

        RecordingTicks() {
            super(chunk -> true);
        }

        @Override
        public void schedule(ScheduledTick<T> tick) {
            LevelChunkTicks<T> container = containerAt(tick.pos());
            int before = container.count();
            super.schedule(tick);
            if (container.count() > before) {
                accepted.add(tick);
            }
        }

        private LevelChunkTicks<T> containerAt(BlockPos pos) {
            return containers.computeIfAbsent(ChunkPos.pack(pos), key -> {
                LevelChunkTicks<T> container = new LevelChunkTicks<>();
                addContainer(ChunkPos.unpack(key), container);
                return container;
            });
        }

        /** 还在排队的计划刻，按官方取出来执行的顺序。 */
        List<ScheduledTick<T>> pending() {
            List<ScheduledTick<T>> pending = new ArrayList<>();
            containers.values().forEach(container -> container.getAll().forEach(pending::add));
            pending.sort(ScheduledTick.DRAIN_ORDER);
            return pending;
        }
    }

    private record Destroyed(BlockPos pos, int state, boolean initiatorExcluded) {
    }

    /**
     * 假世界。继承 ServerLevel 是为了让 level instanceof ServerLevel 的分支和真实服务端一样走。
     * 构造函数没有运行，这里覆盖掉所有会碰到空字段的方法。
     */
    private static final class FakeLevel extends ServerLevel {
        private static final Method TICK_BLOCK = method(ServerLevel.class, "tickBlock", BlockPos.class, Block.class);
        private static final Method TICK_FLUID = method(ServerLevel.class, "tickFluid", BlockPos.class, Fluid.class);

        private Official official;
        private Long2ObjectOpenHashMap<BlockState> blocks;
        private Long2ObjectOpenHashMap<BlockEntity> blockEntities;
        private LinkedHashSet<BlockEventData> blockEvents;
        /**
         * 移动方块实体的 ticker，按登记顺序。官方的完整机制是
         * {@code Level.tickBlockEntities} 加 {@code LevelChunk.tickersInLevel}，这里登记活塞和漏斗：
         * Bamboo 别的方块实体都不 tick。漏斗单独一张表，每刻先跑，和 Bamboo 方块实体表里漏斗总在活塞前面一致；
         * 表里按坐标顺序登记，见 {@link #load}。
         */
        private LinkedHashMap<BlockPos, BlockEntity> tickers;
        private LinkedHashMap<BlockPos, HopperBlockEntity> hopperTickers;
        /** tick 阶段里新登记的 ticker，下一刻开头并进上面两张表。 */
        private LinkedHashMap<BlockPos, BlockEntity> pendingTickers;
        private boolean tickingBlockEntities;
        private RecordingTicks<Block> recordedBlockTicks;
        private RecordingTicks<Fluid> recordedFluidTicks;
        private List<ServerPlayer> playersInWorld;
        private List<Destroyed> destroyed;
        private List<LevelPlay> levelEvents;
        private List<SoundPlay> sounds;
        private List<String> spawned;
        private Set<String> ranBlocks;
        private boolean analogContent;
        private FixedLevelData data;
        private LevelEntityGetter<Entity> entityGetter;
        private int lastEntityId = 100;
        private Set<String> unrandom;

        private FakeLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false);
        }

        static FakeLevel create(Official official, OracleRandom random) {
            FakeLevel level = allocate(FakeLevel.class);
            level.official = official;
            level.blocks = new Long2ObjectOpenHashMap<>();
            level.blockEntities = new Long2ObjectOpenHashMap<>();
            level.blockEvents = new LinkedHashSet<>();
            level.tickers = new LinkedHashMap<>();
            level.hopperTickers = new LinkedHashMap<>();
            level.pendingTickers = new LinkedHashMap<>();
            level.tickingBlockEntities = false;
            level.recordedBlockTicks = new RecordingTicks<>();
            level.recordedFluidTicks = new RecordingTicks<>();
            level.playersInWorld = new ArrayList<>();
            level.destroyed = new ArrayList<>();
            level.levelEvents = new ArrayList<>();
            level.sounds = new ArrayList<>();
            level.spawned = new ArrayList<>();
            level.ranBlocks = new LinkedHashSet<>();
            level.analogContent = false;
            level.data = new FixedLevelData();
            level.entityGetter = new PlayerEntities(level.playersInWorld);
            level.unrandom = Set.of();
            set(Level.class, level, "neighborUpdater", new CollectingNeighborUpdater(level, MAX_CHAINED_UPDATES));
            set(Level.class, level, "random", random);
            set(Level.class, level, "soundSeedGenerator", official.soundSeeds);
            set(Level.class, level, "dimensionTypeRegistration", official.overworld);
            set(Level.class, level, "dimension", Level.OVERWORLD);
            set(Level.class, level, "registryAccess", official.registries);
            set(Level.class, level, "damageSources", official.damageSources);
            set(ServerLevel.class, level, "players", new ArrayList<ServerPlayer>());
            // BucketItem 经过 mayInteract，它直接读这个字段问出生点保护。
            set(ServerLevel.class, level, "server", official.server);
            return level;
        }

        /** 直接写入场景，不触发任何连锁。方块实体和区块加载后一样已经存在。 */
        void load(Long2ObjectMap<BlockState> scene) {
            load(scene, new Long2ObjectOpenHashMap<>());
        }

        /**
         * 见 {@link #load(Long2ObjectMap)}，容器内容按坐标顺序填，箱子两半分开填。
         *
         * <p>内容直接写进方块实体的槽位表，不走 {@code setItem}：官方从 NBT 读区块时也不会因为填内容去通知比较器，
         * 场景加载和重放两边都看不到这一下。
         */
        void load(Long2ObjectMap<BlockState> scene, Long2ObjectMap<NonNullList<ItemStack>> contents) {
            blocks.putAll(scene);
            long[] keys = scene.keySet().toLongArray();
            Arrays.sort(keys);
            for (long key : keys) {
                BlockState state = scene.get(key);
                if (state.hasBlockEntity()) {
                    BlockEntity entity = getBlockEntity(BlockPos.of(key));
                    if (entity != null) {
                        syncContents(entity, state);
                    }
                }
            }
            long[] filled = contents.keySet().toLongArray();
            Arrays.sort(filled);
            for (long key : filled) {
                BlockEntity entity = getBlockEntity(BlockPos.of(key));
                if (!(entity instanceof Container container)) {
                    continue;
                }
                NonNullList<ItemStack> stored = storedItems((BaseContainerBlockEntity) entity);
                NonNullList<ItemStack> stacks = contents.get(key);
                for (int slot = 0; slot < Math.min(container.getContainerSize(), stacks.size()); slot++) {
                    stored.set(slot, stacks.get(slot).copy());
                }
            }
        }

        /** 容器方块的槽位表。子类各自声明 `items` 字段，调父类的 `getItems` 统一拿。 */
        @SuppressWarnings("unchecked")
        private static NonNullList<ItemStack> storedItems(BaseContainerBlockEntity entity) {
            try {
                return (NonNullList<ItemStack>) method(BaseContainerBlockEntity.class, "getItems").invoke(entity);
            } catch (ReflectiveOperationException error) {
                throw new IllegalStateException(error);
            }
        }

        /** 已建模的容器：内容两边都有，比较器读数和漏斗搬运都能比。 */
        private static boolean modelsContents(BlockEntity entity) {
            return entity instanceof ChiseledBookShelfBlockEntity
                    || entity instanceof ChestBlockEntity
                    || entity instanceof BarrelBlockEntity
                    || entity instanceof ShulkerBoxBlockEntity
                    || entity instanceof HopperBlockEntity
                    || entity instanceof DispenserBlockEntity;
        }

        /** 这几种方块的状态是内容的镜像。场景里只有状态，内容要补成一致，否则右键的结果是真实世界到不了的。 */
        private static void syncContents(BlockEntity entity, BlockState state) {
            switch (entity) {
                case ChiseledBookShelfBlockEntity shelf -> {
                    NonNullList<ItemStack> items = read(ChiseledBookShelfBlockEntity.class, shelf, "items");
                    for (int slot = 0; slot < items.size(); slot++) {
                        if (state.getValue(ChiseledBookShelfBlock.SLOT_OCCUPIED_PROPERTIES.get(slot))) {
                            items.set(slot, new ItemStack(Items.BOOK));
                        }
                    }
                }
                case JukeboxBlockEntity jukebox -> {
                    if (state.getValue(JukeboxBlock.HAS_RECORD)) {
                        set(JukeboxBlockEntity.class, jukebox, "item", new ItemStack(Items.MUSIC_DISC_13));
                    }
                }
                case LecternBlockEntity lectern -> {
                    if (state.getValue(LecternBlock.HAS_BOOK)) {
                        set(LecternBlockEntity.class, lectern, "book", new ItemStack(Items.WRITABLE_BOOK));
                    }
                }
                default -> {
                }
            }
        }

        void addPlayer(FakePlayer player) {
            playersInWorld.add(player);
        }

        /**
         * 再走 ticks 个游戏刻，照 ServerLevel.tick 的顺序：先 tickTime 把时间加一，再跑到期的方块计划刻，
         * 然后跑到期的流体计划刻，再跑方块事件，最后跑方块实体。
         *
         * <p>实体的刻不跑。handlingTick 只在计划刻和方块事件期间为真，跑方块实体之前已经放回假，和官方相同。
         */
        void settle(int ticks) {
            for (int i = 0; i < ticks; i++) {
                data.advance();
                long now = getGameTime();
                set(ServerLevel.class, this, "handlingTick", true);
                recordedBlockTicks.tick(now, MAX_TICKS_PER_TICK, this::tickBlock);
                recordedFluidTicks.tick(now, MAX_TICKS_PER_TICK, (at, fluid) -> invoke(TICK_FLUID, at, fluid));
                runBlockEvents();
                set(ServerLevel.class, this, "handlingTick", false);
                tickRegisteredTickers();
            }
        }

        /**
         * ServerLevel.runBlockEvents：按排进来的顺序执行，这一格已经换成别的方块时丢掉。
         *
         * <p>官方在这里还会给客户端发 ClientboundBlockEventPacket，假世界里没客户端。区块没加载时往后挪的
         * 那份重排也不做，假世界的区块都算加载过。
         */
        private void runBlockEvents() {
            withPlayersHidden(() -> {
                while (!blockEvents.isEmpty()) {
                    BlockEventData eventData = blockEvents.iterator().next();
                    blockEvents.remove(eventData);
                    BlockState state = getBlockState(eventData.pos());
                    if (state.is(eventData.block())) {
                        state.triggerEvent(this, eventData.pos(), eventData.paramA(), eventData.paramB());
                    }
                }
            });
        }

        /**
         * 方块事件和方块实体的 tick 里，实体查询看不到玩家。
         *
         * <p>Bamboo 不模拟实体：活塞推实体、敲钟惊动附近的实体都不在范围里。假玩家跳过了构造函数，被推或者被钟
         * 读到没初始化的字段会抛空指针。世界里的方块变化不受影响。
         */
        private void withPlayersHidden(Runnable body) {
            List<ServerPlayer> hidden = new ArrayList<>(playersInWorld);
            playersInWorld.clear();
            try {
                body.run();
            } finally {
                playersInWorld.addAll(hidden);
            }
        }

        /**
         * Level.tickBlockEntities：先并进上一刻登记的新 ticker，再按顺序跑。官方的 wrapper 每下都重读这一格的
         * 状态，类型对不上就不跑。
         *
         * <p>官方遍历的是列表，删掉的 wrapper 要等下一次 tick 才回收；这里也等到循环结束再删，顺序一致。
         */
        private void tickRegisteredTickers() {
            tickingBlockEntities = true;
            pendingTickers.forEach((pos, entity) -> {
                if (entity instanceof HopperBlockEntity hopper) {
                    hopperTickers.put(pos, hopper);
                } else {
                    tickers.put(pos, entity);
                }
            });
            pendingTickers.clear();
            try {
                withPlayersHidden(() -> {
                    for (Map.Entry<BlockPos, HopperBlockEntity> entry : new ArrayList<>(hopperTickers.entrySet())) {
                        BlockPos pos = entry.getKey();
                        HopperBlockEntity entity = entry.getValue();
                        if (!entity.isRemoved()) {
                            BlockState state = getBlockState(pos);
                            if (entity.getType().isValid(state)) {
                                HopperBlockEntity.pushItemsTick(this, pos, state, entity);
                            }
                        }
                    }
                    for (Map.Entry<BlockPos, BlockEntity> entry : new ArrayList<>(tickers.entrySet())) {
                        BlockPos pos = entry.getKey();
                        if (entry.getValue() instanceof PistonMovingBlockEntity piston && !piston.isRemoved()) {
                            BlockState state = getBlockState(pos);
                            if (piston.getType().isValid(state)) {
                                PistonMovingBlockEntity.tick(this, pos, state, piston);
                            }
                        }
                    }
                });
                hopperTickers.values().removeIf(BlockEntity::isRemoved);
                tickers.values().removeIf(BlockEntity::isRemoved);
            } finally {
                tickingBlockEntities = false;
            }
        }

        /** LevelChunk.updateBlockEntityTicker：登记移动活塞和漏斗，见 {@link #tickers}。 */
        private void registerTicker(BlockEntity blockEntity) {
            boolean registered = blockEntity instanceof PistonMovingBlockEntity piston
                            && getBlockState(piston.getBlockPos()).is(Blocks.MOVING_PISTON)
                    || blockEntity instanceof HopperBlockEntity hopper
                            && getBlockState(hopper.getBlockPos()).is(Blocks.HOPPER);
            if (!registered) {
                return;
            }
            if (tickingBlockEntities) {
                pendingTickers.put(blockEntity.getBlockPos(), blockEntity);
            } else if (blockEntity instanceof HopperBlockEntity hopper) {
                hopperTickers.put(hopper.getBlockPos(), hopper);
            } else {
                tickers.put(blockEntity.getBlockPos(), blockEntity);
            }
        }

        /** ServerLevel.tickBlock。到期时这一格换成了别的方块，这个刻就不执行。 */
        private void tickBlock(BlockPos pos, Block block) {
            BlockState state = getBlockState(pos);
            if (state.is(block)) {
                ranBlocks.add(id(block));
                state.tick(this, pos, getRandom());
            }
        }

        /**
         * flow 题：先跑这一格的流体刻，再按官方 ServerLevel.tick 的顺序往后跑 steps 刻，每刻方块的计划刻在前，
         * 流体的在后，然后是方块事件和方块实体。动作自己那一格也一样，流体刻之后跑方块事件和方块实体。
         * 方块的计划刻要是落在 untick 里的类上（下落、红石这些范围外的刻），这一题只能排除。
         */
        void flow(BlockPos pos, int steps, Set<String> untick) {
            set(ServerLevel.class, this, "handlingTick", true);
            getFluidState(pos).tick(this, pos, getBlockState(pos));
            runBlockEvents();
            set(ServerLevel.class, this, "handlingTick", false);
            tickRegisteredTickers();
            for (int step = 1; step <= steps; step++) {
                data.gameTime = GAME_TIME + step;
                long now = getGameTime();
                set(ServerLevel.class, this, "handlingTick", true);
                recordedBlockTicks.tick(now, MAX_TICKS_PER_TICK, (at, block) -> runBlockTick(at, block, untick));
                recordedFluidTicks.tick(now, MAX_TICKS_PER_TICK, (at, fluid) -> invoke(TICK_FLUID, at, fluid));
                runBlockEvents();
                set(ServerLevel.class, this, "handlingTick", false);
                tickRegisteredTickers();
            }
        }

        /** Level.randValue：getBlockRandomPos 的线性同余，和 random 无关，种子见 Oracle.answer。 */
        void seedPositions(int value) {
            set(Level.class, this, "randValue", value);
        }

        /** random 题：ServerLevel.tickChunk 选中这一格时做的事。 */
        void randomTick(BlockPos pos) {
            BlockState state = getBlockState(pos);
            if (state.isRandomlyTicking()) {
                state.randomTick(this, pos, random);
            }
            FluidState fluid = state.getFluidState();
            if (fluid.isRandomlyTicking()) {
                fluid.randomTick(this, pos, random);
            }
        }

        /** chunk 题：官方的 tickChunk，碰到 unrandom 里的类的随机刻时这一题只能排除。 */
        void tickChunk(BlockPos pos, int speed, Set<String> unrandom) {
            this.unrandom = unrandom;
            try {
                tickChunk(FakeChunk.of(this, pos.getX() >> 4, pos.getZ() >> 4), speed);
            } finally {
                this.unrandom = Set.of();
            }
        }

        /** tickChunk 从区块段读到的状态。要做随机刻的状态落在 unrandom 里时排除这一题。 */
        void checkRandomTick(BlockState state) {
            if (!unrandom.isEmpty() && state.isRandomlyTicking()) {
                String outOfScope = ancestorIn(state.getBlock().getClass(), unrandom);
                if (outOfScope != null) {
                    throw new SkipCase("unrandom:" + outOfScope);
                }
            }
        }

        private void runBlockTick(BlockPos pos, Block block, Set<String> untick) {
            if (getBlockState(pos).is(block)) {
                String outOfScope = ancestorIn(block.getClass(), untick);
                if (outOfScope != null) {
                    throw new SkipCase("untick:" + outOfScope);
                }
            }
            invoke(TICK_BLOCK, pos, block);
        }

        /** ServerLevel 的私有方法，原样调用。 */
        private void invoke(Method method, Object... args) {
            try {
                method.invoke(this, args);
            } catch (InvocationTargetException error) {
                if (error.getCause() instanceof RuntimeException runtime) {
                    throw runtime;
                }
                throw new IllegalStateException(error.getCause());
            } catch (IllegalAccessException error) {
                throw new IllegalStateException(error);
            }
        }

        @Override
        public BlockState getBlockState(BlockPos pos) {
            if (!isInValidBounds(pos)) {
                return Blocks.VOID_AIR.defaultBlockState();
            }
            BlockState state = blocks.get(pos.asLong());
            return state != null ? state : Blocks.AIR.defaultBlockState();
        }

        @Override
        public FluidState getFluidState(BlockPos pos) {
            return isInValidBounds(pos) ? getBlockState(pos).getFluidState() : Fluids.EMPTY.defaultFluidState();
        }

        /** Level.setBlock。 */
        @Override
        public boolean setBlock(BlockPos pos, BlockState blockState, int updateFlags, int updateLimit) {
            if (!isInValidBounds(pos)) {
                return false;
            }
            Block block = blockState.getBlock();
            BlockState oldState = setBlockInChunk(pos, blockState, updateFlags);
            if (oldState == null) {
                return false;
            }
            if (getBlockState(pos) == blockState) {
                if ((updateFlags & 1) != 0) {
                    updateNeighborsAt(pos, oldState.getBlock());
                    if (blockState.hasAnalogOutputSignal()) {
                        updateNeighbourForOutputSignal(pos, block);
                    }
                }
                if ((updateFlags & 16) == 0 && updateLimit > 0) {
                    int neighbourUpdateFlags = updateFlags & -34;
                    oldState.updateIndirectNeighbourShapes(this, pos, neighbourUpdateFlags, updateLimit - 1);
                    blockState.updateNeighbourShapes(this, pos, neighbourUpdateFlags, updateLimit - 1);
                    blockState.updateIndirectNeighbourShapes(this, pos, neighbourUpdateFlags, updateLimit - 1);
                }
            }
            return true;
        }

        /** LevelChunk.setBlockState，去掉光照、高度图和区块段。 */
        private BlockState setBlockInChunk(BlockPos pos, BlockState state, int flags) {
            BlockState oldState = getBlockState(pos);
            if (oldState == state) {
                return null;
            }
            blocks.put(pos.asLong(), state);

            Block newBlock = state.getBlock();
            boolean blockChanged = !oldState.is(newBlock);
            boolean movedByPiston = (flags & 64) != 0;
            boolean sideEffects = (flags & 256) == 0;
            if (blockChanged && oldState.hasBlockEntity() && !state.shouldChangedStateKeepBlockEntity(oldState)) {
                if (sideEffects) {
                    BlockEntity blockEntity = getBlockEntity(pos);
                    if (blockEntity != null) {
                        blockEntity.preRemoveSideEffects(pos, oldState);
                    }
                }
                removeBlockEntity(pos);
            }
            if ((blockChanged || newBlock instanceof BaseRailBlock)
                    && ((flags & 1) != 0 || movedByPiston)) {
                oldState.affectNeighborsAfterRemoval(this, pos, movedByPiston);
            }
            if (!getBlockState(pos).is(newBlock)) {
                return null;
            }
            if ((flags & 512) == 0) {
                state.onPlace(this, pos, oldState, movedByPiston);
            }
            if (state.hasBlockEntity() && getBlockState(pos).is(newBlock)) {
                updateBlockEntity(pos, state, newBlock);
            }
            return oldState;
        }

        @SuppressWarnings("deprecation")
        private void updateBlockEntity(BlockPos pos, BlockState state, Block newBlock) {
            BlockEntity blockEntity = existingBlockEntity(pos);
            if (blockEntity != null && !blockEntity.isValidBlockState(state)) {
                removeBlockEntity(pos);
                blockEntity = null;
            }
            if (blockEntity == null) {
                blockEntity = ((EntityBlock) newBlock).newBlockEntity(pos.immutable(), state);
                if (blockEntity != null) {
                    setBlockEntity(blockEntity);
                }
            } else {
                blockEntity.setBlockState(state);
            }
        }

        private BlockEntity existingBlockEntity(BlockPos pos) {
            BlockEntity blockEntity = blockEntities.get(pos.asLong());
            if (blockEntity != null && blockEntity.isRemoved()) {
                blockEntities.remove(pos.asLong());
                return null;
            }
            return blockEntity;
        }

        /** 不算掉落。 */
        @Override
        public boolean destroyBlock(BlockPos pos, boolean dropResources, Entity breaker, int updateLimit) {
            return super.destroyBlock(pos, false, breaker, updateLimit);
        }

        /** Level.getBlockEntity，也就是 EntityCreationType.IMMEDIATE：没有时按当前状态建一个。 */
        @Override
        public BlockEntity getBlockEntity(BlockPos pos) {
            if (!isInValidBounds(pos)) {
                return null;
            }
            BlockEntity blockEntity = blockEntities.get(pos.asLong());
            if (blockEntity == null) {
                BlockState state = getBlockState(pos);
                if (state.hasBlockEntity()) {
                    blockEntity = ((EntityBlock) state.getBlock()).newBlockEntity(pos.immutable(), state);
                    if (blockEntity != null) {
                        setBlockEntity(blockEntity);
                    }
                }
                return blockEntity;
            }
            if (blockEntity.isRemoved()) {
                blockEntities.remove(pos.asLong());
                return null;
            }
            return blockEntity;
        }

        @Override
        @SuppressWarnings("deprecation")
        public void setBlockEntity(BlockEntity blockEntity) {
            BlockPos pos = blockEntity.getBlockPos();
            if (!isInValidBounds(pos)) {
                return;
            }
            BlockState state = getBlockState(pos);
            if (!state.hasBlockEntity()) {
                return;
            }
            if (state != blockEntity.getBlockState()) {
                if (!blockEntity.getType().isValid(state)) {
                    return;
                }
                blockEntity.setBlockState(state);
            }
            blockEntity.setLevel(this);
            blockEntity.clearRemoved();
            BlockEntity previous = blockEntities.put(pos.asLong(), blockEntity);
            if (previous != null && previous != blockEntity) {
                previous.setRemoved();
            }
            registerTicker(blockEntity);
        }

        @Override
        public void removeBlockEntity(BlockPos pos) {
            BlockEntity removed = blockEntities.remove(pos.asLong());
            if (removed != null) {
                removed.setRemoved();
            }
            tickers.remove(pos);
            hopperTickers.remove(pos);
            pendingTickers.remove(pos);
        }

        /**
         * BlockEntity.setChanged 先调这里，再让旁边的比较器重读模拟信号。
         *
         * <p>已建模容器的内容两边都有，比较器读到的数一样。只有书架以外的、Bamboo 没保留内容的方块实体改了内容
         * 又有比较器能读到，这道题的世界两边就不一样了。
         */
        @Override
        public void blockEntityChanged(BlockPos pos) {
            BlockEntity entity = existingBlockEntity(pos);
            if (entity != null && !modelsContents(entity) && readsAnalogOutput(pos)) {
                analogContent = true;
            }
        }

        /** Level.updateNeighbourForOutputSignal 找的是哪些比较器：隔着导电方块也读得到。 */
        private boolean readsAnalogOutput(BlockPos pos) {
            for (Direction direction : Direction.Plane.HORIZONTAL) {
                BlockPos relativePos = pos.relative(direction);
                BlockState state = getBlockState(relativePos);
                if (state.is(Blocks.COMPARATOR)) {
                    return true;
                }
                if (state.isRedstoneConductor(this, relativePos)
                        && getBlockState(relativePos.relative(direction)).is(Blocks.COMPARATOR)) {
                    return true;
                }
            }
            return false;
        }

        @Override
        public LevelTicks<Block> getBlockTicks() {
            return recordedBlockTicks;
        }

        @Override
        public LevelTicks<Fluid> getFluidTicks() {
            return recordedFluidTicks;
        }

        @Override
        public void levelEvent(Entity source, int type, BlockPos pos, int data) {
            levelEvents.add(new LevelPlay(type, pos.immutable(), data, source instanceof Player));
            if (type == LEVEL_EVENT_BLOCK_DESTROYED) {
                destroyed.add(new Destroyed(pos.immutable(), data, source instanceof Player));
            }
        }

        @Override
        public void globalLevelEvent(int type, BlockPos pos, int data) {
        }

        @Override
        public void gameEvent(Holder<GameEvent> gameEvent, Vec3 position, GameEvent.Context context) {
        }

        @Override
        public void sendBlockUpdated(BlockPos pos, BlockState old, BlockState current, int updateFlags) {
        }

        @Override
        public void updatePOIOnBlockStateChange(BlockPos pos, BlockState oldState, BlockState newState) {
        }

        @Override
        public void playSeededSound(Entity except, double x, double y, double z, Holder<SoundEvent> sound, SoundSource source,
                float volume, float pitch, long seed) {
            sounds.add(new SoundPlay(BuiltInRegistries.SOUND_EVENT.getId(sound.value()), x, y, z, source.ordinal(), volume, pitch,
                    except instanceof Player));
        }

        @Override
        public void playSeededSound(Entity except, Entity sourceEntity, Holder<SoundEvent> sound, SoundSource source, float volume,
                float pitch, long seed) {
            playSeededSound(except, sourceEntity.getX(), sourceEntity.getY(), sourceEntity.getZ(), sound, source, volume, pitch, seed);
        }

        /** ServerLevel.blockEvent：排进下一刻执行的队列，同一项只排一次。 */
        @Override
        public void blockEvent(BlockPos pos, Block block, int b0, int b1) {
            blockEvents.add(new BlockEventData(pos.immutable(), block, b0, b1));        }

        @Override
        public void destroyBlockProgress(int id, BlockPos blockPos, int progress) {
        }

        @Override
        public void broadcastEntityEvent(Entity entity, byte event) {
        }

        @Override
        public void broadcastDamageEvent(Entity entity, DamageSource source) {
        }

        @Override
        public void explode(Entity source, DamageSource damageSource, ExplosionDamageCalculator damageCalculator, double x, double y,
                double z, float r, boolean fire, Level.ExplosionInteraction interactionType, ParticleOptions smallExplosionParticles,
                ParticleOptions largeExplosionParticles, WeightedList<ExplosionParticleInfo> blockParticles,
                Holder<SoundEvent> explosionSound) {
            throw new SkipCase("explosion:" + OracleRandom.callSite());
        }

        @Override
        public boolean addFreshEntity(Entity entity) {
            spawned.add(EntityType.getKey(entity.getType()).toString());
            return true;
        }

        @Override
        public int getNextEntityId() {
            return ++lastEntityId;
        }

        @Override
        protected LevelEntityGetter<Entity> getEntities() {
            return entityGetter;
        }

        @Override
        public Collection<EnderDragonPart> dragonParts() {
            return List.of();
        }

        @Override
        public List<ServerPlayer> players() {
            return playersInWorld;
        }

        @Override
        public ServerChunkCache getChunkSource() {
            return official.chunkSource;
        }

        @Override
        public ChunkAccess getChunk(int chunkX, int chunkZ, ChunkStatus status, boolean loadOrGenerate) {
            return FakeChunk.of(this, chunkX, chunkZ);
        }

        @Override
        public StructureManager structureManager() {
            return official.structures;
        }

        @Override
        public DifficultyInstance getCurrentDifficultyAt(BlockPos pos) {
            return new DifficultyInstance(getDifficulty(), getGameTime(), 0L, 0.0F);
        }

        /** 场景里的玩家总在目标附近。 */
        @Override
        public boolean canSpreadFireAround(BlockPos pos) {
            return true;
        }

        @Override
        public ServerScoreboard getScoreboard() {
            return official.scoreboard;
        }

        @Override
        @SuppressWarnings("deprecation")
        public boolean hasChunk(int chunkX, int chunkZ) {
            return true;
        }

        @Override
        public boolean isLoaded(BlockPos pos) {
            return isInValidBounds(pos);
        }

        @Override
        public BlockGetter getChunkForCollisions(int chunkX, int chunkZ) {
            return this;
        }

        @Override
        public int getBrightness(LightLayer layer, BlockPos pos) {
            return FULL_BRIGHTNESS;
        }

        @Override
        public int getRawBrightness(BlockPos pos, int darkening) {
            return FULL_BRIGHTNESS;
        }

        @Override
        public LevelData getLevelData() {
            return data;
        }

        @Override
        public FeatureFlagSet enabledFeatures() {
            return FeatureFlags.DEFAULT_FLAGS;
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
        public DedicatedServer getServer() {
            return official.server;
        }

        @Override
        public EnvironmentAttributeSystem environmentAttributes() {
            return official.attributes;
        }

        @Override
        public BiomeManager getBiomeManager() {
            return official.biomes;
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
        public int getSeaLevel() {
            return SEA_LEVEL;
        }

        @Override
        public long getSeed() {
            return 0L;
        }

        @Override
        public RecipeManager recipeAccess() {
            return official.server.getRecipeManager();
        }
    }

    /** 世界里只有玩家这一种实体，查询直接遍历。 */
    private record PlayerEntities(List<ServerPlayer> players) implements LevelEntityGetter<Entity> {
        @Override
        public Entity get(int id) {
            return players.stream().filter(player -> player.getId() == id).findFirst().orElse(null);
        }

        @Override
        public Entity get(UUID id) {
            return players.stream().filter(player -> player.getUUID().equals(id)).findFirst().orElse(null);
        }

        @Override
        public Iterable<Entity> getAll() {
            return new ArrayList<>(players);
        }

        @Override
        public <U extends Entity> void get(EntityTypeTest<Entity, U> type, AbortableIterationConsumer<U> consumer) {
            for (ServerPlayer player : players) {
                U cast = type.tryCast(player);
                if (cast != null && consumer.accept(cast).shouldAbort()) {
                    return;
                }
            }
        }

        @Override
        public void get(AABB bb, Consumer<Entity> output) {
            for (ServerPlayer player : players) {
                if (player.getBoundingBox().intersects(bb)) {
                    output.accept(player);
                }
            }
        }

        @Override
        public <U extends Entity> void get(EntityTypeTest<Entity, U> type, AABB bb, AbortableIterationConsumer<U> consumer) {
            for (ServerPlayer player : players) {
                if (player.getBoundingBox().intersects(bb)) {
                    U cast = type.tryCast(player);
                    if (cast != null && consumer.accept(cast).shouldAbort()) {
                        return;
                    }
                }
            }
        }
    }

    /** 创造模式的官方 ServerPlayerGameMode，流程一行不改。 */
    private static final class CreativeGameMode extends ServerPlayerGameMode {
        CreativeGameMode(ServerPlayer player) {
            super(player);
            setGameModeForPlayer(GameType.CREATIVE, null);
        }
    }

    /** 假玩家。继承 ServerPlayer 的理由和 FakeLevel 一样。 */
    private static final class FakePlayer extends ServerPlayer {
        private static final StackWalker CALLER = StackWalker.getInstance(StackWalker.Option.RETAIN_CLASS_REFERENCE);

        private PlayerAdvancements advancements;
        private boolean reachedItem;

        private FakePlayer() {
            super(null, null, null, null);
        }

        static FakePlayer create(FakeLevel level, Official official, RandomSource random, int id, Vec3 position, float yaw, float pitch,
                boolean sneak) {
            FakePlayer player = allocate(FakePlayer.class);
            player.assembleEntity(level, official, random, id);
            player.assemblePlayer(official);
            // 真实的玩家每 tick 都做这一步，创造模式的交互距离因此是 5 格。
            call(ServerPlayer.class, player, "updatePlayerAttributes");
            player.moveTo(position, yaw, pitch, sneak);
            level.addPlayer(player);
            return player;
        }

        private void assembleEntity(FakeLevel level, Official official, RandomSource random, int id) {
            set(Entity.class, this, "type", EntityTypes.PLAYER);
            set(Entity.class, this, "level", level);
            set(Entity.class, this, "id", id);
            set(Entity.class, this, "random", random);
            set(Entity.class, this, "position", Vec3.ZERO);
            set(Entity.class, this, "blockPosition", BlockPos.ZERO);
            set(Entity.class, this, "chunkPosition", ChunkPos.ZERO);
            set(Entity.class, this, "deltaMovement", Vec3.ZERO);
            set(Entity.class, this, "levelCallback", EntityInLevelCallback.NULL);
            set(Entity.class, this, "passengers", ImmutableList.of());
            set(Entity.class, this, "dimensions", EntityTypes.PLAYER.getDimensions());
            set(Entity.class, this, "uuid", new UUID(0, id));
            set(Entity.class, this, "stringUUID", new UUID(0, id).toString());
            firstTick = true;
            blocksBuilding = true;
            set(LivingEntity.class, this, "attributes", new AttributeMap(DefaultAttributes.getSupplier(EntityTypes.PLAYER)));
            // 真实的 `LivingEntity` 在构造时就有大脑，敲钟时 `BellBlockEntity` 会往附近生物的大脑里写记忆。
            set(LivingEntity.class, this, "brain", new net.minecraft.world.entity.ai.Brain<>());
            set(LivingEntity.class, this, "activeEffects", new HashMap<>());
            set(Entity.class, this, "entityData", defaultEntityData());
            setHealth(getMaxHealth());
        }

        private SynchedEntityData defaultEntityData() {
            SynchedEntityData.Builder builder = new SynchedEntityData.Builder(this);
            builder.define(DATA_SHARED_FLAGS_ID, (byte) 0);
            builder.define(accessor("DATA_AIR_SUPPLY_ID"), getMaxAirSupply());
            builder.define(accessor("DATA_CUSTOM_NAME_VISIBLE"), false);
            builder.define(accessor("DATA_CUSTOM_NAME"), Optional.<Component>empty());
            builder.define(accessor("DATA_SILENT"), false);
            builder.define(accessor("DATA_NO_GRAVITY"), false);
            builder.define(DATA_POSE, Pose.STANDING);
            builder.define(accessor("DATA_TICKS_FROZEN"), 0);
            defineSynchedData(builder);
            return builder.build();
        }

        private static <T> EntityDataAccessor<T> accessor(String name) {
            return read(Entity.class, null, name);
        }

        private void assemblePlayer(Official official) {
            PlayerEquipment playerEquipment = new PlayerEquipment(this);
            set(LivingEntity.class, this, "equipment", playerEquipment);
            set(Player.class, this, "inventory", new Inventory(this, playerEquipment));
            set(Player.class, this, "abilities", new Abilities());
            set(Player.class, this, "cooldowns", new ItemCooldowns());
            set(Player.class, this, "gameProfile", official.profile);
            enderChestInventory = new PlayerEnderChestContainer();
            foodData = new FoodData();
            set(ServerPlayer.class, this, "server", official.server);
            set(ServerPlayer.class, this, "gameMode", new CreativeGameMode(this));
            advancements = official.advancements;
            connection = official.connection;
        }

        private void moveTo(Vec3 position, float yaw, float pitch, boolean sneak) {
            if (sneak) {
                setShiftKeyDown(true);
                setPose(Pose.CROUCHING);
            }
            setPos(position.x, position.y, position.z);
            setYRot(yaw);
            setXRot(pitch);
            yRotO = yaw;
            xRotO = pitch;
            yHeadRot = yaw;
            yHeadRotO = yaw;
            yBodyRot = yaw;
            yBodyRotO = yaw;
            // 构造函数没跑，站着的玩家没有经过 setPose，眼睛的高度还是 0。
            refreshDimensions();
        }

        void hold(Item main, Item off) {
            equipment.set(EquipmentSlot.MAINHAND, main == Items.AIR ? ItemStack.EMPTY : new ItemStack(main));
            equipment.set(EquipmentSlot.OFFHAND, off == Items.AIR ? ItemStack.EMPTY : new ItemStack(off));
        }

        /** ServerPlayerGameMode 只在 useItemOn 轮到物品的时候问这个，这时方块已经没有吃掉这次点击。 */
        @Override
        public boolean hasInfiniteMaterials() {
            if (CALLER.getCallerClass() == ServerPlayerGameMode.class) {
                reachedItem = true;
            }
            return super.hasInfiniteMaterials();
        }

        /** 没有权限系统的服务端：GameMaster 方块放不了也破坏不了。 */
        @Override
        public PermissionSet permissions() {
            return PermissionSet.NO_PERMISSIONS;
        }

        @Override
        public PlayerAdvancements getAdvancements() {
            return advancements;
        }

        @Override
        public void awardStat(Stat<?> stat, int count) {
        }

        @Override
        public void resetStat(Stat<?> stat) {
        }

        @Override
        public OptionalInt openMenu(MenuProvider provider) {
            return OptionalInt.empty();
        }

        @Override
        public void closeContainer() {
        }

        @Override
        public void sendSystemMessage(Component message) {
        }

        @Override
        public void sendSystemMessage(Component message, boolean overlay) {
        }

        @Override
        public void sendOverlayMessage(Component message) {
        }

        @Override
        public void sendBuildLimitMessage(boolean isTooHigh, int limit) {
        }

        @Override
        public void onUpdateAbilities() {
        }
    }

    /** 一次 levelEvent：类型、坐标、数据、官方广播时跳过的玩家。 */
    private record LevelPlay(int type, BlockPos pos, int data, boolean excluded) {
    }

    /** 一次 playSeededSound：音效、坐标、来源、音量、音调、官方广播时跳过的玩家。 */
    private record SoundPlay(int id, double x, double y, double z, int source, float volume, float pitch, boolean excluded) {
    }

    private record ScheduledBlockTick(BlockPos pos, boolean fluid, String type, int delay, int priority) {
        /** 同一格同一类型只会排着一个，所以这三样能认出是哪一个计划刻。 */
        TickKey key() {
            return new TickKey(pos, fluid, type);
        }
    }

    private record TickKey(BlockPos pos, boolean fluid, String type) {
    }

    /** 容器里一个非空的格子：坐标、槽位、物品 ID、数量。 */
    private record ContainerStack(long pos, int slot, String item, int count) {
    }

    /** 官方给出的答案。 */
    private static final class Outcome {
        final String result;
        final Long2ObjectOpenHashMap<BlockState> changes = new Long2ObjectOpenHashMap<>();
        final List<Destroyed> destroyed;
        final List<LevelPlay> levelEvents;
        final List<SoundPlay> sounds;
        final List<ScheduledBlockTick> ticks = new ArrayList<>();
        final List<ContainerStack> contents = new ArrayList<>();
        final List<String> spawned;
        final List<String> ran;
        final boolean itemReached;
        final boolean analogContent;
        final List<String> gained;
        boolean touchedRandom;

        Outcome(String result, FakeLevel level, Case c, boolean itemReached, List<String> gained) {
            this.result = result;
            this.itemReached = itemReached;
            this.gained = gained;
            this.destroyed = level.destroyed;
            this.levelEvents = level.levelEvents;
            this.sounds = level.sounds;
            this.spawned = level.spawned;
            this.ran = List.copyOf(level.ranBlocks);
            this.analogContent = level.analogContent;
            collectChanges(level, c.scene);
            collectTicks(level);
            collectContents(level);
        }

        /**
         * 结束时所有已建模容器的非空格子，按坐标和槽位排序。箱子两半分别列，和存档里的方块实体一样。
         *
         * <p>雕纹书架的内容是方块状态的镜像，Bamboo 那边由状态表示，不在这张表里。
         */
        private void collectContents(FakeLevel level) {
            long[] keys = level.blockEntities.keySet().toLongArray();
            Arrays.sort(keys);
            for (long key : keys) {
                BlockEntity entity = level.blockEntities.get(key);
                if (!(entity instanceof Container container) || !FakeLevel.modelsContents(entity)
                        || entity instanceof ChiseledBookShelfBlockEntity) {
                    continue;
                }
                for (int slot = 0; slot < container.getContainerSize(); slot++) {
                    ItemStack stack = container.getItem(slot);
                    if (!stack.isEmpty()) {
                        contents.add(new ContainerStack(key, slot, id(stack.getItem()), stack.getCount()));
                    }
                }
            }
        }

        private void collectChanges(FakeLevel level, Long2ObjectMap<BlockState> scene) {
            BlockState air = Blocks.AIR.defaultBlockState();
            for (Long2ObjectMap.Entry<BlockState> entry : level.blocks.long2ObjectEntrySet()) {
                if (entry.getValue() != scene.getOrDefault(entry.getLongKey(), air)) {
                    changes.put(entry.getLongKey(), entry.getValue());
                }
            }
        }

        /** 动作和 settle 走完之后还排着的计划刻，按执行顺序，方块的在流体的前面。 */
        private void collectTicks(FakeLevel level) {
            long now = level.getGameTime();
            for (ScheduledTick<Block> tick : level.recordedBlockTicks.pending()) {
                ticks.add(new ScheduledBlockTick(tick.pos(), false, id(tick.type()), delay(tick, now), tick.priority().getValue()));
            }
            for (ScheduledTick<Fluid> tick : level.recordedFluidTicks.pending()) {
                String fluid = BuiltInRegistries.FLUID.getKey(tick.type()).toString();
                ticks.add(new ScheduledBlockTick(tick.pos(), true, fluid, delay(tick, now), tick.priority().getValue()));
            }
        }

        private static int delay(ScheduledTick<?> tick, long now) {
            return (int) (tick.triggerTick() - now);
        }

        boolean sameWorld(Outcome other) {
            return result.equals(other.result) && changes.equals(other.changes) && destroyed.equals(other.destroyed)
                    && levelEvents.equals(other.levelEvents) && sounds.equals(other.sounds)
                    && contents.equals(other.contents) && gained.equals(other.gained)
                    && Set.copyOf(ran).equals(Set.copyOf(other.ran)) && sameTicksExceptDelay(other);
        }

        /** 随机的延迟会改变执行顺序，所以按格子和类型配对，不按下标。 */
        private boolean sameTicksExceptDelay(Outcome other) {
            Map<TickKey, ScheduledBlockTick> others = other.ticksByKey();
            return ticks.size() == others.size() && ticks.stream().allMatch(tick -> {
                ScheduledBlockTick match = others.get(tick.key());
                return match != null && match.priority == tick.priority;
            });
        }

        private Map<TickKey, ScheduledBlockTick> ticksByKey() {
            Map<TickKey, ScheduledBlockTick> byKey = new HashMap<>();
            ticks.forEach(tick -> byKey.put(tick.key(), tick));
            return byKey;
        }

        /** 世界一样、只有延迟不同时，把延迟记成 -1。调用前 sameWorld 已经为真，每一个都配得上。 */
        void maskRandomDelays(Outcome other) {
            Map<TickKey, ScheduledBlockTick> others = other.ticksByKey();
            for (int i = 0; i < ticks.size(); i++) {
                ScheduledBlockTick tick = ticks.get(i);
                if (tick.delay != others.get(tick.key()).delay) {
                    ticks.set(i, new ScheduledBlockTick(tick.pos, tick.fluid, tick.type, -1, tick.priority));
                }
            }
        }
    }

    /** 一道题。 */
    private static final class Case {
        final int id;
        final Kind kind;
        final Block subject;
        final Item item;
        final Height height;
        final Long2ObjectOpenHashMap<BlockState> scene = new Long2ObjectOpenHashMap<>();
        /** 场景里容器方块的内容，键是坐标。没有条目的容器是空的。 */
        final Long2ObjectOpenHashMap<NonNullList<ItemStack>> contents = new Long2ObjectOpenHashMap<>();
        Item main = Items.AIR;
        Item off = Items.AIR;
        InteractionHand hand = InteractionHand.MAIN_HAND;
        BlockHitResult hit;
        BlockPos target;
        Vec3 actor;
        float yaw;
        float pitch;
        boolean sneak;
        Vec3 bystander;
        AABB actorBox;
        AABB bystanderBox;
        /** 动作做完之后再走几个游戏刻。 */
        int settle;
        /** flow 题在第一个流体刻之后再跑多少刻。 */
        int steps;
        /** chunk 题的 tickSpeed：每个区块段挑几格。 */
        int speed;

        Case(int id, Kind kind, Subject subject, Height height) {
            this.id = id;
            this.kind = kind;
            this.subject = subject.block;
            this.item = subject.item;
            this.height = height;
        }

        void put(BlockPos pos, BlockState state) {
            if (height.contains(pos)) {
                scene.put(pos.asLong(), state);
            }
        }

        boolean occupied(BlockPos pos) {
            return scene.containsKey(pos.asLong());
        }

        /** 往场景里的容器放一组物品，从第 0 格起。这一格不是已建模的容器时直接报错。 */
        void putContents(BlockPos pos, ItemStack... stacks) {
            int size = containerSize(scene.get(pos.asLong()));
            if (size < 0) {
                throw new IllegalArgumentException("not a modelled container: " + pos);
            }
            NonNullList<ItemStack> list = NonNullList.withSize(size, ItemStack.EMPTY);
            for (int slot = 0; slot < stacks.length; slot++) {
                list.set(slot, stacks[slot]);
            }
            contents.put(pos.asLong(), list);
        }
    }

    /**
     * 这个状态对应的已建模容器有几格，不是的话是 -1。书架的格子是状态的镜像，不在这里填。
     *
     * <p>和 {@link FakeLevel#modelsContents} 用同一张名单；这里在建题时就查，所以临时建一个方块实体看类型。
     */
    private static int containerSize(BlockState state) {
        if (state == null || !state.hasBlockEntity()) {
            return -1;
        }
        BlockEntity entity = ((EntityBlock) state.getBlock()).newBlockEntity(BlockPos.ZERO, state);
        if (entity == null || !FakeLevel.modelsContents(entity) || entity instanceof ChiseledBookShelfBlockEntity) {
            return -1;
        }
        return entity instanceof Container container ? container.getContainerSize() : -1;
    }

    /** 建筑高度的范围，两端都包含。 */
    private record Height(int min, int max) {
        boolean contains(BlockPos pos) {
            return pos.getY() >= min && pos.getY() <= max;
        }
    }

    private record Subject(Block block, Item item) {
    }

    /** 从注册表算出来的各种方块和物品的集合。 */
    private static final class Catalog {
        private static final Set<Class<?>> GENERIC_ROOTS = Set.of(Object.class, BlockBehaviour.class, Block.class, BaseEntityBlock.class,
                HorizontalDirectionalBlock.class, DirectionalBlock.class, VegetationBlock.class);
        private static final List<Block> TERRAIN = List.of(Blocks.STONE, Blocks.DIRT, Blocks.GRASS_BLOCK, Blocks.SAND, Blocks.RED_SAND,
                Blocks.GRAVEL, Blocks.OAK_PLANKS, Blocks.OAK_LOG, Blocks.NETHERRACK, Blocks.SOUL_SAND, Blocks.SOUL_SOIL, Blocks.END_STONE,
                Blocks.FARMLAND, Blocks.CLAY, Blocks.MOSS_BLOCK, Blocks.MUD, Blocks.MYCELIUM, Blocks.PODZOL, Blocks.GLASS, Blocks.OBSIDIAN,
                Blocks.ICE, Blocks.CRIMSON_NYLIUM, Blocks.WARPED_NYLIUM, Blocks.COBBLESTONE, Blocks.ROOTED_DIRT, Blocks.SNOW_BLOCK,
                Blocks.MAGMA_BLOCK, Blocks.SLIME_BLOCK, Blocks.JUNGLE_LOG, Blocks.OAK_LEAVES);
        /** 信号源和被信号驱动的元件，用来在题目旁边堆出电路。导线单独铺，不在这里。 */
        private static final List<Block> CIRCUITRY = List.of(Blocks.REDSTONE_BLOCK, Blocks.REDSTONE_TORCH, Blocks.REDSTONE_WALL_TORCH,
                Blocks.LEVER, Blocks.STONE_BUTTON, Blocks.OAK_BUTTON, Blocks.REPEATER, Blocks.COMPARATOR, Blocks.OBSERVER,
                Blocks.DAYLIGHT_DETECTOR, Blocks.TARGET, Blocks.TRIPWIRE_HOOK, Blocks.DETECTOR_RAIL,
                Blocks.LIGHTNING_ROD.weathering().unaffected(), Blocks.SCULK_SENSOR, Blocks.CALIBRATED_SCULK_SENSOR,
                Blocks.STONE_PRESSURE_PLATE, Blocks.REDSTONE_LAMP, Blocks.COPPER_BULB.weathering().unaffected(), Blocks.NOTE_BLOCK,
                Blocks.OAK_DOOR, Blocks.IRON_DOOR, Blocks.OAK_TRAPDOOR, Blocks.OAK_FENCE_GATE, Blocks.POWERED_RAIL,
                Blocks.ACTIVATOR_RAIL, Blocks.DISPENSER, Blocks.DROPPER, Blocks.CRAFTER, Blocks.PISTON, Blocks.STICKY_PISTON,
                Blocks.TNT, Blocks.BELL, Blocks.HOPPER, Blocks.BIG_DRIPLEAF, Blocks.LECTERN, Blocks.CAKE, Blocks.COMPOSTER,
                Blocks.CAULDRON, Blocks.WATER_CAULDRON, Blocks.BEEHIVE, Blocks.RESPAWN_ANCHOR, Blocks.END_PORTAL_FRAME,
                Blocks.CHISELED_BOOKSHELF);

        private final Set<String> avoid;
        final List<Block> blocks = new ArrayList<>();
        /** blocks 里能进场景的方块，--avoid 没有排除的那些。 */
        final List<Block> scenery = new ArrayList<>();
        final List<BlockItem> blockItems = new ArrayList<>();
        /** blockItems 里能拿在手里的物品。 */
        final List<BlockItem> heldBlockItems = new ArrayList<>();
        final List<Item> items = new ArrayList<>();
        final List<Item> heldItems = new ArrayList<>();
        final Map<Class<?>, List<Item>> interactiveByClass = new LinkedHashMap<>();
        final List<List<Item>> interactive = new ArrayList<>();
        final List<Item> tools = new ArrayList<>();
        /** 用 use_item 起作用的物品：桶、细雪桶、荷叶和青蛙卵。 */
        final List<Item> usable = new ArrayList<>();
        final List<Item> pottable = new ArrayList<>();
        final List<Item> candles = new ArrayList<>();
        final List<Item> compostable = new ArrayList<>();
        final List<BlockState> sturdy = new ArrayList<>();
        final List<Block> weak = new ArrayList<>();
        final List<Block> replaceable = new ArrayList<>();
        final Set<Block> placeable = new HashSet<>();
        final Map<Class<?>, List<Block>> families = new HashMap<>();
        /** CIRCUITRY 里 --avoid 没有排除的那些。 */
        final List<Block> circuitry;
        private final List<Block> terrain;

        Catalog(Set<String> avoid) throws ReflectiveOperationException {
            this.avoid = avoid;
            this.terrain = TERRAIN.stream().filter(this::allowed).toList();
            this.circuitry = CIRCUITRY.stream().filter(this::allowed).toList();
            for (Block block : BuiltInRegistries.BLOCK) {
                if (block.isEnabled(FeatureFlags.DEFAULT_FLAGS)) {
                    addBlock(block);
                }
            }
            for (Item item : BuiltInRegistries.ITEM) {
                if (item.isEnabled(FeatureFlags.DEFAULT_FLAGS) && item != Items.AIR) {
                    addItem(item);
                }
            }
            interactive.addAll(interactiveByClass.values());
            Map<Block, Block> potted = read(FlowerPotBlock.class, null, "POTTED_BY_CONTENT");
            potted.keySet().stream().map(Block::asItem).filter(item -> item != Items.AIR).sorted(Comparator.comparing(BlockOracle::id))
                    .forEach(pottable::add);
            items.stream().filter(item -> new ItemStack(item).has(DataComponents.COMPOSTABLE)).forEach(compostable::add);
            addTools(List.of(Items.WOODEN_AXE, Items.DIAMOND_AXE, Items.STONE_SHOVEL, Items.IRON_HOE, Items.GOLDEN_PICKAXE,
                    Items.IRON_SWORD, Items.TRIDENT, Items.MACE, Items.SHEARS, Items.FLINT_AND_STEEL, Items.FIRE_CHARGE, Items.BRUSH,
                    Items.BONE_MEAL, Items.WATER_BUCKET, Items.LAVA_BUCKET, Items.BUCKET, Items.POWDER_SNOW_BUCKET, Items.GLASS_BOTTLE,
                    Items.POTION, Items.HONEY_BOTTLE, Items.HONEYCOMB, Items.GLOW_INK_SAC, Items.INK_SAC, Items.DYE.white(), Items.DYE.red(),
                    Items.GLOWSTONE, Items.BOOK, Items.WRITABLE_BOOK, Items.WRITTEN_BOOK, Items.ENCHANTED_BOOK, Items.MUSIC_DISC_13,
                    Items.ENDER_EYE, Items.COMPASS, Items.STICK, Items.BEEF, Items.COOKED_BEEF, Items.CAKE, Items.SWEET_BERRIES,
                    Items.GLOW_BERRIES, Items.WHEAT_SEEDS, Items.COCOA_BEANS, Items.BAMBOO, Items.MINECART, Items.OAK_BOAT, Items.ARMOR_STAND,
                    Items.ITEM_FRAME, Items.PAINTING, Items.LEAD, Items.NAME_TAG, Items.DEBUG_STICK));
        }

        private void addTools(List<Item> candidates) {
            candidates.stream().filter(this::allowedInHand).forEach(tools::add);
        }

        private boolean allowed(Block block) {
            return !avoid.contains(className(block));
        }

        private boolean allowedInHand(Item item) {
            return !(item instanceof BlockItem blockItem) || allowed(blockItem.getBlock());
        }

        private void addBlock(Block block) {
            blocks.add(block);
            if (block.defaultBlockState().is(BlockTags.CANDLES) && block.asItem() != Items.AIR) {
                candles.add(block.asItem());
            }
            if (allowed(block)) {
                addScenery(block);
            }
        }

        private void addScenery(Block block) {
            scenery.add(block);
            families.computeIfAbsent(familyRoot(block.getClass()), root -> new ArrayList<>()).add(block);
            BlockState state = block.defaultBlockState();
            if (isSturdy(state)) {
                sturdy.add(state);
            } else if (!state.getCollisionShape(EmptyBlockGetter.INSTANCE, BlockPos.ZERO).isEmpty()) {
                weak.add(block);
            }
            if (state.canBeReplaced()) {
                replaceable.add(block);
            }
        }

        private static boolean isSturdy(BlockState state) {
            return Arrays.stream(DIRECTIONS).allMatch(face -> state.isFaceSturdy(EmptyBlockGetter.INSTANCE, BlockPos.ZERO, face));
        }

        private void addItem(Item item) {
            items.add(item);
            if (allowedInHand(item)) {
                heldItems.add(item);
                if (item instanceof BucketItem || item instanceof SolidBucketItem || item instanceof PlaceOnWaterBlockItem) {
                    usable.add(item);
                }
            }
            if (item instanceof BlockItem blockItem) {
                blockItems.add(blockItem);
                if (allowedInHand(item)) {
                    heldBlockItems.add(blockItem);
                }
                placeable.add(blockItem.getBlock());
                if (item instanceof StandingAndWallBlockItem standing) {
                    placeable.add(read(StandingAndWallBlockItem.class, standing, "wallBlock"));
                }
            } else if (hasOwnUseOn(item)) {
                interactiveByClass.computeIfAbsent(item.getClass(), type -> new ArrayList<>()).add(item);
            }
        }

        /** 有带流体的状态：液体、气泡柱、海草这些，还有能含水的方块。 */
        static boolean holdsFluid(Block block) {
            return block.getStateDefinition().getPossibleStates().stream().anyMatch(state -> !state.getFluidState().isEmpty());
        }

        /** 有会随机刻的状态，方块自己的或者流体的。 */
        static boolean ticksRandomly(Block block) {
            return block.getStateDefinition().getPossibleStates().stream()
                    .anyMatch(state -> state.isRandomlyTicking() || state.getFluidState().isRandomlyTicking());
        }

        /** 方块物品的 useOn 就是放置，不算。 */
        static boolean hasOwnUseOn(Item item) {
            return !(item instanceof BlockItem) && (overridesUseOn(item) || new ItemStack(item).has(DataComponents.BLOCK_TRANSFORMER));
        }

        private static boolean overridesUseOn(Item item) {
            try {
                return item.getClass().getMethod("useOn", UseOnContext.class).getDeclaringClass() != Item.class;
            } catch (NoSuchMethodException error) {
                throw new IllegalStateException(error);
            }
        }

        private static Class<?> familyRoot(Class<?> type) {
            Class<?> root = type;
            while (!GENERIC_ROOTS.contains(root.getSuperclass())) {
                root = root.getSuperclass();
            }
            return root;
        }

        /** 这个方块的同族方块，--avoid 把整个家族都排除时只有它自己。 */
        List<Block> familyOf(Block block) {
            return families.getOrDefault(familyRoot(block.getClass()), List.of(block));
        }

        BlockState terrain(Random random) {
            return pick(random, terrain).defaultBlockState();
        }
    }

    /** 一种能让方块存活的邻居。side 是邻居相对方块的方向，waterBeside 表示邻居旁边还要有水。 */
    private record Support(Direction side, BlockState state, boolean waterBeside) {
    }

    private record SupportInfo(boolean free, List<Support> supports) {
    }

    /** 用官方的 canSurvive 试出每个方块要什么支撑，这样不用手写每个方块的放置规则。 */
    private static final class Supports {
        private static final int MAX_SAMPLES = 6;
        private static final int MAX_SUPPORTS = 96;
        private static final BlockPos CENTER = new BlockPos(0, 64, 0);

        private final Official official;
        private final Catalog catalog;
        private final Map<Block, SupportInfo> known = new HashMap<>();
        private final List<Block> needy = new ArrayList<>();

        Supports(Official official, Catalog catalog) {
            this.official = official;
            this.catalog = catalog;
        }

        SupportInfo of(Block block) {
            return known.computeIfAbsent(block, this::discover);
        }

        private SupportInfo discover(Block block) {
            FakeLevel level = FakeLevel.create(official, new OracleRandom(OracleRandom.Mode.LOW, 0));
            List<BlockState> states = samples(block);
            boolean free = states.stream().anyMatch(state -> survives(state, level));
            List<Support> supports = new ArrayList<>();
            if (!free) {
                collectSingleSupports(states, level, supports);
                if (supports.stream().allMatch(support -> support.state.is(block))) {
                    collectWateredSupports(states, level, supports);
                }
                Collections.shuffle(supports, new Random(BuiltInRegistries.BLOCK.getId(block)));
                supports = new ArrayList<>(supports.subList(0, Math.min(MAX_SUPPORTS, supports.size())));
            }
            return new SupportInfo(free, supports);
        }

        private static List<BlockState> samples(Block block) {
            List<BlockState> states = block.getStateDefinition().getPossibleStates();
            if (states.size() <= MAX_SAMPLES) {
                return states;
            }
            List<BlockState> samples = new ArrayList<>();
            samples.add(block.defaultBlockState());
            for (int i = 1; i < MAX_SAMPLES; i++) {
                samples.add(states.get(i * (states.size() / MAX_SAMPLES)));
            }
            return samples;
        }

        private static boolean survives(BlockState state, FakeLevel level) {
            try {
                return state.canSurvive(level, CENTER);
            } catch (RuntimeException ignored) {
                // 这里只是在猜支撑，假邻居让官方代码取不到属性时当作不行。
                return false;
            }
        }

        private void collectSingleSupports(List<BlockState> states, FakeLevel level, List<Support> supports) {
            for (Direction side : DIRECTIONS) {
                BlockPos neighbour = CENTER.relative(side);
                for (Block candidate : catalog.scenery) {
                    BlockState support = candidate.defaultBlockState();
                    level.blocks.clear();
                    level.blocks.put(neighbour.asLong(), support);
                    if (states.stream().anyMatch(state -> survives(state, level))) {
                        supports.add(new Support(side, support, false));
                    }
                }
            }
        }

        /** 甘蔗这样的方块，脚下的方块旁边还要有水。 */
        private void collectWateredSupports(List<BlockState> states, FakeLevel level, List<Support> supports) {
            BlockPos below = CENTER.below();
            for (Block candidate : catalog.scenery) {
                BlockState support = candidate.defaultBlockState();
                level.blocks.clear();
                level.blocks.put(below.asLong(), support);
                level.blocks.put(below.east().asLong(), Blocks.WATER.defaultBlockState());
                if (states.stream().anyMatch(state -> survives(state, level))) {
                    supports.add(new Support(Direction.DOWN, support, true));
                }
            }
        }

        List<Block> needy() {
            if (needy.isEmpty()) {
                for (Block block : catalog.scenery) {
                    if (!of(block).free && !of(block).supports.isEmpty()) {
                        needy.add(block);
                    }
                }
            }
            return needy;
        }
    }

    /** 出题。每一题只用自己的种子，所以 --case 能单独重现。 */
    private static final class Generator {
        private static final float[] CURSORS = {0.0F, 0.0625F, 0.25F, 0.4999F, 0.5F, 0.5001F, 0.75F, 0.9375F, 1.0F};
        private static final float[] INSET_SURFACES = {0.125F, 0.1875F, 0.5F, 0.75F, 0.875F, 0.9375F};
        private static final int DEPENDANT_LAYERS = 3;
        private static final int[] FLOW_STEPS = {0, 1, 5, 10, 20, 40, 80, 120};
        /** 3 是默认的 random_tick_speed，大的值让场景里的方块在一题里多半都被挑中。 */
        private static final int[] CHUNK_SPEEDS = {1, 3, 64, 1024};
        private static final double STANDING_EYE_HEIGHT = 1.62;
        private static final double CROUCHING_EYE_HEIGHT = 1.27;
        private static final float[] LOOK_PITCHES = {-90.0F, -89.9F, -60.0F, -45.0F, -30.0F, 0.0F, 30.0F, 45.0F, 60.0F, 89.9F, 90.0F};
        /** 随机填进场景容器的物品。剑用来试堆叠上限 1。 */
        private static final List<Item> CONTAINER_ITEMS = List.of(Items.STONE, Items.DIRT, Items.OAK_LOG, Items.BREAD,
                Items.COBBLESTONE, Items.IRON_INGOT, Items.IRON_SWORD);

        private final Catalog catalog;
        private final Supports supports;
        private final FakeLevel probe;
        private final Height height;

        Generator(Official official, Catalog catalog, Supports supports) {
            this.catalog = catalog;
            this.supports = supports;
            this.probe = FakeLevel.create(official, new OracleRandom(OracleRandom.Mode.LOW, 0));
            this.height = new Height(probe.getMinY(), probe.getMaxY());
        }

        Case build(int id, Kind kind, Subject subject, long seed) {
            Random random = new Random(seed * 0x9E3779B97F4A7C15L + id);
            Case c = new Case(id, kind, subject, height);
            BlockPos focus = switch (kind) {
                case PLACE -> buildPlace(c, random);
                case USE -> buildUse(c, random);
                case DESTROY -> buildDestroy(c, random);
                case TICK -> buildTick(c, random);
                case FLOW -> buildFlow(c, random);
                case ITEM -> buildItem(c, random);
                case RANDOM -> buildRandom(c, random);
                case CHUNK -> buildChunk(c, random);
            };
            // flow 和 item 题考的是流体和物品本身，不加红石电路，也不额外 settle。
            if (kind != Kind.FLOW && kind != Kind.ITEM) {
                addCircuit(c, random, focus);
                if (c.settle == 0) {
                    c.settle = pickSettle(c, random);
                }
            }
            fillContainers(c, random);
            return c;
        }

        /** 场景里的容器随机放些物品，比较器读数、漏斗搬运和投掷器才有内容可比。 */
        private void fillContainers(Case c, Random random) {
            long[] keys = c.scene.keySet().toLongArray();
            Arrays.sort(keys);
            for (long key : keys) {
                if (c.contents.containsKey(key)) {
                    continue;
                }
                int size = containerSize(c.scene.get(key));
                if (size < 0 || chance(random, 40)) {
                    continue;
                }
                NonNullList<ItemStack> list = NonNullList.withSize(size, ItemStack.EMPTY);
                boolean full = chance(random, 10);
                int stacks = full ? size : 1 + random.nextInt(Math.min(3, size));
                List<Integer> slots = new ArrayList<>(size);
                for (int slot = 0; slot < size; slot++) {
                    slots.add(slot);
                }
                Collections.shuffle(slots, random);
                for (int i = 0; i < stacks; i++) {
                    Item item = pick(random, CONTAINER_ITEMS);
                    int max = new ItemStack(item).getMaxStackSize();
                    int count = full ? max : 1 + random.nextInt(max);
                    list.set(slots.get(i), new ItemStack(item, count));
                }
                c.contents.put(key, list);
            }
        }

        private static final BlockPos PISTON_CELL = new BlockPos(0, 64, 0);
        /** 多刻活塞题各出 settle = 1..4：推方块、再推半格、放回方块、放回方块引起的连锁。 */
        private static final int PISTON_SETTLE_STEPS = 4;

        /**
         * 多刻活塞题：场景和动作都是固定的，同一道题按 settle 各出一遍，逐刻比较世界。
         *
         * <p>随机题里活塞旁边不一定有信号，走得完整个动画的很少，所以单独出这几道。id 从 base 开始，
         * {@code --case} 也能单独重现。
         */
        List<Case> pistonCases(int base) {
            List<Scenario> scenarios = List.of(
                    new Scenario(Kind.PLACE, Blocks.PISTON, Items.REDSTONE_BLOCK, c -> {
                        c.put(PISTON_CELL, piston(Blocks.PISTON, Direction.EAST, false));
                        c.put(PISTON_CELL.relative(Direction.EAST, 2), Blocks.STONE.defaultBlockState());
                        placeOnTop(c, PISTON_CELL, Items.REDSTONE_BLOCK);
                    }),
                    new Scenario(Kind.PLACE, Blocks.STICKY_PISTON, Items.REDSTONE_BLOCK, c -> {
                        c.put(PISTON_CELL, piston(Blocks.STICKY_PISTON, Direction.EAST, false));
                        c.put(PISTON_CELL.relative(Direction.EAST, 2), Blocks.STONE.defaultBlockState());
                        placeOnTop(c, PISTON_CELL, Items.REDSTONE_BLOCK);
                    }),
                    new Scenario(Kind.DESTROY, Blocks.PISTON, Items.AIR, c -> {
                        c.put(PISTON_CELL, piston(Blocks.PISTON, Direction.EAST, true));
                        c.put(PISTON_CELL.relative(Direction.EAST), head(Direction.EAST, PistonType.DEFAULT));
                        c.put(PISTON_CELL.relative(Direction.EAST, 2), Blocks.STONE.defaultBlockState());
                        c.put(PISTON_CELL.above(), Blocks.REDSTONE_BLOCK.defaultBlockState());
                        destroyAt(c, PISTON_CELL.above());
                    }),
                    new Scenario(Kind.DESTROY, Blocks.STICKY_PISTON, Items.AIR, c -> {
                        c.put(PISTON_CELL, piston(Blocks.STICKY_PISTON, Direction.EAST, true));
                        c.put(PISTON_CELL.relative(Direction.EAST), head(Direction.EAST, PistonType.STICKY));
                        c.put(PISTON_CELL.relative(Direction.EAST, 2), Blocks.STONE.defaultBlockState());
                        c.put(PISTON_CELL.above(), Blocks.REDSTONE_BLOCK.defaultBlockState());
                        destroyAt(c, PISTON_CELL.above());
                    }),
                    new Scenario(Kind.PLACE, Blocks.PISTON, Items.REDSTONE_BLOCK, c -> {
                        BlockPos slime = PISTON_CELL.relative(Direction.EAST);
                        c.put(PISTON_CELL, piston(Blocks.PISTON, Direction.EAST, false));
                        c.put(slime, Blocks.SLIME_BLOCK.defaultBlockState());
                        c.put(slime.above(), Blocks.STONE.defaultBlockState());
                        c.put(slime.relative(Direction.EAST), Blocks.STONE.defaultBlockState());
                        placeOnTop(c, PISTON_CELL, Items.REDSTONE_BLOCK);
                    }),
                    new Scenario(Kind.PLACE, Blocks.PISTON, Items.REDSTONE_BLOCK, c -> {
                        c.put(PISTON_CELL, piston(Blocks.PISTON, Direction.EAST, false));
                        c.put(PISTON_CELL.relative(Direction.EAST), Blocks.OBSIDIAN.defaultBlockState());
                        placeOnTop(c, PISTON_CELL, Items.REDSTONE_BLOCK);
                    }),
                    new Scenario(Kind.PLACE, Blocks.PISTON, Items.REDSTONE_BLOCK, c -> {
                        BlockPos torch = PISTON_CELL.relative(Direction.EAST);
                        c.put(PISTON_CELL, piston(Blocks.PISTON, Direction.EAST, false));
                        c.put(torch.below(), Blocks.STONE.defaultBlockState());
                        c.put(torch, Blocks.TORCH.defaultBlockState());
                        placeOnTop(c, PISTON_CELL, Items.REDSTONE_BLOCK);
                    }));
            List<Case> cases = new ArrayList<>();
            for (Scenario scenario : scenarios) {
                for (int settle = 1; settle <= PISTON_SETTLE_STEPS; settle++) {
                    Case c = new Case(base + cases.size(), scenario.kind(),
                            new Subject(scenario.subject(), scenario.item()), height);
                    scenario.setup().accept(c);
                    c.settle = settle;
                    cases.add(c);
                }
            }
            return cases;
        }

        /** 固定场景的最小描述。{@code item} 只给汇总用，{@code setup} 填场景和动作。 */
        private record Scenario(Kind kind, Block subject, Item item, Consumer<Case> setup) {
        }

        private static BlockState piston(Block block, Direction facing, boolean extended) {
            return block.defaultBlockState()
                    .setValue(PistonBaseBlock.FACING, facing)
                    .setValue(PistonBaseBlock.EXTENDED, extended);
        }

        private static BlockState head(Direction facing, PistonType type) {
            return Blocks.PISTON_HEAD.defaultBlockState()
                    .setValue(PistonHeadBlock.FACING, facing)
                    .setValue(PistonHeadBlock.TYPE, type);
        }

        /** 站得够远，往 pos 的顶面放 item，方块落在 pos 上面那一格。 */
        private static void placeOnTop(Case c, BlockPos pos, Item item) {
            c.main = item;
            c.hand = InteractionHand.MAIN_HAND;
            c.hit = new BlockHitResult(new Vec3(pos.getX() + 0.5, pos.getY() + 1.0, pos.getZ() + 0.5), Direction.UP, pos, false);
            c.actor = new Vec3(pos.getX() + 0.5, pos.getY(), pos.getZ() + 2.5);
            c.yaw = 0.0F;
            c.pitch = 0.0F;
        }

        /** 站在 pos 旁边打掉它。 */
        private static void destroyAt(Case c, BlockPos pos) {
            c.target = pos;
            c.actor = new Vec3(pos.getX() + 0.5, pos.getY() - 1.0, pos.getZ() + 2.5);
            c.yaw = 0.0F;
            c.pitch = 0.0F;
        }

        /** 漏斗题的格子和 settle 步数。物品每 8 刻挪一格，40 和 100 刻能看到连搬几次。 */
        private static final BlockPos HOPPER_CELL = new BlockPos(0, 64, 0);
        private static final int[] HOPPER_SETTLE_STEPS = {1, 9, 40, 100};

        /**
         * 漏斗和投掷器的固定题：场景摆好，同一道按 settle 各出一遍，逐刻比较整个世界的容器内容。
         *
         * <p>随机题里漏斗旁边不一定有容器，物品也不一定搬得起来，所以单独出这些。
         * id 从 base 开始，{@code --case} 也能单独重现。
         */
        List<Case> hopperCases(int base) {
            List<Scenario> scenarios = List.of(
                    // 上面的箱子有东西，漏斗从上面吸，往下面的箱子送。
                    new Scenario(Kind.TICK, Blocks.HOPPER, Items.STONE, c -> {
                        c.put(HOPPER_CELL.above(), Blocks.CHEST.defaultBlockState());
                        c.putContents(HOPPER_CELL.above(), new ItemStack(Items.STONE, 5), new ItemStack(Items.BREAD, 2));
                        c.put(HOPPER_CELL, hopper(Direction.DOWN, true));
                        c.put(HOPPER_CELL.below(), Blocks.CHEST.defaultBlockState());
                        c.putContents(HOPPER_CELL.below(), new ItemStack(Items.DIRT, 1));
                        c.target = HOPPER_CELL;
                    }),
                    // 漏斗串：上面的漏斗把东西送进下面的漏斗，下面那个再往箱子里送。
                    new Scenario(Kind.TICK, Blocks.HOPPER, Items.STONE, c -> {
                        c.put(HOPPER_CELL, hopper(Direction.DOWN, true));
                        c.put(HOPPER_CELL.above(), hopper(Direction.DOWN, true));
                        c.putContents(HOPPER_CELL.above(), new ItemStack(Items.STONE, 5));
                        c.put(HOPPER_CELL.below(), Blocks.CHEST.defaultBlockState());
                        c.putContents(HOPPER_CELL.below(), new ItemStack(Items.DIRT, 1));
                        c.target = HOPPER_CELL;
                    }),
                    // 被红石锁住的漏斗什么都不搬。
                    new Scenario(Kind.TICK, Blocks.HOPPER, Items.STONE, c -> {
                        c.put(HOPPER_CELL.above(), Blocks.CHEST.defaultBlockState());
                        c.putContents(HOPPER_CELL.above(), new ItemStack(Items.STONE, 5));
                        c.put(HOPPER_CELL, hopper(Direction.DOWN, false));
                        c.put(HOPPER_CELL.below(), Blocks.CHEST.defaultBlockState());
                        c.putContents(HOPPER_CELL.below(), new ItemStack(Items.DIRT, 1));
                        c.target = HOPPER_CELL;
                    }),
                    // 投掷器朝箱子，把东西塞进去。
                    new Scenario(Kind.TICK, Blocks.DROPPER, Items.STONE, c -> {
                        c.put(HOPPER_CELL, dropper(Direction.DOWN));
                        c.putContents(HOPPER_CELL, new ItemStack(Items.STONE, 3), new ItemStack(Items.BREAD, 2));
                        c.put(HOPPER_CELL.below(), Blocks.CHEST.defaultBlockState());
                        c.putContents(HOPPER_CELL.below(), new ItemStack(Items.DIRT, 1));
                        c.target = HOPPER_CELL;
                    }),
                    // 投掷器朝空气，东西消失；随机数两边要取一样多。
                    new Scenario(Kind.TICK, Blocks.DROPPER, Items.STONE, c -> {
                        c.put(HOPPER_CELL, dropper(Direction.UP));
                        c.putContents(HOPPER_CELL, new ItemStack(Items.STONE, 3), new ItemStack(Items.BREAD, 2));
                        c.target = HOPPER_CELL;
                    }),
                    // 漏斗从上面的箱子吸到第一件物品，旁边的比较器读数从 0 变成 1，灯亮。
                    new Scenario(Kind.TICK, Blocks.HOPPER, Items.STONE, c -> {
                        c.put(HOPPER_CELL.above(), Blocks.CHEST.defaultBlockState());
                        c.putContents(HOPPER_CELL.above(), new ItemStack(Items.STONE, 1));
                        c.put(HOPPER_CELL, hopper(Direction.DOWN, true));
                        c.put(HOPPER_CELL.east(), comparator(Direction.WEST));
                        c.put(HOPPER_CELL.east(2), Blocks.REDSTONE_LAMP.defaultBlockState());
                        c.target = HOPPER_CELL;
                    }),
                    // 上面的漏斗往各种容器里塞东西，旁边的比较器读数跟着变。
                    pushingInto(Blocks.CHEST),
                    pushingInto(Blocks.BARREL),
                    pushingInto(Blocks.SHULKER_BOX),
                    pushingInto(Blocks.DISPENSER),
                    pushingInto(Blocks.DROPPER),
                    // 双箱：比较器读的是两半合起来的 54 格。
                    new Scenario(Kind.TICK, Blocks.CHEST, Items.STONE, c -> {
                        BlockPos first = HOPPER_CELL;
                        c.put(first, Blocks.CHEST.defaultBlockState()
                                .setValue(ChestBlock.FACING, Direction.NORTH)
                                .setValue(ChestBlock.TYPE, ChestType.LEFT));
                        c.putContents(first, new ItemStack(Items.STONE, 10));
                        c.put(first.east(), Blocks.CHEST.defaultBlockState()
                                .setValue(ChestBlock.FACING, Direction.NORTH)
                                .setValue(ChestBlock.TYPE, ChestType.RIGHT));
                        c.putContents(first.east(), new ItemStack(Items.DIRT, 5));
                        c.put(first.above(), hopper(Direction.DOWN, true));
                        c.putContents(first.above(), new ItemStack(Items.STONE, 3));
                        c.put(first.east().east(), comparator(Direction.WEST));
                        c.put(first.east(2).east(), Blocks.REDSTONE_LAMP.defaultBlockState());
                        c.target = first.above();
                    }));
            List<Case> cases = new ArrayList<>();
            for (Scenario scenario : scenarios) {
                for (int settle : HOPPER_SETTLE_STEPS) {
                    Case c = new Case(base + cases.size(), scenario.kind(),
                            new Subject(scenario.subject(), scenario.item()), height);
                    scenario.setup().accept(c);
                    // 假玩家要有个位置；漏斗题里它不碰方块，站远一点。
                    if (c.actor == null) {
                        c.actor = new Vec3(4.5, 70.0, 4.5);
                    }
                    c.settle = settle;
                    cases.add(c);
                }
            }
            return cases;
        }

        /** 上面一个装了石头 3 个的漏斗，往下面这一类容器里塞；容器旁边是比较器和灯。 */
        private static Scenario pushingInto(Block container) {
            return new Scenario(Kind.TICK, container, Items.STONE, c -> {
                c.put(HOPPER_CELL, container.defaultBlockState());
                c.putContents(HOPPER_CELL, new ItemStack(Items.STONE, 1), new ItemStack(Items.BREAD, 1));
                c.put(HOPPER_CELL.above(), hopper(Direction.DOWN, true));
                c.putContents(HOPPER_CELL.above(), new ItemStack(Items.STONE, 3));
                c.put(HOPPER_CELL.east(), comparator(Direction.WEST));
                c.put(HOPPER_CELL.east(2), Blocks.REDSTONE_LAMP.defaultBlockState());
                c.target = HOPPER_CELL.above();
            });
        }

        private static BlockState hopper(Direction facing, boolean enabled) {
            return Blocks.HOPPER.defaultBlockState()
                    .setValue(HopperBlock.FACING, facing)
                    .setValue(HopperBlock.ENABLED, enabled);
        }

        private static BlockState dropper(Direction facing) {
            return Blocks.DROPPER.defaultBlockState().setValue(DispenserBlock.FACING, facing);
        }

        /** 比较器的 FACING 指着它读的那一格，输出在反面。 */
        private static BlockState comparator(Direction input) {
            return Blocks.COMPARATOR.defaultBlockState().setValue(HorizontalDirectionalBlock.FACING, input);
        }

        /**
         * 一半的题在题目旁边堆一点红石，三种样子里挑一种：
         *
         * <ul>
         *   <li>散放的信号源和元件，加一条导线。状态是随机的，不一定自洽，官方的 updateShape 和 neighborChanged
         *       会把它纠正过来，要比的就是这一步。</li>
         *   <li>从题目旁边往外接的一串元件。题目一变，信号顺着它传出去，中继器的延迟要走几刻才看得到。</li>
         *   <li>火把和导线绕成的环。题目一碰它就开始闪，闪到第八次烧坏。</li>
         * </ul>
         */
        private void addCircuit(Case c, Random random, BlockPos focus) {
            if (catalog.circuitry.isEmpty() || !chance(random, 50)) {
                return;
            }
            Set<Long> reserved = reserveForCircuit(c, focus);
            int roll = random.nextInt(100);
            if (roll < 40) {
                addLooseParts(c, random, focus, reserved);
                addWire(c, random, focus, reserved);
            } else if (roll < 85) {
                addChain(c, random, focus, reserved);
            } else {
                addTorchClock(c, random, focus, reserved);
            }
        }

        private void addLooseParts(Case c, Random random, BlockPos focus, Set<Long> reserved) {
            int parts = 1 + random.nextInt(4);
            for (int i = 0; i < parts; i++) {
                BlockPos at = focus.offset(random.nextInt(5) - 2, random.nextInt(3) - 1, random.nextInt(5) - 2);
                if (free(c, at, reserved)) {
                    c.put(at, randomState(random, pick(random, catalog.circuitry)));
                }
            }
        }

        private static boolean free(Case c, BlockPos pos, Set<Long> reserved) {
            return !c.occupied(pos) && !reserved.contains(pos.asLong());
        }

        /** 四个水平方向打乱以后，第一个让 cells 给出的格子全都空着的方向。 */
        private static Optional<Direction> freeDirection(Case c, Random random, Set<Long> reserved,
                Function<Direction, List<BlockPos>> cells) {
            List<Direction> directions = new ArrayList<>(List.of(HORIZONTAL));
            Collections.shuffle(directions, random);
            return directions.stream()
                    .filter(along -> cells.apply(along).stream().allMatch(pos -> free(c, pos, reserved)))
                    .findFirst();
        }

        /**
         * 从题目旁边沿一个水平方向往外接一串，每一节脚下垫导体：导线、中继器、比较器、导体，最后一节是随便一个元件。
         * 中继器和比较器的输入朝着题目。开头旁边有时放一个信号源。状态多半是默认的，少数是随机的。
         */
        private void addChain(Case c, Random random, BlockPos focus, Set<Long> reserved) {
            int length = 2 + random.nextInt(5);
            Optional<Direction> found = freeDirection(c, random, reserved, along -> {
                List<BlockPos> cells = new ArrayList<>();
                for (int i = 1; i <= length; i++) {
                    cells.add(focus.relative(along, i));
                    cells.add(focus.relative(along, i).below());
                }
                return cells;
            });
            if (found.isEmpty()) {
                return;
            }
            Direction along = found.get();
            for (int i = 1; i <= length; i++) {
                BlockPos at = focus.relative(along, i);
                c.put(at.below(), conductor(random));
                c.put(at, i == length ? randomState(random, pick(random, catalog.circuitry)) : chainLink(random, along));
            }
            BlockPos beside = focus.relative(along).relative(along.getClockWise());
            if (chance(random, 40) && free(c, beside, reserved)) {
                c.put(beside, randomState(random, pick(random, catalog.circuitry)));
            }
        }

        /** 串里的一节，信号从 along 的反方向进来。 */
        private BlockState chainLink(Random random, Direction along) {
            int roll = random.nextInt(100);
            BlockState link;
            if (roll < 45) {
                link = Blocks.REDSTONE_WIRE.defaultBlockState();
            } else if (roll < 70) {
                link = Blocks.REPEATER.defaultBlockState().setValue(BlockStateProperties.DELAY, 1 + random.nextInt(4));
            } else if (roll < 82) {
                link = Blocks.COMPARATOR.defaultBlockState()
                        .setValue(BlockStateProperties.MODE_COMPARATOR, pick(random, List.of(ComparatorMode.values())));
            } else {
                return conductor(random);
            }
            if (!catalog.allowed(link.getBlock())) {
                return conductor(random);
            }
            if (chance(random, 20)) {
                link = randomState(random, link.getBlock());
            }
            if (link.hasProperty(BlockStateProperties.HORIZONTAL_FACING)) {
                link = link.setValue(BlockStateProperties.HORIZONTAL_FACING, along.getOpposite());
            }
            return link;
        }

        /** 导体，多半是石头，偶尔是别的地形方块。地形里有玻璃这种不导电的，正好也考一下。 */
        private BlockState conductor(Random random) {
            return chance(random, 80) && catalog.allowed(Blocks.STONE) ? Blocks.STONE.defaultBlockState() : catalog.terrain(random);
        }

        /**
         * 火把时钟：墙上的火把插在 base 的侧面，把头顶的 cover 强充能；导线在 base 顶上，挨着 cover，
         * 读到信号就给 base 充能，火把于是熄灭，信号断了再亮回来，每两刻翻一次，60 刻里翻到第八次烧坏。
         * 导线挨着题目，题目一动它就重新算，环就转起来了，所以要多走几十刻。
         */
        private void addTorchClock(Case c, Random random, BlockPos focus, Set<Long> reserved) {
            if (!catalog.allowed(Blocks.REDSTONE_WIRE) || !catalog.allowed(Blocks.REDSTONE_WALL_TORCH)) {
                return;
            }
            Optional<Direction> found = freeDirection(c, random, reserved, along -> {
                BlockPos wire = focus.relative(along);
                return List.of(wire, wire.below(), wire.below().relative(along), wire.relative(along));
            });
            if (found.isEmpty()) {
                return;
            }
            Direction along = found.get();
            BlockPos wire = focus.relative(along);
            BlockPos torch = wire.below().relative(along);
            c.put(wire.below(), conductor(random));
            c.put(torch.above(), conductor(random));
            c.put(wire, Blocks.REDSTONE_WIRE.defaultBlockState());
            c.put(torch, Blocks.REDSTONE_WALL_TORCH.defaultBlockState().setValue(BlockStateProperties.HORIZONTAL_FACING, along));
            c.settle = 20 + random.nextInt(41);
        }

        /** 放置的目标格和点中的格子不能被电路占掉，否则这一题考的就不是原来那件事了。 */
        private static Set<Long> reserveForCircuit(Case c, BlockPos focus) {
            Set<Long> reserved = new HashSet<>();
            reserved.add(focus.asLong());
            reserved.add(focus.above().asLong());
            if (c.hit != null) {
                BlockPos clicked = c.hit.getBlockPos();
                reserved.add(clicked.asLong());
                reserved.add(clicked.relative(c.hit.getDirection()).asLong());
            }
            return reserved;
        }

        /** 一条直线的导线，底下多半垫实心方块。导线网络的功率计算要靠它才比得到。 */
        private void addWire(Case c, Random random, BlockPos focus, Set<Long> reserved) {
            if (!catalog.allowed(Blocks.REDSTONE_WIRE) || !chance(random, 60)) {
                return;
            }
            Direction along = pick(random, List.of(HORIZONTAL));
            BlockPos at = focus.relative(pick(random, List.of(HORIZONTAL)));
            int length = 1 + random.nextInt(4);
            for (int i = 0; i < length; i++, at = at.relative(along)) {
                if (c.occupied(at) || reserved.contains(at.asLong())) {
                    continue;
                }
                c.put(at, randomState(random, Blocks.REDSTONE_WIRE));
                if (!c.occupied(at.below()) && !reserved.contains(at.below().asLong()) && chance(random, 80)) {
                    c.put(at.below(), pickSolid(random));
                }
            }
        }

        /** 题目的方块带着流体，周围是有洞的地板和零散的方块，跑完第一个流体刻以后再跑几刻。 */
        private BlockPos buildFlow(Case c, Random random) {
            BlockPos target = originInWorld(random);
            BlockState state = withFluid(random, c.subject);
            c.put(target, state);
            complete(c, random, target, state);
            addSupports(c, random, target, 50);
            flowTerrain(c, random, target);
            pickActor(c, random, target);
            c.target = target;
            c.steps = FLOW_STEPS[random.nextInt(FLOW_STEPS.length)];
            return target;
        }

        /** 题目方块带流体的状态：液体多半是源头，别的方块从带流体的状态里挑。 */
        private static BlockState withFluid(Random random, Block block) {
            if (block instanceof LiquidBlock && chance(random, 50)) {
                return block.defaultBlockState();
            }
            List<BlockState> wet = block.getStateDefinition().getPossibleStates().stream()
                    .filter(state -> !state.getFluidState().isEmpty()).toList();
            return pick(random, wet);
        }

        /**
         * 流体要流过的地方：目标下面一到三格是一层地板，地板上有洞；地板和目标之间零散地放着方块，偶尔有水和岩浆。
         * 地板不用会下落的方块，它们旁边一有水流过来就排下落的刻，这种题只能排除。
         */
        private void flowTerrain(Case c, Random random, BlockPos target) {
            int radius = 2 + random.nextInt(5);
            int holes = 8 * random.nextInt(4);
            int clutter = 4 + 6 * random.nextInt(4);
            int floorY = target.getY() - 1 - (chance(random, 30) ? random.nextInt(3) : 0);
            for (int dx = -radius; dx <= radius; dx++) {
                for (int dz = -radius; dz <= radius; dz++) {
                    BlockPos floor = new BlockPos(target.getX() + dx, floorY, target.getZ() + dz);
                    if (!c.occupied(floor) && !chance(random, holes)) {
                        c.put(floor, flowFloor(random));
                    }
                    for (int y = floorY + 1; y <= target.getY() + 1; y++) {
                        BlockPos at = new BlockPos(floor.getX(), y, floor.getZ());
                        if (!c.occupied(at) && chance(random, clutter)) {
                            putNeighbour(c, random, at);
                        }
                    }
                }
            }
            addOtherFluids(c, random, target, radius);
        }

        private BlockState flowFloor(Random random) {
            BlockState state = pickSolid(random);
            return state.getBlock() instanceof Fallable ? Blocks.STONE.defaultBlockState() : state;
        }

        /** 另一种流体的源头，或者灵魂土上的蓝冰，岩浆碰到它们会凝固。 */
        private void addOtherFluids(Case c, Random random, BlockPos target, int radius) {
            int count = random.nextInt(4);
            for (int i = 0; i < count; i++) {
                BlockPos at = target.offset(random.nextInt(2 * radius + 1) - radius, random.nextInt(3) - 1,
                        random.nextInt(2 * radius + 1) - radius);
                if (c.occupied(at)) {
                    continue;
                }
                int roll = random.nextInt(100);
                if (roll < 40) {
                    c.put(at, chance(random, 60) ? Blocks.LAVA.defaultBlockState() : randomState(random, Blocks.LAVA));
                } else if (roll < 80) {
                    c.put(at, chance(random, 60) ? Blocks.WATER.defaultBlockState() : randomState(random, Blocks.WATER));
                } else {
                    c.put(at, Blocks.BLUE_ICE.defaultBlockState());
                    c.put(target.below(), Blocks.SOUL_SOIL.defaultBlockState());
                }
            }
        }

        /**
         * 动作做完之后再走几刻。场景里没有红石时不走：别的类的计划刻要么没移植，要么和这一章无关，
         * 白走一场还会让题目作废。
         */
        private int pickSettle(Case c, Random random) {
            if (!hasCircuitry(c)) {
                return 0;
            }
            int roll = random.nextInt(100);
            if (roll < 30) {
                return 0;
            }
            if (roll < 70) {
                // 中继器最长 8 刻，侦测器、火把、比较器 2 刻，灯 4 刻。
                return 1 + random.nextInt(8);
            }
            // 按钮弹回是 20 刻（石头）或者 30 刻（木头），一串中继器要的更久。
            return roll < 85 ? 9 + random.nextInt(12) : 21 + random.nextInt(20);
        }

        /** 场景里有信号源或者元件。 */
        private boolean hasCircuitry(Case c) {
            if (catalog.circuitry.contains(c.subject) || c.subject == Blocks.REDSTONE_WIRE) {
                return true;
            }
            for (BlockState state : c.scene.values()) {
                if (state.is(Blocks.REDSTONE_WIRE) || catalog.circuitry.contains(state.getBlock())) {
                    return true;
                }
            }
            return false;
        }

        /** 用 use_item 的题：玩家从几格外看着题目的方块，手里拿着桶、荷叶或者青蛙卵。 */
        private BlockPos buildItem(Case c, Random random) {
            BlockPos target = originInWorld(random);
            BlockState state = randomState(random, c.subject);
            c.put(target, state);
            complete(c, random, target, state);
            addSupports(c, random, target, 50);
            decorate(c, random, target, Set.of(target.asLong()));
            if (chance(random, 30)) {
                addOtherFluids(c, random, target, 1);
            }
            aimAt(c, random, target);
            c.target = target;
            holdForItem(c, random);
            return target;
        }

        /** 眼睛在目标附近一点的反方向上，离开 1 到 6 格，交互距离是 5 格，所以有一些题够不着。 */
        private static void aimAt(Case c, Random random, BlockPos target) {
            c.sneak = chance(random, 30);
            c.yaw = pickYaw(random);
            c.pitch = pickPitch(random);
            Vec3 point = new Vec3(target.getX() + random.nextDouble(), target.getY() + random.nextDouble(),
                    target.getZ() + random.nextDouble());
            if (chance(random, 15)) {
                point = point.add(random.nextDouble() * 4.0 - 2.0, random.nextDouble() * 4.0 - 2.0, random.nextDouble() * 4.0 - 2.0);
            }
            double distance = 1.0 + random.nextDouble() * 5.0;
            Vec3 eye = point.subtract(Player.calculateViewVector(c.pitch, c.yaw).scale(distance));
            double eyeHeight = c.sneak ? CROUCHING_EYE_HEIGHT : STANDING_EYE_HEIGHT;
            c.actor = new Vec3(eye.x, eye.y - eyeHeight, eye.z);
        }

        private void holdForItem(Case c, Random random) {
            if (chance(random, 80)) {
                c.main = c.item;
                c.off = chance(random, 20) ? pickHeld(random, c.subject) : Items.AIR;
                return;
            }
            c.hand = InteractionHand.OFF_HAND;
            c.off = c.item;
            int roll = random.nextInt(100);
            // 主手拿着光源方块或者脚手架时，它们的选中框是整格，视线会停在上面。
            c.main = roll < 40 ? Items.AIR : roll < 60 ? pick(random, List.of(Items.LIGHT, Items.SCAFFOLDING)) : pickHeld(random, c.subject);
        }

        private BlockPos buildPlace(Case c, Random random) {
            BlockPos target = origin(random);
            pickActor(c, random, target);
            Direction facing = Direction.fromYRot(c.yaw);
            Set<Long> reserved = reserve(random, target, facing);
            int roll = random.nextInt(100);
            if (roll < 46) {
                placeAgainstNeighbour(c, random, target);
            } else if (roll < 62) {
                placeIntoReplaceable(c, random, target);
            } else if (roll < 74) {
                placeOntoOwnKind(c, random, target);
            } else if (roll < 86) {
                placeInWater(c, random, target);
            } else {
                placeAnywhere(c, random, target);
            }
            decorate(c, random, target, reserved);
            holdForPlacing(c, random);
            return target;
        }

        /** 目标格、上面一格、面前一格多半留空，门和床才放得下。 */
        private static Set<Long> reserve(Random random, BlockPos target, Direction facing) {
            Set<Long> reserved = new HashSet<>();
            reserved.add(target.asLong());
            if (chance(random, 80)) {
                reserved.add(target.above().asLong());
                reserved.add(target.relative(facing).asLong());
            }
            return reserved;
        }

        private void holdForPlacing(Case c, Random random) {
            if (chance(random, 85)) {
                c.main = c.item;
                c.off = chance(random, 20) ? pickHeld(random, c.subject) : Items.AIR;
            } else {
                c.hand = InteractionHand.OFF_HAND;
                c.off = c.item;
                c.main = chance(random, 50) ? Items.AIR : pickHeld(random, c.subject);
            }
        }

        private void placeAgainstNeighbour(Case c, Random random, BlockPos target) {
            SupportInfo info = supports.of(c.subject);
            Support support = !info.supports.isEmpty() && chance(random, 75) ? pick(random, info.supports) : null;
            Direction side = support != null ? support.side : pickSide(random);
            if (!clickable(target.relative(side))) {
                side = towardsWorld(target);
                support = null;
            }
            BlockPos clicked = target.relative(side);
            c.put(clicked, support != null ? support.state : pickSolid(random));
            if (support != null && support.waterBeside) {
                c.put(clicked.relative(pick(random, List.of(HORIZONTAL))), Blocks.WATER.defaultBlockState());
            }
            click(c, random, clicked, side.getOpposite());
        }

        private void placeIntoReplaceable(Case c, Random random, BlockPos target) {
            if (!clickable(target)) {
                placeAgainstNeighbour(c, random, target);
                return;
            }
            c.put(target, pickReplaceable(random));
            addFloor(c, random, target);
            click(c, random, target, pick(random, List.of(DIRECTIONS)));
        }

        /** 台阶、蜡烛、海泡菜这些能叠加的方块：目标格已经有一个同类方块，点它或者点它旁边。 */
        private void placeOntoOwnKind(Case c, Random random, BlockPos target) {
            if (!clickable(target)) {
                placeAgainstNeighbour(c, random, target);
                return;
            }
            c.put(target, randomState(random, chance(random, 70) ? c.subject : pick(random, catalog.familyOf(c.subject))));
            addFloor(c, random, target);
            if (chance(random, 70)) {
                click(c, random, target, pick(random, List.of(DIRECTIONS)));
            } else {
                Direction side = clickableSide(random, target);
                c.put(target.relative(side), pickSolid(random));
                click(c, random, target.relative(side), side.getOpposite());
            }
        }

        private void placeInWater(Case c, Random random, BlockPos target) {
            if (!clickable(target)) {
                placeAgainstNeighbour(c, random, target);
                return;
            }
            c.put(target, chance(random, 80) ? Blocks.WATER.defaultBlockState() : randomState(random, Blocks.WATER));
            SupportInfo info = supports.of(c.subject);
            if (!info.supports.isEmpty()) {
                Support support = pick(random, info.supports);
                c.put(target.relative(support.side), support.state);
                if (chance(random, 50) && clickable(target.relative(support.side))) {
                    click(c, random, target.relative(support.side), support.side.getOpposite());
                    return;
                }
            } else {
                addFloor(c, random, target);
            }
            click(c, random, target, pick(random, List.of(DIRECTIONS)));
        }

        private void placeAnywhere(Case c, Random random, BlockPos target) {
            Direction side = clickableSide(random, target);
            c.put(target.relative(side), neighbour(random, c));
            click(c, random, target.relative(side), side.getOpposite());
        }

        private void addFloor(Case c, Random random, BlockPos target) {
            if (chance(random, 70)) {
                c.put(target.below(), pickSolid(random));
            }
        }

        /** 题目的方块：随机状态，补全多格结构，再选玩家。 */
        private BlockPos placeSubject(Case c, Random random) {
            BlockPos target = originInWorld(random);
            BlockState state = randomState(random, c.subject);
            c.put(target, state);
            complete(c, random, target, state);
            pickActor(c, random, target);
            return target;
        }

        private BlockPos buildUse(Case c, Random random) {
            BlockPos target = placeSubject(c, random);
            if (c.item == Items.ENDER_EYE && c.subject == Blocks.END_PORTAL_FRAME && chance(random, 50)) {
                surroundWithPortalRing(c, random, target);
            }
            addSupports(c, random, target, 60);
            decorate(c, random, target, Set.of(target.asLong()));
            click(c, random, target, pick(random, List.of(DIRECTIONS)));
            holdForUse(c, random);
            return target;
        }

        /**
         * 末影之眼的题：目标是一圈朝里的框架里还没装眼的一个，别的框架大多装好了，装上这一个就可能打开传送门。
         * 圈里的 3×3 由后面的 decorate 随机放东西，打开时要破坏掉。
         */
        private void surroundWithPortalRing(Case c, Random random, BlockPos target) {
            List<BlockPos> slots = new ArrayList<>();
            List<Direction> facings = new ArrayList<>();
            for (int i = 0; i < 3; i++) {
                slots.addAll(List.of(new BlockPos(i, 0, -1), new BlockPos(i, 0, 3), new BlockPos(-1, 0, i), new BlockPos(3, 0, i)));
                facings.addAll(List.of(Direction.SOUTH, Direction.NORTH, Direction.EAST, Direction.WEST));
            }
            int mine = random.nextInt(slots.size());
            BlockPos interior = target.subtract(slots.get(mine));
            for (int i = 0; i < slots.size(); i++) {
                boolean eye = i != mine && chance(random, 92);
                c.put(interior.offset(slots.get(i)), Blocks.END_PORTAL_FRAME.defaultBlockState()
                        .setValue(EndPortalFrameBlock.FACING, facings.get(i)).setValue(EndPortalFrameBlock.HAS_EYE, eye));
            }
        }

        private void holdForUse(Case c, Random random) {
            c.main = c.item != null ? c.item : pickHeld(random, c.subject);
            c.off = chance(random, 30) ? pickHeld(random, c.subject) : Items.AIR;
            if (c.item != null && new ItemStack(c.item).has(DataComponents.BLOCK_TRANSFORMER) && chance(random, 15)) {
                c.off = Items.SHIELD;
            }
            if (chance(random, 30)) {
                c.hand = InteractionHand.OFF_HAND;
                if (c.item != null) {
                    c.off = c.item;
                }
            }
            dropAvoidedItems(c);
        }

        /**
         * 花盆和堆肥桶的题要拿着 --avoid 的植物才比得了方块自己的逻辑，这些物品不能走到放置。
         * 蹲着、用副手或者点中的不是题目的方块时，点击可能轮到物品，放下一个 --avoid 的类，比不出方块的问题。
         */
        private void dropAvoidedItems(Case c) {
            BlockState clicked = c.scene.get(c.hit.getBlockPos().asLong());
            if (!c.sneak && c.hand == InteractionHand.MAIN_HAND && clicked != null && clicked.is(c.subject)) {
                return;
            }
            if (!catalog.allowedInHand(c.main)) {
                c.main = Items.AIR;
            }
            if (!catalog.allowedInHand(c.off)) {
                c.off = Items.AIR;
            }
        }

        private BlockPos buildDestroy(Case c, Random random) {
            BlockPos target = placeSubject(c, random);
            addSupports(c, random, target, 50);
            addDependants(c, random, target);
            decorate(c, random, target, Set.of(target.asLong()));
            c.target = target;
            c.main = chance(random, 60) ? Items.AIR : chance(random, 50) ? pick(random, catalog.heldBlockItems) : pick(random, catalog.tools);
            return target;
        }

        private BlockPos buildTick(Case c, Random random) {
            BlockPos target = placeSubject(c, random);
            addSupports(c, random, target, 50);
            decorate(c, random, target, Set.of(target.asLong()));
            c.target = target;
            return target;
        }

        /** 题目的方块多半是会随机刻的状态，有支撑和零散的邻居，脚下常常有一片地面，长和蔓延都有地方。 */
        private BlockPos buildRandom(Case c, Random random) {
            BlockPos target = originInWorld(random);
            BlockState state = tickingState(random, c.subject);
            c.put(target, state);
            complete(c, random, target, state);
            pickActor(c, random, target);
            addSupports(c, random, target, 60);
            if (chance(random, 50)) {
                growthGround(c, random, target);
            }
            decorate(c, random, target, Set.of(target.asLong()));
            c.target = target;
            return target;
        }

        /** 和 random 题一样的场景，跑官方的 tickChunk，看挑格子的顺序和区块段的判断。 */
        private BlockPos buildChunk(Case c, Random random) {
            BlockPos target = buildRandom(c, random);
            c.speed = CHUNK_SPEEDS[random.nextInt(CHUNK_SPEEDS.length)];
            return target;
        }

        /** 状态里有会随机刻的时候多半挑它们，否则题目大多什么也不做。 */
        private static BlockState tickingState(Random random, Block block) {
            List<BlockState> ticking = block.getStateDefinition().getPossibleStates().stream()
                    .filter(state -> state.isRandomlyTicking() || state.getFluidState().isRandomlyTicking())
                    .toList();
            return ticking.isEmpty() || chance(random, 15) ? randomState(random, block) : pick(random, ticking);
        }

        /** 目标下面一层铺一片 3×3 的地面：草和菌丝的蔓延、茎结瓜、作物看耕地都要它。 */
        private void growthGround(Case c, Random random, BlockPos target) {
            for (int dx = -1; dx <= 1; dx++) {
                for (int dz = -1; dz <= 1; dz++) {
                    BlockPos at = target.offset(dx, -1, dz);
                    if (!c.occupied(at)) {
                        c.put(at, chance(random, 40) && catalog.allowed(Blocks.DIRT) ? Blocks.DIRT.defaultBlockState()
                                : catalog.terrain(random));
                    }
                }
            }
        }

        private void addSupports(Case c, Random random, BlockPos target, int percent) {
            SupportInfo info = supports.of(c.subject);
            if (!info.supports.isEmpty() && chance(random, percent)) {
                Support support = pick(random, info.supports);
                BlockPos at = target.relative(support.side);
                if (!c.occupied(at)) {
                    c.put(at, support.state);
                    if (support.waterBeside) {
                        c.put(at.relative(pick(random, List.of(HORIZONTAL))), Blocks.WATER.defaultBlockState());
                    }
                }
            } else if (chance(random, 40)) {
                c.put(target.below(), pickSolid(random));
            }
        }

        /** 目标周围放上靠它才活得下来的方块，破坏后要看到连锁。 */
        private void addDependants(Case c, Random random, BlockPos target) {
            probe.blocks.clear();
            probe.blocks.putAll(c.scene);
            addDependants(c, random, target, DEPENDANT_LAYERS);
        }

        /** 靠别的方块活着的方块上面还可以有这样的方块，这样才看得到邻居队列分层的顺序。 */
        private void addDependants(Case c, Random random, BlockPos target, int layers) {
            for (Direction side : DIRECTIONS) {
                BlockPos at = target.relative(side);
                if (!c.occupied(at) && chance(random, 45)) {
                    BlockState dependant = findDependant(random, at);
                    if (dependant != null) {
                        c.put(at, dependant);
                        complete(c, random, at, dependant);
                        probe.blocks.putAll(c.scene);
                        if (layers > 1 && chance(random, 60)) {
                            addDependants(c, random, at, layers - 1);
                        }
                    }
                }
            }
        }

        private BlockState findDependant(Random random, BlockPos at) {
            List<Block> needy = supports.needy();
            for (int attempt = 0; attempt < 10 && !needy.isEmpty(); attempt++) {
                BlockState state = randomState(random, pick(random, needy));
                if (survives(state, at)) {
                    return state;
                }
            }
            return null;
        }

        private boolean survives(BlockState state, BlockPos at) {
            try {
                return state.canSurvive(probe, at);
            } catch (RuntimeException ignored) {
                return false;
            }
        }

        /** 把多格结构的另一部分补上。留一部分残缺的，官方代码在残缺时的行为也要比较。 */
        private void complete(Case c, Random random, BlockPos pos, BlockState state) {
            if (chance(random, 15)) {
                return;
            }
            if (state.hasProperty(BlockStateProperties.DOUBLE_BLOCK_HALF)) {
                boolean lower = state.getValue(BlockStateProperties.DOUBLE_BLOCK_HALF) == DoubleBlockHalf.LOWER;
                c.put(lower ? pos.above() : pos.below(), state.setValue(BlockStateProperties.DOUBLE_BLOCK_HALF,
                        lower ? DoubleBlockHalf.UPPER : DoubleBlockHalf.LOWER));
            }
            if (state.hasProperty(BlockStateProperties.BED_PART)) {
                Direction facing = state.getValue(BlockStateProperties.HORIZONTAL_FACING);
                boolean foot = state.getValue(BlockStateProperties.BED_PART) == BedPart.FOOT;
                c.put(pos.relative(foot ? facing : facing.getOpposite()),
                        state.setValue(BlockStateProperties.BED_PART, foot ? BedPart.HEAD : BedPart.FOOT));
            }
            if (state.getBlock() instanceof ChestBlock && state.getValue(ChestBlock.TYPE) != ChestType.SINGLE) {
                c.put(pos.relative(ChestBlock.getConnectedDirection(state)),
                        state.setValue(ChestBlock.TYPE, state.getValue(ChestBlock.TYPE).getOpposite()));
            }
            completePiston(c, pos, state);
        }

        private void completePiston(Case c, BlockPos pos, BlockState state) {
            if (state.getBlock() instanceof PistonBaseBlock && state.getValue(PistonBaseBlock.EXTENDED)) {
                Direction facing = state.getValue(PistonBaseBlock.FACING);
                PistonType type = state.is(Blocks.STICKY_PISTON) ? PistonType.STICKY : PistonType.DEFAULT;
                c.put(pos.relative(facing), Blocks.PISTON_HEAD.defaultBlockState()
                        .setValue(PistonHeadBlock.FACING, facing).setValue(PistonHeadBlock.TYPE, type));
            }
            if (state.getBlock() instanceof PistonHeadBlock) {
                Direction facing = state.getValue(PistonHeadBlock.FACING);
                Block base = state.getValue(PistonHeadBlock.TYPE) == PistonType.STICKY ? Blocks.STICKY_PISTON : Blocks.PISTON;
                c.put(pos.relative(facing.getOpposite()), base.defaultBlockState()
                        .setValue(PistonBaseBlock.FACING, facing).setValue(PistonBaseBlock.EXTENDED, true));
            }
        }

        /** 目标周围六格和稍远的几格，疏密每题不同。reserved 里的格子不放东西。 */
        private void decorate(Case c, Random random, BlockPos target, Set<Long> reserved) {
            int percent = 15 + 20 * random.nextInt(4);
            for (Direction side : DIRECTIONS) {
                BlockPos at = target.relative(side);
                if (!c.occupied(at) && !reserved.contains(at.asLong()) && chance(random, percent)) {
                    putNeighbour(c, random, at);
                }
            }
            int extras = random.nextInt(9);
            for (int i = 0; i < extras; i++) {
                BlockPos at = target.offset(random.nextInt(5) - 2, random.nextInt(5) - 2, random.nextInt(5) - 2);
                if (!c.occupied(at) && !reserved.contains(at.asLong())) {
                    putNeighbour(c, random, at);
                }
            }
        }

        private void putNeighbour(Case c, Random random, BlockPos at) {
            BlockState state = neighbour(random, c);
            c.put(at, state);
            if (chance(random, 50)) {
                complete(c, random, at, state);
            }
        }

        /** 第 4 章的邻居：同家族、实心、半格、能替换、水，还有一小部分什么都行。 */
        private BlockState neighbour(Random random, Case c) {
            int roll = random.nextInt(100);
            if (roll < 36) {
                return aligned(random, familyState(random, c.subject), c);
            }
            if (roll < 56) {
                return pickSolid(random);
            }
            if (roll < 70) {
                return randomState(random, pick(random, catalog.weak));
            }
            if (roll < 79) {
                return pickReplaceable(random);
            }
            if (roll < 87) {
                return chance(random, 70) ? Blocks.WATER.defaultBlockState() : randomState(random, Blocks.WATER);
            }
            return randomState(random, pick(random, catalog.scenery));
        }

        private BlockState familyState(Random random, Block subject) {
            return randomState(random, chance(random, 55) ? subject : pick(random, catalog.familyOf(subject)));
        }

        /** 让邻居的朝向常常和玩家一致，否则箱子合并这种要求朝向相同的分支很难碰到。 */
        private BlockState aligned(Random random, BlockState state, Case c) {
            if (state.hasProperty(BlockStateProperties.HORIZONTAL_FACING) && chance(random, 50)) {
                state = state.setValue(BlockStateProperties.HORIZONTAL_FACING, Direction.fromYRot(c.yaw).getOpposite());
            }
            if (state.getBlock() instanceof ChestBlock && chance(random, 70)) {
                state = state.setValue(ChestBlock.TYPE, ChestType.SINGLE);
            }
            return state;
        }

        private BlockState pickSolid(Random random) {
            int roll = random.nextInt(100);
            if (roll < 60) {
                return catalog.terrain(random);
            }
            return roll < 85 ? pick(random, catalog.sturdy) : randomState(random, pick(random, catalog.weak));
        }

        private BlockState pickReplaceable(Random random) {
            return randomState(random, pick(random, catalog.replaceable));
        }

        private static BlockState randomState(Random random, Block block) {
            if (chance(random, 15)) {
                return block.defaultBlockState();
            }
            return pick(random, block.getStateDefinition().getPossibleStates());
        }

        private Item pickHeld(Random random, Block subject) {
            if (subject instanceof FlowerPotBlock && chance(random, 50)) {
                return pick(random, catalog.pottable);
            }
            if (subject instanceof CakeBlock && chance(random, 50)) {
                return pick(random, catalog.candles);
            }
            if (subject instanceof ComposterBlock && chance(random, 50)) {
                return pick(random, catalog.compostable);
            }
            int roll = random.nextInt(100);
            if (roll < 30) {
                return Items.AIR;
            }
            if (roll < 42 && subject.asItem() != Items.AIR) {
                return subject.asItem();
            }
            if (roll < 62) {
                return pick(random, catalog.tools);
            }
            if (roll < 74) {
                return pick(random, pick(random, catalog.interactive));
            }
            return roll < 92 ? pick(random, catalog.heldBlockItems) : pick(random, catalog.heldItems);
        }

        private boolean clickable(BlockPos pos) {
            return height.contains(pos);
        }

        private static Direction pickSide(Random random) {
            int roll = random.nextInt(100);
            if (roll < 40) {
                return Direction.DOWN;
            }
            return roll < 80 ? pick(random, List.of(HORIZONTAL)) : Direction.UP;
        }

        private Direction clickableSide(Random random, BlockPos target) {
            Direction side = pick(random, List.of(DIRECTIONS));
            return clickable(target.relative(side)) ? side : towardsWorld(target);
        }

        private Direction towardsWorld(BlockPos target) {
            return target.getY() >= height.max ? Direction.DOWN : Direction.UP;
        }

        /** 世界顶部和底部各占一部分，还有点在世界最外一层的面上、目标格已经出界的情况。 */
        private BlockPos origin(Random random) {
            int roll = random.nextInt(100);
            int y;
            if (roll < 6) {
                y = height.max;
            } else if (roll < 9) {
                y = height.max - 1;
            } else if (roll < 12) {
                y = height.max + 1;
            } else if (roll < 18) {
                y = height.min;
            } else if (roll < 21) {
                y = height.min + 1;
            } else if (roll < 24) {
                y = height.min - 1;
            } else {
                y = 60 + random.nextInt(9);
            }
            return new BlockPos(random.nextInt(64) - 32, y, random.nextInt(64) - 32);
        }

        /** 方块本身要在世界里的题目用这个。 */
        private BlockPos originInWorld(Random random) {
            BlockPos origin = origin(random);
            return origin.atY(Math.clamp(origin.getY(), height.min, height.max));
        }

        private void pickActor(Case c, Random random, BlockPos target) {
            c.yaw = pickYaw(random);
            c.pitch = pickPitch(random);
            c.sneak = chance(random, 30);
            c.actor = chance(random, 62) ? farPosition(random, target) : blockingPosition(random, target);
            if (chance(random, 10)) {
                c.bystander = blockingPosition(random, target);
            }
        }

        /** 服务端收到的 yaw 已经 wrapDegrees 到 [-180, 180)。 */
        private static float pickYaw(Random random) {
            int roll = random.nextInt(100);
            float yaw;
            if (roll < 30) {
                yaw = random.nextInt(8) * 45.0F - 180.0F;
            } else if (roll < 50) {
                float boundary = random.nextInt(8) * 45.0F - 180.0F;
                yaw = random.nextBoolean() ? Math.nextUp(boundary) : Math.nextDown(boundary);
            } else {
                yaw = random.nextFloat() * 360.0F - 180.0F;
            }
            return yaw < -180.0F ? -180.0F : yaw;
        }

        private static float pickPitch(Random random) {
            if (chance(random, 30)) {
                return LOOK_PITCHES[random.nextInt(LOOK_PITCHES.length)];
            }
            return random.nextFloat() * 180.0F - 90.0F;
        }

        private static Vec3 farPosition(Random random, BlockPos target) {
            double angle = random.nextDouble() * Math.PI * 2.0;
            double distance = 2.0 + random.nextDouble() * 3.0;
            return new Vec3(target.getX() + 0.5 + Math.cos(angle) * distance, target.getY() - 2 + random.nextInt(5),
                    target.getZ() + 0.5 + Math.sin(angle) * distance);
        }

        /** 站在目标格、半格、刚好贴着、一格外，脚或者头碰到目标格。 */
        private static Vec3 blockingPosition(Random random, BlockPos target) {
            double x = target.getX() + 0.5;
            double y = target.getY();
            double z = target.getZ() + 0.5;
            double sign = random.nextBoolean() ? 1.0 : -1.0;
            return switch (random.nextInt(6)) {
                case 0 -> new Vec3(x, y, z);
                case 1 -> new Vec3(x + 0.5 * sign, y, z + (random.nextDouble() - 0.5) * 0.5);
                case 2 -> new Vec3(x + 0.8 * sign, y, z);
                case 3 -> new Vec3(x, y, z + 1.0 * sign);
                case 4 -> new Vec3(x + (random.nextDouble() - 0.5), y - 1.0, z + (random.nextDouble() - 0.5));
                default -> new Vec3(x + (random.nextDouble() - 0.5), y + 1.0, z + (random.nextDouble() - 0.5));
            };
        }

        private static void click(Case c, Random random, BlockPos pos, Direction face) {
            c.hit = new BlockHitResult(location(random, pos, face), face, pos, chance(random, 5));
        }

        /** 和官方解码一样：客户端发 float 的光标，服务端用 double 加上方块坐标。 */
        private static Vec3 location(Random random, BlockPos pos, Direction face) {
            float x = cursor(random);
            float y = cursor(random);
            float z = cursor(random);
            float surface = face.getAxisDirection() == Direction.AxisDirection.POSITIVE ? 1.0F : 0.0F;
            if (chance(random, 12)) {
                surface = INSET_SURFACES[random.nextInt(INSET_SURFACES.length)];
            }
            switch (face.getAxis()) {
                case X -> x = surface;
                case Y -> y = surface;
                case Z -> z = surface;
            }
            return new Vec3((double) pos.getX() + x, (double) pos.getY() + y, (double) pos.getZ() + z);
        }

        private static float cursor(Random random) {
            return chance(random, 30) ? CURSORS[random.nextInt(CURSORS.length)] : random.nextFloat();
        }
    }

    /** 从 --only 算出每种题目的题干，再按顺序轮转，保证每个方块和物品都轮得到。 */
    private static final class Subjects {
        private static final int RARE_CLASS_COPIES = 6;
        private static final int PROBE_ALL_STATES = 64;
        private static final int PROBE_SAMPLES = 16;
        private static final Kind[] PATTERN = {Kind.PLACE, Kind.USE, Kind.PLACE, Kind.DESTROY, Kind.USE, Kind.PLACE, Kind.TICK, Kind.USE,
                Kind.FLOW, Kind.ITEM, Kind.RANDOM, Kind.CHUNK, Kind.RANDOM};

        private final Map<Kind, List<Subject>> byKind = new LinkedHashMap<>();
        private final List<Kind> pattern = new ArrayList<>();
        private final Map<Kind, Integer> turns = new HashMap<>();

        Subjects(Official official, Catalog catalog, String only, long seed) {
            Random random = new Random(seed);
            List<Subject> placing = new ArrayList<>();
            List<Subject> others = new ArrayList<>();
            List<Subject> held = new ArrayList<>();
            for (BlockItem item : catalog.blockItems) {
                if (only == null || matches(only, item.getBlock()) || matches(only, item)) {
                    placing.add(new Subject(item.getBlock(), item));
                }
            }
            for (Block block : catalog.blocks) {
                if (only == null || matches(only, block) || matches(only, block.asItem())) {
                    others.add(new Subject(block, null));
                }
            }
            for (Item item : catalog.items) {
                if (only != null && !(item instanceof BlockItem) && matches(only, item)) {
                    held.add(new Subject(null, item));
                }
            }
            List<Subject> usable = new ArrayList<>();
            for (Item item : catalog.usable) {
                if (only != null && matches(only, item)) {
                    usable.add(new Subject(null, item));
                }
            }
            placing = weighted(placing, random);
            others = weighted(others, random);
            List<Subject> wet = others.stream().filter(subject -> Catalog.holdsFluid(subject.block)).toList();
            List<Subject> ticking = others.stream().filter(subject -> Catalog.ticksRandomly(subject.block)).toList();
            List<Subject> using = usable.isEmpty() && held.isEmpty() ? withUsableItems(others, catalog, random)
                    : heldOnBlocks(usable, catalog, official, random);
            if (!held.isEmpty()) {
                others.addAll(heldOnBlocks(held, catalog, official, random));
            }
            byKind.put(Kind.PLACE, placing);
            byKind.put(Kind.USE, others);
            if (held.isEmpty()) {
                byKind.put(Kind.DESTROY, others);
                byKind.put(Kind.TICK, others);
                byKind.put(Kind.FLOW, wet);
                byKind.put(Kind.RANDOM, ticking);
                byKind.put(Kind.CHUNK, ticking);
            }
            byKind.put(Kind.ITEM, using);
            for (Kind kind : PATTERN) {
                if (!byKind.getOrDefault(kind, List.of()).isEmpty()) {
                    pattern.add(kind);
                }
            }
            if (pattern.isEmpty()) {
                throw new IllegalArgumentException("--only " + only + " matches no block, item or class");
            }
        }

        /** 每个方块配一个用 use_item 的物品，玩家看着这个方块用它。 */
        private static List<Subject> withUsableItems(List<Subject> blocks, Catalog catalog, Random random) {
            List<Subject> subjects = new ArrayList<>();
            for (Subject subject : blocks) {
                subjects.add(new Subject(subject.block, pick(random, catalog.usable)));
            }
            return subjects;
        }

        /**
         * 不能放置的物品：每个方块都轮到一次，它会改的方块再补到和全部方块一样多。
         * 不然锄头这种只改几个方块的物品，几万题里只有几十题真的耕了地。
         */
        private static List<Subject> heldOnBlocks(List<Subject> held, Catalog catalog, Official official, Random random) {
            List<Subject> subjects = new ArrayList<>();
            for (Subject subject : held) {
                for (Block block : catalog.blocks) {
                    subjects.add(new Subject(block, subject.item));
                }
                List<Block> targets = catalog.blocks.stream().filter(block -> changesClickedBlock(official, subject.item, block)).toList();
                for (int i = 0; !targets.isEmpty() && i < catalog.blocks.size(); i++) {
                    subjects.add(new Subject(targets.get(i % targets.size()), subject.item));
                }
            }
            Collections.shuffle(subjects, random);
            return subjects;
        }

        /** 蹲着拿这个物品点方块的顶面，上面是空气。试的状态里有一个被改了就算。 */
        private static boolean changesClickedBlock(Official official, Item item, Block block) {
            BlockPos pos = new BlockPos(0, 64, 0);
            BlockHitResult hit = new BlockHitResult(new Vec3(0.5, 65.0, 0.5), Direction.UP, pos, false);
            for (BlockState state : probeStates(block)) {
                FakeLevel level = FakeLevel.create(official, new OracleRandom(OracleRandom.Mode.LOW, 0));
                Long2ObjectOpenHashMap<BlockState> scene = new Long2ObjectOpenHashMap<>();
                scene.put(pos.asLong(), state);
                level.load(scene);
                FakePlayer player = FakePlayer.create(level, official, level.getRandom(), 1, new Vec3(3.5, 64.0, 0.5), 90.0F, 30.0F, true);
                player.hold(item, Items.AIR);
                try {
                    player.gameMode.useItemOn(player, level, player.getMainHandItem(), InteractionHand.MAIN_HAND, hit);
                } catch (RuntimeException ignored) {
                    // 这里只是在挑方块，官方代码在孤零零的方块上抛异常时不挑它。
                    continue;
                }
                if (level.getBlockState(pos) != state) {
                    return true;
                }
            }
            return false;
        }

        /** 状态不多的方块全试，营火要“没点着、没泡水”的状态才挑得出来。状态多的均匀挑一些。 */
        private static List<BlockState> probeStates(Block block) {
            List<BlockState> states = block.getStateDefinition().getPossibleStates();
            if (states.size() <= PROBE_ALL_STATES) {
                return states;
            }
            List<BlockState> picked = new ArrayList<>();
            for (int i = 0; i < PROBE_SAMPLES; i++) {
                picked.add(states.get(i * states.size() / PROBE_SAMPLES));
            }
            return picked;
        }

        /** 一个类的方块很少时多给几次机会，否则只有一个方块的类几乎轮不到。 */
        private static List<Subject> weighted(List<Subject> subjects, Random random) {
            Map<Class<?>, List<Subject>> byClass = new LinkedHashMap<>();
            for (Subject subject : subjects) {
                byClass.computeIfAbsent(subject.block.getClass(), type -> new ArrayList<>()).add(subject);
            }
            List<Subject> result = new ArrayList<>(subjects);
            for (List<Subject> group : byClass.values()) {
                int states = group.getFirst().block.getStateDefinition().getPossibleStates().size();
                int copies = RARE_CLASS_COPIES + Integer.SIZE - Integer.numberOfLeadingZeros(states);
                for (int i = group.size(); i < copies; i++) {
                    result.add(pick(random, group));
                }
            }
            result.sort(Comparator.comparing(subject -> id(subject.block)));
            Collections.shuffle(result, random);
            return result;
        }

        private static boolean matches(String only, Block block) {
            return only.equals(id(block)) || ("minecraft:" + only).equals(id(block)) || classMatches(only, block.getClass());
        }

        private static boolean matches(String only, Item item) {
            return item != Items.AIR
                    && (only.equals(id(item)) || ("minecraft:" + only).equals(id(item)) || classMatches(only, item.getClass()));
        }

        private static boolean classMatches(String only, Class<?> type) {
            for (Class<?> current = type; current != null; current = current.getSuperclass()) {
                if (current.getSimpleName().equals(only)) {
                    return true;
                }
            }
            return false;
        }

        Kind kind(int id) {
            return pattern.get(id % pattern.size());
        }

        Subject next(Kind kind) {
            List<Subject> subjects = byKind.get(kind);
            int turn = turns.merge(kind, 1, Integer::sum) - 1;
            return subjects.get(turn % subjects.size());
        }
    }

    /** 一个方块类的统计。 */
    private static final class ClassStats {
        final int[] cases = new int[Kind.values().length];
        final int[] changed = new int[Kind.values().length];
        final Map<String, Integer> skips = new TreeMap<>();
        final Map<String, Integer> placeResults = new TreeMap<>();
        int placed;
    }

    /** 跑题、写答案、做汇总。 */
    private static final class Oracle {
        private static final char[] HEX = "0123456789abcdef".toCharArray();

        private final Official official;
        private final Options options;
        private final Writer out;
        private final PrintStream log;
        private final Catalog catalog;
        private final Supports supports;
        private final Generator generator;
        private final Subjects subjects;
        private final StringBuilder line = new StringBuilder(4096);
        private final Map<String, ClassStats> stats = new TreeMap<>();
        private final Map<String, Integer> exceptions = new TreeMap<>();
        private final Map<String, Integer> skipReasons = new TreeMap<>();
        private final Map<String, Integer> cosmeticRandom = new TreeMap<>();
        private final Map<String, Integer> decisiveRandom = new TreeMap<>();
        private final int[] kinds = new int[Kind.values().length];
        private int written;
        private int skipped;
        private int changedWorld;

        Oracle(Official official, Options options, Writer out, PrintStream log) throws ReflectiveOperationException {
            this.official = official;
            this.options = options;
            this.out = out;
            this.log = log;
            pinFluidShapes(official);
            this.catalog = new Catalog(options.avoid);
            this.supports = new Supports(official, catalog);
            this.generator = new Generator(official, catalog, supports);
            this.subjects = new Subjects(official, catalog, options.only, options.seed);
        }

        /**
         * FlowingFluid.getShape 按流体状态缓存形状，高度是进程里第一次调用时那个位置算出来的。先在上面没有同种流体的位置
         * 调一次，水源和岩浆源的形状就固定成它们自己的高度，和 mc-bamboo-blocks 的视线判定一致。
         */
        private static void pinFluidShapes(Official official) {
            FakeLevel level = FakeLevel.create(official, new OracleRandom(OracleRandom.Mode.LOW, 0));
            BlockPos pos = new BlockPos(0, 64, 0);
            for (Block liquid : List.of(Blocks.WATER, Blocks.LAVA)) {
                level.blocks.clear();
                level.blocks.put(pos.asLong(), liquid.defaultBlockState());
                level.getFluidState(pos).getShape(level, pos);
            }
        }

        /** 显式活塞题的 id 从这里开始，和随机题的号段分开。 */
        private static final int PISTON_CASE_BASE = 1_000_000;
        /** 显式漏斗题的 id 从这里开始。 */
        private static final int HOPPER_CASE_BASE = 2_000_000;

        void run() throws IOException {
            long start = System.nanoTime();
            List<Case> pistonCases = generator.pistonCases(PISTON_CASE_BASE);
            List<Case> hopperCases = generator.hopperCases(HOPPER_CASE_BASE);
            boolean single = options.single >= 0;
            boolean pistons = single ? options.single >= PISTON_CASE_BASE && options.single < HOPPER_CASE_BASE
                    : includesPistonCases();
            boolean hoppers = single ? options.single >= HOPPER_CASE_BASE : includesHopperCases();
            writeHeader(single ? 1 : options.cases
                    + (pistons ? pistonCases.size() : 0)
                    + (hoppers ? hopperCases.size() : 0));
            int first = Math.max(options.single, 0);
            int end = single && options.single < PISTON_CASE_BASE ? options.single + 1 : options.cases;
            for (int id = 0; id < end; id++) {
                Kind kind = subjects.kind(id);
                Subject subject = subjects.next(kind);
                if (id >= first) {
                    runCase(id, kind, subject);
                }
            }
            if (pistons) {
                for (Case c : pistonCases) {
                    if (!single || c.id == options.single) {
                        runBuilt(c, statsOf(c.subject));
                    }
                }
            }
            if (hoppers) {
                for (Case c : hopperCases) {
                    if (!single || c.id == options.single) {
                        runBuilt(c, statsOf(c.subject));
                    }
                }
            }
            writeEnd();
            out.flush();
            printSummary(System.nanoTime() - start);
        }

        /** --only 点名的是活塞、活塞头、移动活塞或者它们的类时，显式活塞题也要跑。 */
        private boolean includesPistonCases() {
            if (options.only == null) {
                return true;
            }
            for (Block block : List.of(Blocks.PISTON, Blocks.STICKY_PISTON, Blocks.MOVING_PISTON, Blocks.PISTON_HEAD)) {
                if (Subjects.matches(options.only, block)) {
                    return true;
                }
            }
            return false;
        }

        /** --only 点名的是漏斗、投掷器、发射器或那几种容器时，显式漏斗题也要跑。 */
        private boolean includesHopperCases() {
            if (options.only == null) {
                return true;
            }
            for (Block block : List.of(Blocks.HOPPER, Blocks.DROPPER, Blocks.DISPENSER, Blocks.CHEST, Blocks.TRAPPED_CHEST,
                    Blocks.BARREL, Blocks.SHULKER_BOX, Blocks.COMPARATOR)) {
                if (Subjects.matches(options.only, block)) {
                    return true;
                }
            }
            return false;
        }

        private void runCase(int id, Kind kind, Subject subject) throws IOException {
            runBuilt(generator.build(id, kind, subject, options.seed), statsOf(subject.block));
        }

        private void runBuilt(Case c, ClassStats classStats) throws IOException {
            classStats.cases[c.kind.ordinal()]++;
            kinds[c.kind.ordinal()]++;
            try {
                Outcome outcome = answer(c);
                record(c, outcome, classStats);
                writeCase(c, outcome);
            } catch (SkipCase skip) {
                skip(c, classStats, skip.reason);
            } catch (Throwable error) {
                Throwable cause = rootCause(error);
                if (cause instanceof SkipCase skip) {
                    skip(c, classStats, skip.reason);
                } else if (readsMissingProperty(cause)) {
                    skip(c, classStats, "impossible:" + siteOf(cause));
                } else {
                    recordException(c, cause);
                    skip(c, classStats, "exception:" + cause.getClass().getSimpleName());
                }
            }
        }

        /** 比如小型垂滴叶的下半上面不是上半，骨粉直接读那一格的 half。题目故意留了一些残缺的结构，见 complete。 */
        private static boolean readsMissingProperty(Throwable error) {
            return error instanceof IllegalArgumentException && String.valueOf(error.getMessage()).startsWith("Cannot get property");
        }

        private static String siteOf(Throwable error) {
            return Arrays.stream(error.getStackTrace())
                    .filter(frame -> frame.getClassName().startsWith("net.minecraft.") && !frame.getClassName().endsWith(".StateHolder"))
                    .findFirst()
                    .map(frame -> frame.getClassName().substring(frame.getClassName().lastIndexOf('.') + 1) + "." + frame.getMethodName())
                    .orElse("unknown");
        }

        private static Throwable rootCause(Throwable error) {
            Throwable cause = error;
            while (cause.getCause() != null && cause.getCause() != cause) {
                cause = cause.getCause();
            }
            return cause;
        }

        private void recordException(Case c, Throwable error) {
            StackTraceElement site = Arrays.stream(error.getStackTrace())
                    .filter(frame -> frame.getClassName().startsWith("net.minecraft.") || frame.getClassName().startsWith("BlockOracle"))
                    .findFirst().orElse(null);
            String where = site == null ? "unknown" : site.getClassName().substring(site.getClassName().lastIndexOf('.') + 1) + "."
                    + site.getMethodName();
            String key = error.getClass().getSimpleName() + " at " + where + ": " + error.getMessage();
            if (exceptions.merge(key.length() > 200 ? key.substring(0, 200) : key, 1, Integer::sum) == 1) {
                log.println("first exception of its kind, case " + c.id + " " + c.kind.label + " " + id(c.subject));
                error.printStackTrace(log);
            }
        }

        private void skip(Case c, ClassStats classStats, String reason) throws IOException {
            skipped++;
            skipReasons.merge(reason, 1, Integer::sum);
            classStats.skips.merge(c.kind.label + ":" + reason, 1, Integer::sum);
            line.setLength(0);
            line.append("{\"type\":\"skip\",\"id\":").append(c.id);
            line.append(",\"kind\":\"").append(c.kind.label).append('"');
            line.append(",\"block\":\"").append(id(c.subject)).append('"');
            line.append(",\"class\":\"").append(className(c.subject)).append('"');
            line.append(",\"reason\":\"").append(reason).append("\"}\n");
            out.append(line);
        }

        /** 世界随机源的种子，写在 case 行里，mc-bamboo-blocks 用它重现同样的随机数。 */
        private long caseSeed(Case c) {
            return options.seed * 31 + c.id;
        }

        /**
         * 跑一次。世界的随机源和 mc-bamboo-blocks 用同一个种子，结果直接比较。玩家自己的随机源 Bamboo 没有，
         * 它碰过的题换几种随机数重跑，世界的结果变了就排除。
         */
        private Outcome answer(Case c) {
            long seed = caseSeed(c);
            OracleRandom levelRandom = new OracleRandom(OracleRandom.Mode.SEEDED, seed);
            OracleRandom entityRandom = new OracleRandom(OracleRandom.Mode.SEEDED, -seed);
            Outcome outcome = execute(c, levelRandom, entityRandom);
            outcome.touchedRandom = levelRandom.draws > 0 || entityRandom.draws > 0;
            if (entityRandom.draws > 0) {
                List<OracleRandom> variants = List.of(new OracleRandom(OracleRandom.Mode.LOW, 0),
                        new OracleRandom(OracleRandom.Mode.HIGH, 0), new OracleRandom(OracleRandom.Mode.SEEDED, ~seed));
                for (OracleRandom variant : variants) {
                    Outcome other = execute(c, new OracleRandom(OracleRandom.Mode.SEEDED, seed), variant);
                    if (!outcome.sameWorld(other)) {
                        entityRandom.sites.forEach(site -> decisiveRandom.merge(site, 1, Integer::sum));
                        throw new SkipCase("random-entity:" + String.join("+", entityRandom.sites));
                    }
                    outcome.maskRandomDelays(other);
                }
            }
            levelRandom.sites.forEach(site -> cosmeticRandom.merge(site, 1, Integer::sum));
            entityRandom.sites.forEach(site -> cosmeticRandom.merge(site, 1, Integer::sum));
            return outcome;
        }

        private Outcome execute(Case c, OracleRandom levelRandom, OracleRandom entityRandom) {
            FakeLevel level = FakeLevel.create(official, levelRandom);
            level.seedPositions(new LegacyRandomSource(~caseSeed(c)).nextInt());
            level.load(c.scene, c.contents);
            FakePlayer actor = FakePlayer.create(level, official, entityRandom, 1, c.actor, c.yaw, c.pitch, c.sneak);
            actor.hold(c.main, c.off);
            c.actorBox = actor.getBoundingBox();
            if (c.bystander != null) {
                c.bystanderBox = FakePlayer.create(level, official, entityRandom, 2, c.bystander, 0.0F, 0.0F, false).getBoundingBox();
            }
            List<ItemStack> before = inventoryOf(actor);
            String result = switch (c.kind) {
                case PLACE, USE -> resultName(actor.gameMode.useItemOn(actor, level, actor.getItemInHand(c.hand), c.hand, c.hit));
                case DESTROY -> actor.gameMode.destroyBlock(c.target) ? "destroyed" : "refused";
                case TICK -> {
                    level.getBlockState(c.target).tick(level, c.target, levelRandom);
                    yield "ticked";
                }
                case FLOW -> {
                    level.flow(c.target, c.steps, options.untick);
                    yield "ticked";
                }
                case RANDOM -> {
                    level.randomTick(c.target);
                    yield "ticked";
                }
                case CHUNK -> {
                    level.tickChunk(c.target, c.speed, options.unrandom);
                    yield "ticked";
                }
                case ITEM -> resultName(actor.gameMode.useItem(actor, level, actor.getItemInHand(c.hand), c.hand));
            };
            level.settle(c.settle);
            List<String> gained = c.kind == Kind.ITEM ? gainedItems(before, inventoryOf(actor)) : List.of();
            return new Outcome(result, level, c, actor.reachedItem, gained);
        }

        private static List<ItemStack> inventoryOf(FakePlayer player) {
            Inventory inventory = player.getInventory();
            List<ItemStack> slots = new ArrayList<>();
            for (int slot = 0; slot < inventory.getContainerSize(); slot++) {
                slots.add(inventory.getItem(slot).copy());
            }
            return slots;
        }

        /** 物品栏里新出现的物品：创造模式用空桶舀起东西时，官方把装满的桶放进物品栏。 */
        private static List<String> gainedItems(List<ItemStack> before, List<ItemStack> after) {
            List<String> gained = new ArrayList<>();
            for (int slot = 0; slot < after.size(); slot++) {
                ItemStack now = after.get(slot);
                if (!now.isEmpty() && !ItemStack.matches(before.get(slot), now)) {
                    gained.add(id(now.getItem()));
                }
            }
            return gained;
        }

        private static String resultName(InteractionResult result) {
            return switch (result) {
                case InteractionResult.Success success -> switch (success.swingSource()) {
                    case PREDICTED -> "success";
                    case SERVER_ONLY -> "success_server";
                    case NONE -> "consume";
                };
                case InteractionResult.Fail fail -> "fail";
                case InteractionResult.Pass pass -> "pass";
                case InteractionResult.TryEmptyHandInteraction tryEmpty -> "try_empty_hand";
            };
        }

        private ClassStats statsOf(Block block) {
            return stats.computeIfAbsent(className(block), name -> new ClassStats());
        }

        private void record(Case c, Outcome outcome, ClassStats classStats) {
            if (!outcome.changes.isEmpty()) {
                classStats.changed[c.kind.ordinal()]++;
                changedWorld++;
            }
            if (c.kind == Kind.PLACE) {
                classStats.placeResults.merge(outcome.result, 1, Integer::sum);
                recordPlaced(c, outcome);
            }
        }

        /** 点击的格子或者它旁边的格子变成了新方块才算放下。墙上的火把算在 WallTorchBlock 头上。 */
        private void recordPlaced(Case c, Outcome outcome) {
            BlockPos clicked = c.hit.getBlockPos();
            for (BlockPos pos : List.of(clicked, clicked.relative(c.hit.getDirection()))) {
                BlockState placed = outcome.changes.get(pos.asLong());
                BlockState before = c.scene.get(pos.asLong());
                if (placed != null && !placed.isAir() && (before == null || !before.is(placed.getBlock()))) {
                    statsOf(placed.getBlock()).placed++;
                }
            }
        }

        private void writeHeader(int cases) throws IOException {
            line.setLength(0);
            line.append("{\"type\":\"header\",\"version\":\"").append(SharedConstants.getCurrentVersion().id()).append('"');
            line.append(",\"protocol\":").append(SharedConstants.getCurrentVersion().protocolVersion());
            line.append(",\"seed\":").append(options.seed);
            line.append(",\"cases\":").append(cases);
            line.append(",\"maxChainedUpdates\":").append(MAX_CHAINED_UPDATES);
            line.append(",\"gameTime\":").append(GAME_TIME);
            line.append(",\"minY\":").append(generator.height.min).append(",\"maxY\":").append(generator.height.max).append("}\n");
            out.append(line);
        }

        private void writeEnd() throws IOException {
            line.setLength(0);
            line.append("{\"type\":\"end\",\"cases\":").append(written).append(",\"skipped\":").append(skipped).append("}\n");
            out.append(line);
        }

        private void writeCase(Case c, Outcome outcome) throws IOException {
            written++;
            line.setLength(0);
            line.append("{\"type\":\"case\",\"id\":").append(c.id);
            line.append(",\"seed\":").append(caseSeed(c));
            line.append(",\"block\":\"").append(id(c.subject)).append('"');
            line.append(",\"class\":\"").append(className(c.subject)).append('"');
            line.append(",\"scene\":");
            positions(c.scene);
            line.append(",\"contents\":");
            sceneContents(c.contents);
            writeAction(c);
            writeOutcome(outcome);
            if (options.explain) {
                explain(c, outcome);
            }
            line.append("}\n");
            out.append(line);
        }

        private void writeAction(Case c) {
            line.append(",\"action\":{\"kind\":\"").append(c.kind.label).append('"');
            line.append(",\"main\":\"").append(id(c.main)).append("\",\"off\":\"").append(id(c.off)).append('"');
            line.append(",\"hand\":\"").append(c.hand == InteractionHand.MAIN_HAND ? "main" : "off").append('"');
            Item used = c.hand == InteractionHand.MAIN_HAND ? c.main : c.off;
            line.append(",\"itemClass\":\"").append(used.getClass().getSimpleName()).append('"');
            line.append(",\"itemUseOn\":").append(Catalog.hasOwnUseOn(used));
            if (c.hit != null) {
                BlockPos pos = c.hit.getBlockPos();
                line.append(",\"pos\":");
                pos(pos);
                line.append(",\"face\":\"").append(c.hit.getDirection().getName()).append('"');
                Vec3 location = c.hit.getLocation();
                line.append(",\"loc\":[");
                hex(location.x);
                line.append(',');
                hex(location.y);
                line.append(',');
                hex(location.z);
                line.append("],\"inside\":").append(c.hit.isInside());
            } else {
                line.append(",\"pos\":");
                pos(c.target);
            }
            if (c.kind == Kind.FLOW) {
                line.append(",\"steps\":").append(c.steps);
            }
            if (c.kind == Kind.CHUNK) {
                line.append(",\"speed\":").append(c.speed);
            }
            writeActor(c);
            line.append(",\"settle\":").append(c.settle);
            line.append('}');
        }

        private void writeActor(Case c) {
            line.append(",\"actor\":[");
            hex(c.actor.x);
            line.append(',');
            hex(c.actor.y);
            line.append(',');
            hex(c.actor.z);
            line.append("],\"yaw\":");
            hex(c.yaw);
            line.append(",\"pitch\":");
            hex(c.pitch);
            line.append(",\"sneak\":").append(c.sneak);
            line.append(",\"box\":");
            box(c.actorBox);
            line.append(",\"others\":[");
            if (c.bystander != null) {
                box(c.bystanderBox);
            }
            line.append(']');
        }

        private void writeOutcome(Outcome outcome) {
            line.append(",\"outcome\":{\"result\":\"").append(outcome.result).append('"');
            line.append(",\"changes\":");
            positions(outcome.changes);
            line.append(",\"destroyed\":[");
            for (int i = 0; i < outcome.destroyed.size(); i++) {
                Destroyed event = outcome.destroyed.get(i);
                if (i > 0) {
                    line.append(',');
                }
                line.append('[').append(event.pos.getX()).append(',').append(event.pos.getY()).append(',').append(event.pos.getZ());
                line.append(',').append(event.state).append(',').append(event.initiatorExcluded).append(']');
            }
            line.append("],\"levelEvents\":[");
            for (int i = 0; i < outcome.levelEvents.size(); i++) {
                LevelPlay event = outcome.levelEvents.get(i);
                if (i > 0) {
                    line.append(',');
                }
                line.append('[').append(event.type).append(',').append(event.pos.getX()).append(',').append(event.pos.getY());
                line.append(',').append(event.pos.getZ()).append(',').append(event.data).append(',').append(event.excluded).append(']');
            }
            line.append("],\"sounds\":[");
            for (int i = 0; i < outcome.sounds.size(); i++) {
                SoundPlay sound = outcome.sounds.get(i);
                if (i > 0) {
                    line.append(',');
                }
                line.append('[').append(sound.id).append(',');
                hex(sound.x);
                line.append(',');
                hex(sound.y);
                line.append(',');
                hex(sound.z);
                line.append(',').append(sound.source).append(',');
                hex(sound.volume);
                line.append(',');
                hex(sound.pitch);
                line.append(',').append(sound.excluded).append(']');
            }
            line.append("],\"contents\":");
            outcomeContents(outcome.contents);
            line.append(",\"ticks\":[");            for (int i = 0; i < outcome.ticks.size(); i++) {
                ScheduledBlockTick tick = outcome.ticks.get(i);
                if (i > 0) {
                    line.append(',');
                }
                line.append('[').append(tick.pos.getX()).append(',').append(tick.pos.getY()).append(',').append(tick.pos.getZ());
                line.append(",\"").append(tick.fluid ? "fluid" : "block").append("\",\"").append(tick.type).append("\",");
                line.append(tick.delay).append(',').append(tick.priority).append(']');
            }
            line.append("],\"ran\":[");
            for (int i = 0; i < outcome.ran.size(); i++) {
                line.append(i > 0 ? ",\"" : "\"").append(outcome.ran.get(i)).append('"');
            }
            line.append(']');
            line.append(",\"analogContent\":").append(outcome.analogContent);
            line.append(",\"itemReached\":").append(outcome.itemReached);
            strings("gained", outcome.gained);
            strings("spawned", outcome.spawned);
            line.append(",\"rng\":").append(outcome.touchedRandom).append('}');
        }

        private void strings(String name, List<String> values) {
            line.append(",\"").append(name).append("\":[");
            for (int i = 0; i < values.size(); i++) {
                line.append(i > 0 ? ",\"" : "\"").append(values.get(i)).append('"');
            }
            line.append(']');
        }

        private void positions(Long2ObjectMap<BlockState> states) {
            long[] keys = states.keySet().toLongArray();
            Arrays.sort(keys);
            line.append('[');
            for (int i = 0; i < keys.length; i++) {
                if (i > 0) {
                    line.append(',');
                }
                line.append('[').append(BlockPos.getX(keys[i])).append(',').append(BlockPos.getY(keys[i])).append(',');
                line.append(BlockPos.getZ(keys[i])).append(',').append(Block.getId(states.get(keys[i]))).append(']');
            }
            line.append(']');
        }

        private void pos(BlockPos pos) {
            line.append('[').append(pos.getX()).append(',').append(pos.getY()).append(',').append(pos.getZ()).append(']');
        }

        /** 场景里容器内容的 JSON，按坐标和槽位排序。 */
        private void sceneContents(Long2ObjectMap<NonNullList<ItemStack>> contents) {
            long[] keys = contents.keySet().toLongArray();
            Arrays.sort(keys);
            line.append('[');
            boolean first = true;
            for (long key : keys) {
                NonNullList<ItemStack> stacks = contents.get(key);
                for (int slot = 0; slot < stacks.size(); slot++) {
                    ItemStack stack = stacks.get(slot);
                    if (!stack.isEmpty()) {
                        first = containerStack(first, key, slot, stack);
                    }
                }
            }
            line.append(']');
        }

        /** 答案里容器内容的 JSON。列表在收集时已经按坐标和槽位排好。 */
        private void outcomeContents(List<ContainerStack> contents) {
            line.append('[');
            for (int i = 0; i < contents.size(); i++) {
                ContainerStack stack = contents.get(i);
                if (i > 0) {
                    line.append(',');
                }
                containerStack(true, stack.pos, stack.slot, stack.item, stack.count);
            }
            line.append(']');
        }

        private boolean containerStack(boolean first, long pos, int slot, ItemStack stack) {
            return containerStack(first, pos, slot, id(stack.getItem()), stack.getCount());
        }

        private boolean containerStack(boolean first, long pos, int slot, String item, int count) {
            if (!first) {
                line.append(',');
            }
            line.append('[').append(BlockPos.getX(pos)).append(',').append(BlockPos.getY(pos)).append(',');
            line.append(BlockPos.getZ(pos)).append(',').append(slot).append(",\"").append(item).append("\",");
            line.append(count).append(']');
            return false;
        }

        private void box(AABB box) {
            line.append('[');
            hex(box.minX);
            line.append(',');
            hex(box.minY);
            line.append(',');
            hex(box.minZ);
            line.append(',');
            hex(box.maxX);
            line.append(',');
            hex(box.maxY);
            line.append(',');
            hex(box.maxZ);
            line.append(']');
        }

        private void hex(double value) {
            long bits = Double.doubleToRawLongBits(value);
            line.append('"');
            for (int shift = 60; shift >= 0; shift -= 4) {
                line.append(HEX[(int) (bits >>> shift) & 15]);
            }
            line.append('"');
        }

        private void hex(float value) {
            int bits = Float.floatToRawIntBits(value);
            line.append('"');
            for (int shift = 28; shift >= 0; shift -= 4) {
                line.append(HEX[(bits >>> shift) & 15]);
            }
            line.append('"');
        }

        /** --explain 给人看的版本，不属于格式的一部分。 */
        private void explain(Case c, Outcome outcome) {
            StringBuilder text = new StringBuilder();
            text.append(c.kind.label).append(' ').append(id(c.subject)).append(" yaw=").append(c.yaw).append(" pitch=").append(c.pitch);
            text.append(" sneak=").append(c.sneak).append(" hand=").append(c.hand).append(" actor=").append(c.actor);
            if (c.hit != null) {
                text.append(" click=").append(c.hit.getBlockPos().toShortString()).append(' ').append(c.hit.getDirection());
                text.append(' ').append(c.hit.getLocation());
            }
            text.append(" scene:");
            describe(text, c.scene);
            text.append(" changes:");
            describe(text, outcome.changes);
            line.append(",\"text\":\"").append(text.toString().replace("\\", "\\\\").replace("\"", "\\\"")).append('"');
        }

        private static void describe(StringBuilder text, Long2ObjectMap<BlockState> states) {
            long[] keys = states.keySet().toLongArray();
            Arrays.sort(keys);
            for (long key : keys) {
                text.append(" (").append(BlockPos.of(key).toShortString()).append(") ");
                text.append(states.get(key).toString().replace("Block{minecraft:", "").replace("}", ""));
            }
        }

        private void printSummary(long nanos) {
            double seconds = nanos / 1e9;
            int total = written + skipped;
            log.printf("oracle summary: seed=%d cases=%d written=%d skipped=%d changedWorld=%d time=%.1fs rate=%.0f cases/s%n", options.seed,
                    total, written, skipped, changedWorld, seconds, total / seconds);
            for (Kind kind : Kind.values()) {
                log.printf("  kind %-8s %d%n", kind.label, kinds[kind.ordinal()]);
            }
            printMap("skip reasons", skipReasons);
            printMap("exceptions", exceptions);
            printRandomSites();
            log.println("per class: place/use/destroy/tick/flow/item/random/chunk cases | changed | placed | skips");
            stats.forEach(this::printClass);
            printZeroPlacements();
        }

        /** 第一列是玩家的随机源改变了结果、只能排除的题，第二列是写出来比较的题（世界的随机源直接比较）。 */
        private void printRandomSites() {
            Set<String> sites = new TreeSet<>(decisiveRandom.keySet());
            sites.addAll(cosmeticRandom.keySet());
            log.println("random sites: skipped because the entity random changed the outcome | written cases");
            for (String site : sites) {
                log.printf("  %7d %7d %s%n", decisiveRandom.getOrDefault(site, 0), cosmeticRandom.getOrDefault(site, 0), site);
            }
        }

        private void printMap(String title, Map<String, Integer> map) {
            log.println(title + ": " + map.size());
            map.forEach((key, count) -> log.printf("  %7d %s%n", count, key));
        }

        private void printClass(String name, ClassStats s) {
            StringBuilder counts = new StringBuilder();
            for (int[] row : List.of(s.cases, s.changed)) {
                for (int count : row) {
                    counts.append(String.format("%6d", count));
                }
                counts.append(" |");
            }
            log.printf("  %-36s%s %5d | %s%n", name, counts, s.placed, s.skips);
        }

        private void printZeroPlacements() {
            log.println("block classes with zero placements:");
            Map<String, List<Block>> byClass = new TreeMap<>();
            catalog.blocks.forEach(block -> byClass.computeIfAbsent(className(block), name -> new ArrayList<>()).add(block));
            byClass.forEach((name, blocks) -> {
                ClassStats s = stats.get(name);
                boolean inScope = options.only == null || (s != null && s.cases[Kind.PLACE.ordinal()] > 0);
                if (inScope && (s == null || s.placed == 0)) {
                    log.printf("  %-36s %s%n", name, explainZero(blocks, s));
                }
            });
        }

        private String explainZero(List<Block> blocks, ClassStats s) {
            if (blocks.stream().noneMatch(catalog.placeable::contains)) {
                return "no block item places this block";
            }
            if (blocks.stream().allMatch(block -> block.asItem() instanceof GameMasterBlockItem)) {
                return "needs the gamemaster permission";
            }
            if (blocks.stream().allMatch(block -> block.asItem() instanceof PlaceOnWaterBlockItem)) {
                return "placed by Item.use (the use_item packet with a ray trace), useItemOn always passes";
            }
            if (s != null && s.skips.keySet().stream().anyMatch(key -> key.startsWith("place:random:"))) {
                return "getStateForPlacement draws from the random source, every placement is a skip line: " + s.skips;
            }
            return s == null ? "UNEXPLAINED: no cases" : "UNEXPLAINED: results " + s.placeResults + " skips " + s.skips;
        }
    }
}
