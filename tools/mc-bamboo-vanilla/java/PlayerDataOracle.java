import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.mojang.serialization.Dynamic;
import com.mojang.serialization.Lifecycle;
import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.InputStreamReader;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Base64;
import java.util.List;
import java.util.Optional;
import java.util.UUID;
import sun.misc.Unsafe;
import net.minecraft.SharedConstants;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.util.Mth;
import net.minecraft.util.RandomSource;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.item.ItemStack;
import net.minecraft.commands.Commands;
import net.minecraft.core.BlockPos;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.LayeredRegistryAccess;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.UUIDUtil;
import net.minecraft.core.component.DataComponentPatch;
import net.minecraft.core.component.DataComponents;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.IntArrayTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.StringTag;
import net.minecraft.nbt.Tag;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.ReloadableServerResources;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.CloseableResourceManager;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.ProblemReporter;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.Difficulty;
import net.minecraft.world.ItemStackWithSlot;
import net.minecraft.world.entity.player.Abilities;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.DataPackConfig;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LevelSettings;
import net.minecraft.world.level.WorldDataConfiguration;
import net.minecraft.world.level.storage.LevelData;
import net.minecraft.world.level.storage.PrimaryLevelData;
import net.minecraft.world.level.storage.TagValueInput;
import net.minecraft.world.level.storage.ValueInput;
import net.minecraft.world.effect.MobEffectInstance;
import net.minecraft.world.entity.ai.attributes.AttributeInstance;
import net.minecraft.world.entity.ai.attributes.AttributeModifier;

/**
 * 用官方 26.3 的类检查 Bamboo 写出的带槽位物品、玩家数据和 level.dat。
 *
 * <p>不启动服务端、不接受 EULA，只加载世界层注册表，然后在 JVM 里直接调官方的
 * {@code ItemStackWithSlot.CODEC} / {@code ItemStack.CODEC}、{@code TagValueInput}
 * 和 {@code PrimaryLevelData.parse} / {@code createTag}。stdin 一行一个 JSON 请求，
 * stdout 一行一个 JSON 结果；NBT 字节用 base64 传，按键排序的规范 JSON 用来比较
 * （官方 {@code CompoundTag} 用 HashMap，键序是哈希序，不能逐字节比）。
 *
 * <p>命令：
 * <ul>
 *   <li>{@code item-read}：读 Bamboo 写出的一个物品 / 一个带槽位列表，用官方 codec
 *       解出来再编码回去，返回两份规范 JSON 和写回去的字节。</li>
 *   <li>{@code item-write}：官方造一批带槽位物品，返回 NBT 字节和规范 JSON。</li>
 *   <li>{@code player-read}：读 Bamboo 写出的 gzip 玩家文件，逐字段用
 *       {@code TagValueInput} 和官方 codec 读，再按官方 {@code ValueOutput} 的写法
 *       写回去，返回两份规范 JSON 和写回去的 gzip 字节。</li>
 *   <li>{@code player-write}：官方造一份玩家数据（只用 ValueOutput + 官方 codec，
 *       不构造 Player 实例），返回 gzip 字节和规范 JSON。</li>
 *   <li>{@code level-read}：读 Bamboo 写出的 gzip level.dat，用
 *       {@code PrimaryLevelData.parse} 解析，再 {@code createTag} 写回来，返回两份
 *       规范 JSON 和写回去的 gzip 字节。</li>
 *   <li>{@code level-write}：官方 {@code PrimaryLevelData.createTag} 写一份
 *       level.dat，返回 gzip 字节和规范 JSON。</li>
 * </ul>
 */
public final class PlayerDataOracle {
    private static final int OUTPUT_BUFFER = 1 << 20;
    private static final Unsafe UNSAFE = loadUnsafe();

    private PlayerDataOracle() {
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
        // 官方日志默认写 stdout，要抢在日志框架初始化前换掉，否则日志会混进 JSON。
        System.setOut(log);
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        try (CloseableResourceManager resources =
                        new MultiPackResourceManager(PackType.SERVER_DATA, repository.openAllSelected());
                Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), OUTPUT_BUFFER)) {
            RegistryAccess registries = loadRegistries(resources);
            new Oracle(registries, out).run();
        }
    }

    /**
     * 和 ComponentOracle 一样加载世界层注册表和服务端资源，不构造 ServerLevel。
     *
     * <p>物品的默认组件是数据驱动的，不走 ReloadableServerResources 的话
     * `Item.components()` 会抛「Components not bound yet」。
     */
    private static RegistryAccess loadRegistries(CloseableResourceManager resources) throws Exception {
        LayeredRegistryAccess<RegistryLayer> initial = RegistryLayer.createRegistryAccess();
        List<Registry.PendingTags<?>> staticTags =
                TagLoader.loadTagsForExistingRegistries(resources, initial.getLayer(RegistryLayer.STATIC));
        RegistryAccess.Frozen context = initial.getAccessForLoading(RegistryLayer.WORLD);
        List<HolderLookup.RegistryLookup<?>> lookups = TagLoader.buildUpdatedLookups(context, staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader
                .load(resources, lookups, RegistryDataLoader.WORLD_REGISTRIES,
                        java.util.concurrent.ForkJoinPool.commonPool())
                .join();
        LayeredRegistryAccess<RegistryLayer> layers = initial.replaceFrom(RegistryLayer.WORLD, world);
        ReloadableServerResources managers = ReloadableServerResources
                .loadResources(resources, layers, staticTags, FeatureFlags.DEFAULT_FLAGS,
                        Commands.CommandSelection.DEDICATED, LevelBasedPermissionSet.GAMEMASTER,
                        java.util.concurrent.ForkJoinPool.commonPool(), Runnable::run)
                .get();
        managers.updateComponentsAndStaticRegistryTags();
        return (RegistryAccess) managers.fullRegistries().lookup();
    }

    private static final class Oracle {
        private final RegistryAccess registries;
        private final Writer out;
        private final BufferedReader in;

        Oracle(RegistryAccess registries, Writer out) {
            this.registries = registries;
            this.out = out;
            this.in = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        }

        void run() throws Exception {
            String line;
            while ((line = in.readLine()) != null) {
                if (line.isBlank()) {
                    continue;
                }
                JsonObject request = com.google.gson.JsonParser.parseString(line).getAsJsonObject();
                JsonObject response = new JsonObject();
                String command = request.get("command").getAsString();
                response.addProperty("command", command);
                try {
                    handle(request, response, command);
                } catch (Exception error) {
                    response.addProperty("error", String.valueOf(error));
                }
                out.write(response.toString());
                out.write('\n');
                out.flush();
            }
        }

        private void handle(JsonObject request, JsonObject response, String command) throws Exception {
            switch (command) {
                case "item-read" -> itemRead(request, response);
                case "item-write" -> itemWrite(request, response);
                case "player-read" -> playerRead(request, response);
                case "player-write" -> playerWrite(request, response);
                case "level-read" -> levelRead(request, response);
                case "level-write" -> levelWrite(request, response);
                case "xp-arithmetic" -> xpArithmetic(request, response);
                case "xp-seed-load" -> xpSeedLoad(request, response);
                case "xp-criteria" -> xpCriteria(request, response);
                default -> response.addProperty("error", "unknown command " + command);
            }
        }

        /** 一个物品或一张带槽位列表的复合标签，用官方 codec 解一遍再编回去。 */
        private void itemRead(JsonObject request, JsonObject response) throws Exception {
            CompoundTag original = readNbt(request.get("nbt").getAsString());
            CompoundTag written;
            if (request.has("list") && request.get("list").getAsBoolean()) {
                // 一张列表：每一项按官方 TypedListWrapper 解，解不开的报告下标后跳过。
                ListTag read = original.getListOrEmpty("Items");
                JsonArray problems = new JsonArray();
                ListTag output = new ListTag();
                for (int index = 0; index < read.size(); index++) {
                    Tag entry = read.get(index);
                    var decoded = ItemStackWithSlot.CODEC.parse(ops(), entry);
                    if (decoded.error().isPresent()) {
                        problems.add(index);
                        continue;
                    }
                    output.add(ItemStackWithSlot.CODEC.encodeStart(ops(), decoded.result().orElseThrow())
                            .getOrThrow());
                }
                written = new CompoundTag();
                written.put("Items", output);
                response.add("original", canonical(rootItems(read)));
                response.add("problems", problems);
            } else {
                var decoded = ItemStack.CODEC.parse(ops(), original);
                Tag encoded = ItemStack.CODEC.encodeStart(ops(), decoded.getOrThrow()).getOrThrow();
                written = (CompoundTag) encoded;
                response.add("original", canonical(original));
            }
            response.add("written", canonical(written));
            response.addProperty("writtenNbt", encode(written));
        }

        /** 官方构造的一批带槽位物品。 */
        private void itemWrite(JsonObject request, JsonObject response) throws Exception {
            ListTag items = new ListTag();
            for (var element : request.getAsJsonArray("items")) {
                JsonObject spec = element.getAsJsonObject();
                int slot = spec.get("slot").getAsInt();
                int count = spec.get("count").getAsInt();
                ItemStack stack = new ItemStack(Items.STONE, count);
                if (spec.has("component") && spec.get("component").getAsBoolean()) {
                    stack = new ItemStack(Items.STONE.builtInRegistryHolder(), count,
                            DataComponentPatch.builder().set(DataComponents.DAMAGE, 3).build());
                }
                Tag tag = ItemStackWithSlot.CODEC.encodeStart(ops(), new ItemStackWithSlot(slot, stack))
                        .getOrThrow();
                items.add(tag);
            }
            CompoundTag root = new CompoundTag();
            root.put("Items", items);
            response.addProperty("nbt", encode(root));
            response.add("canonical", canonical(root));
        }

        /** 官方 TagValueInput 逐字段读玩家文件，再按官方 ValueOutput 的字段写法写回来。 */
        private void playerRead(JsonObject request, JsonObject response) throws Exception {
            CompoundTag original = readGzip(request.get("gzip").getAsString()).orElseThrow();
            ValueInput input = TagValueInput.create(ProblemReporter.DISCARDING, registries, original);
            CompoundTag written = playerTagFromInput(input, original);
            response.add("original", canonical(original));
            response.add("written", canonical(written));
            response.addProperty("gzipWritten", gzip(written));
            response.add("fields", playerFields(input, original));
        }

        /** 官方 codec 造一份玩家文件：每个字段都用 TagValueOutput 的同名方法写。 */
        private void playerWrite(JsonObject request, JsonObject response) throws Exception {
            CompoundTag tag = buildPlayerTag(request);
            response.addProperty("gzip", gzip(tag));
            response.add("canonical", canonical(tag));
        }

        /** 官方 PrimaryLevelData.parse 读，再 createTag 写回来。 */
        private void levelRead(JsonObject request, JsonObject response) throws Exception {
            CompoundTag root = readGzip(request.get("gzip").getAsString()).orElseThrow();
            CompoundTag data = root.getCompound("Data").orElseThrow();
            PrimaryLevelData parsed = PrimaryLevelData.parse(
                    new Dynamic<>(NbtOps.INSTANCE, data),
                    LevelSettings.parse(new Dynamic<>(NbtOps.INSTANCE, data), WorldDataConfiguration.DEFAULT),
                    PrimaryLevelData.SpecialWorldProperty.NONE,
                    Lifecycle.stable());
            CompoundTag writtenData = parsed.createTag(null);
            CompoundTag written = new CompoundTag();
            written.put("Data", writtenData);
            // D9：根和 Data 里 Bamboo 不认识的键原样写回，方便按键比较。
            written.putLong("LastPlayed", data.getLong("LastPlayed").orElse(0L));
            for (String key : root.keySet()) {
                if (!written.contains(key)) {
                    written.put(key, root.get(key));
                }
            }
            for (String key : data.keySet()) {
                if (!writtenData.contains(key)) {
                    writtenData.put(key, data.get(key));
                }
            }
            response.add("original", canonical(root));
            response.add("written", canonical(written));
            response.addProperty("gzipWritten", gzip(written));
            response.add("fields", levelFields(parsed, data));
        }

        /** 官方 PrimaryLevelData.createTag 写一份 level.dat。 */
        private void levelWrite(JsonObject request, JsonObject response) throws Exception {
            CompoundTag data = buildLevelTag(request);
            CompoundTag root = new CompoundTag();
            root.put("Data", data);
            response.addProperty("gzip", gzip(root));
            response.add("canonical", canonical(root));
        }

        /**
         * 官方 {@code ServerPlayer.doTick} 对 `xp` / `level` 只读准则取的是
         * {@code Mth.ceil((float) totalExperience)} 和
         * {@code Mth.ceil((float) experienceLevel)}。
         */
        private void xpCriteria(JsonObject request, JsonObject response) {
            JsonArray results = new JsonArray();
            for (var element : request.getAsJsonArray("cases")) {
                JsonObject spec = element.getAsJsonObject();
                int total = spec.get("total").getAsInt();
                int level = spec.get("level").getAsInt();
                JsonArray row = new JsonArray();
                row.add(net.minecraft.util.Mth.ceil((float) total));
                row.add(net.minecraft.util.Mth.ceil((float) level));
                results.add(row);
            }
            response.add("results", results);
        }

        /**
         * 官方 {@code Player.readAdditionalSaveData} 里 {@code XpSeed} 的一段：
         * {@code enchantmentSeed = input.getIntOr("XpSeed", 0)}，只有 0 才
         * {@code this.random.nextInt()}。这里用官方 {@code RandomSource.create(seed)}
         * 把这几次取数算出来，再取一次 nextInt 暴露随机源状态；Bamboo 的
         * {@code Player} 构造与读档必须产生同样的序列。
         */
        private void xpSeedLoad(JsonObject request, JsonObject response) {
            JsonArray results = new JsonArray();
            for (var element : request.getAsJsonArray("cases")) {
                JsonObject spec = element.getAsJsonObject();
                int saved = spec.get("saved").getAsInt();
                RandomSource random = RandomSource.create(spec.get("seed").getAsLong());
                int enchantment = saved;
                if (enchantment == 0) {
                    enchantment = random.nextInt();
                }
                JsonArray row = new JsonArray();
                row.add(enchantment);
                row.add(random.nextInt());
                results.add(row);
            }
            response.add("results", results);
        }

        /**
         * 用官方 Player / ServerPlayer 的方法跑一组经验题目，不构造服务端。
         *
         * <p>{@code give_points} / {@code give_levels} / {@code set_points} /
         * {@code set_levels} / {@code on_enchantment} 都调真实的官方方法；
         * {@code query_points} 和 {@code need} 用官方 {@code Mth.floor} 和
         * {@code getXpNeededForNextLevel}。返回每个字段的原始位，Rust 侧重放后逐项比。
         */
        private void xpArithmetic(JsonObject request, JsonObject response) throws Exception {
            JsonArray results = new JsonArray();
            for (var element : request.getAsJsonArray("cases")) {
                JsonObject spec = element.getAsJsonObject();
                XpPlayer player = XpPlayer.create(spec);
                String op = spec.get("op").getAsString();
                int value = spec.get("value").getAsInt();
                int points = 0;
                switch (op) {
                    case "give_points" -> player.giveExperiencePoints(value);
                    case "give_levels" -> player.giveExperienceLevels(value);
                    case "set_points" -> player.setExperiencePoints(value);
                    case "set_levels" -> player.setExperienceLevels(value);
                    case "on_enchantment" -> player.onEnchantmentPerformed(ItemStack.EMPTY, value);
                    case "query_points" -> points = Mth.floor(player.experienceProgress
                            * player.getXpNeededForNextLevel());
                    case "need" -> points = player.getXpNeededForNextLevel();
                    default -> throw new IllegalArgumentException("unknown xp op " + op);
                }
                JsonArray row = new JsonArray();
                row.add(player.experienceLevel);
                row.add(Float.floatToRawIntBits(player.experienceProgress));
                row.add(player.totalExperience);
                row.add(player.trackedScore);
                row.add(player.lastSentExp());
                if (op.equals("on_enchantment")) {
                    row.add(player.getEnchantmentSeed());
                } else {
                    row.add(points);
                }
                results.add(row);
            }
            response.add("results", results);
        }

        /**
         * 不跑构造函数：ServerPlayer 的构造函数要一个完整的服务端。
         * 只覆盖 {@code increaseScore}，避开没有初始化的 {@code entityData}；
         * 经验公式本身仍是官方 {@code Player.giveExperiencePoints}。
         */
        private static final class XpPlayer extends ServerPlayer {
            int trackedScore;

            private XpPlayer() {
                super(null, null, null, null);
            }

            static XpPlayer create(JsonObject spec) throws Exception {
                XpPlayer player = (XpPlayer) UNSAFE.allocateInstance(XpPlayer.class);
                set(Player.class, player, "experienceLevel", spec.get("level").getAsInt());
                set(Player.class, player, "experienceProgress",
                        Float.intBitsToFloat(spec.get("progress_bits").getAsInt()));
                set(Player.class, player, "totalExperience", spec.get("total").getAsInt());
                set(net.minecraft.world.entity.Entity.class, player, "tickCount", 0);
                set(Player.class, player, "lastLevelUpTime", 0);
                set(net.minecraft.world.entity.Entity.class, player, "random",
                        RandomSource.create(spec.get("seed").getAsLong()));
                set(ServerPlayer.class, player, "lastSentExp", -99999999);
                player.trackedScore = spec.get("score").getAsInt();
                return player;
            }

            @Override
            public void increaseScore(int amount) {
                trackedScore += amount;
            }

            int lastSentExp() throws Exception {
                Field field = ServerPlayer.class.getDeclaredField("lastSentExp");
                field.setAccessible(true);
                return field.getInt(this);
            }
        }

        private static void set(Class<?> owner, Object target, String name, Object value)
                throws Exception {
            Field field = owner.getDeclaredField(name);
            field.setAccessible(true);
            field.set(target, value);
        }

        private net.minecraft.resources.RegistryOps<Tag> ops() {
            return registries.createSerializationContext(NbtOps.INSTANCE);
        }

        private CompoundTag rootItems(ListTag items) {
            CompoundTag root = new CompoundTag();
            root.put("Items", items);
            return root;
        }

        /** 把一串官方 codec 编码的标签装进 ListTag（构造器是包私有的）。 */
        private ListTag tagList(java.util.stream.Stream<? extends Tag> values) {
            ListTag list = new ListTag();
            values.forEach(list::add);
            return list;
        }

        /** 用带注册表上下文的 ops 编解码，避免 Holder 的注册名解不开。 */
        private <T> void putCodec(CompoundTag tag, String name,
                com.mojang.serialization.Codec<T> codec, T value) {
            Tag encoded = codec.encodeStart(ops(), value).getOrThrow();
            tag.put(name, encoded);
        }

        /** 官方读取入口逐字段读出来的值，交给 Rust 侧对照自己的 PlayerData。 */
        private JsonObject playerFields(ValueInput input, CompoundTag original) {
            JsonObject fields = new JsonObject();
            fields.addProperty("data_version", input.getIntOr("DataVersion", -1));
            fields.addProperty("uuid", java.util.Arrays.toString(input.getIntArray("UUID").orElse(new int[4])));
            fields.add("pos", canonical(input.read("Pos", net.minecraft.world.phys.Vec3.CODEC)
                    .map(value -> net.minecraft.world.phys.Vec3.CODEC.encodeStart(ops(), value).getOrThrow())
                    .orElse(new ListTag())));
            fields.add("motion", canonical(input.read("Motion", net.minecraft.world.phys.Vec3.CODEC)
                    .map(value -> net.minecraft.world.phys.Vec3.CODEC.encodeStart(ops(), value).getOrThrow())
                    .orElse(new ListTag())));
            fields.add("rotation", canonical(input.read("Rotation", net.minecraft.world.phys.Vec2.CODEC)
                    .map(value -> net.minecraft.world.phys.Vec2.CODEC.encodeStart(ops(), value).getOrThrow())
                    .orElse(new ListTag())));
            fields.addProperty("fall_distance", input.getDoubleOr("fall_distance", 0.0));
            fields.addProperty("fire", input.getShortOr("Fire", (short) 0));
            fields.addProperty("air", input.getShortOr("Air", (short) 300));
            fields.addProperty("ticks_frozen", input.getIntOr("TicksFrozen", 0));
            fields.addProperty("on_ground", input.getBooleanOr("OnGround", false));
            fields.addProperty("invulnerable", input.getBooleanOr("Invulnerable", false));
            fields.addProperty("portal_cooldown", input.getIntOr("PortalCooldown", 0));
            fields.addProperty("health", (double) input.getFloatOr("Health", 20.0F));
            fields.addProperty("hurt_time", input.getShortOr("HurtTime", (short) 0));
            fields.addProperty("death_time", input.getShortOr("DeathTime", (short) 0));
            fields.addProperty("absorption", input.getFloatOr("AbsorptionAmount", 0.0F));
            fields.addProperty("fall_flying", input.getBooleanOr("FallFlying", false));
            fields.add("inventory", canonical(tagList(input.listOrEmpty("Inventory", ItemStackWithSlot.CODEC)
                    .stream().map(one -> ItemStackWithSlot.CODEC.encodeStart(ops(), one).getOrThrow()))));
            fields.add("ender_items", canonical(tagList(input.listOrEmpty("EnderItems", ItemStackWithSlot.CODEC)
                    .stream().map(one -> ItemStackWithSlot.CODEC.encodeStart(ops(), one).getOrThrow()))));
            fields.addProperty("selected_item_slot", input.getIntOr("SelectedItemSlot", 0));
            fields.addProperty("sleep_timer", input.getShortOr("SleepTimer", (short) 0));
            fields.addProperty("xp_p", input.getFloatOr("XpP", 0.0F));
            fields.addProperty("xp_level", input.getIntOr("XpLevel", 0));
            fields.addProperty("xp_total", input.getIntOr("XpTotal", 0));
            fields.addProperty("xp_seed", input.getIntOr("XpSeed", 0));
            fields.addProperty("score", input.getIntOr("Score", 0));
            fields.addProperty("food_level", input.getIntOr("foodLevel", 20));
            fields.addProperty("food_tick_timer", input.getIntOr("foodTickTimer", 0));
            fields.addProperty("food_saturation", input.getFloatOr("foodSaturationLevel", 5.0F));
            fields.addProperty("food_exhaustion", input.getFloatOr("foodExhaustionLevel", 0.0F));
            fields.add("attributes", canonical(tagList(input.listOrEmpty("attributes",
                    AttributeInstance.Packed.CODEC).stream()
                    .map(one -> AttributeInstance.Packed.CODEC.encodeStart(ops(), one).getOrThrow()))));
            fields.add("active_effects", canonical(tagList(input.listOrEmpty("active_effects",
                    MobEffectInstance.CODEC).stream()
                    .map(one -> MobEffectInstance.CODEC.encodeStart(ops(), one).getOrThrow()))));
            fields.add("abilities", canonical(input.read("abilities", Abilities.Packed.CODEC)
                    .map(value -> Abilities.Packed.CODEC.encodeStart(ops(), value).getOrThrow())
                    .orElse(new CompoundTag())));
            fields.addProperty("dimension", input.getStringOr("Dimension", "minecraft:overworld"));
            fields.addProperty("game_type", input.read("playerGameType", GameType.LEGACY_ID_CODEC)
                    .map(GameType::getId).orElse(0));
            fields.addProperty("seen_credits", input.getBooleanOr("seenCredits", false));
            fields.addProperty("spawn_extra_particles_on_fall",
                    input.getBooleanOr("spawn_extra_particles_on_fall", false));
            // `ServerPlayer` 的 `warden_spawn_tracker`：WardenSpawnTracker.CODEC 恒写。
            fields.add("warden_spawn_tracker", canonical(input.read("warden_spawn_tracker",
                    net.minecraft.world.entity.monster.warden.WardenSpawnTracker.CODEC)
                    .map(value -> net.minecraft.world.entity.monster.warden.WardenSpawnTracker.CODEC
                            .encodeStart(ops(), value).getOrThrow())
                    .orElse(new CompoundTag())));
            // `LastDeathLocation` 是 GlobalPos.CODEC：只有 Optional 非空才写。
            fields.add("last_death_location", input.read("LastDeathLocation",
                    net.minecraft.core.GlobalPos.CODEC)
                    .map(value -> net.minecraft.core.GlobalPos.CODEC.encodeStart(ops(), value).getOrThrow())
                    .map(Oracle::canonical).orElseGet(() -> new JsonObject()));
            // `ServerPlayer.saveEnderPearls` 的 `ender_pearls` childrenList：官方
            // `ValueInput.childrenListOrEmpty` 读出的条数，和原始 NBT 的规范 JSON。
            fields.addProperty("ender_pearl_count",
                    input.childrenListOrEmpty("ender_pearls").stream().count());
            fields.add("ender_pearls", Oracle.canonical(original.getListOrEmpty("ender_pearls")));
            // `ServerPlayer.loadAndSpawnParentVehicle` 读的是
            // `ValueInput.child("RootVehicle")`；这里比官方读回的整条标签。
            input.child("RootVehicle").ifPresent(child -> {
                var attach = child.read("Attach", net.minecraft.core.UUIDUtil.CODEC)
                        .map(value -> net.minecraft.core.UUIDUtil.CODEC
                                .encodeStart(ops(), value).getOrThrow());
                fields.add("root_vehicle_attach", attach
                        .map(Oracle::canonical).orElseGet(JsonObject::new));
                var root = original.getCompoundOrEmpty("RootVehicle");
                fields.add("root_vehicle_entity",
                        Oracle.canonical(root.getCompoundOrEmpty("Entity")));
                fields.add("root_vehicle_tag", Oracle.canonical(root));
            });
            return fields;
        }

        /** 按官方 ValueOutput 的字段名和类型把玩家文件写回来。 */
        private CompoundTag playerTagFromInput(ValueInput input, CompoundTag original) {
            CompoundTag tag = new CompoundTag();
            putCodec(tag, "Pos", net.minecraft.world.phys.Vec3.CODEC,
                    input.read("Pos", net.minecraft.world.phys.Vec3.CODEC).orElse(new net.minecraft.world.phys.Vec3(0, 0, 0)));
            putCodec(tag, "Motion", net.minecraft.world.phys.Vec3.CODEC,
                    input.read("Motion", net.minecraft.world.phys.Vec3.CODEC).orElse(new net.minecraft.world.phys.Vec3(0, 0, 0)));
            putCodec(tag, "Rotation", net.minecraft.world.phys.Vec2.CODEC,
                    input.read("Rotation", net.minecraft.world.phys.Vec2.CODEC).orElse(new net.minecraft.world.phys.Vec2(0, 0)));
            tag.putDouble("fall_distance", input.getDoubleOr("fall_distance", 0.0));
            tag.putShort("Fire", (short) input.getShortOr("Fire", (short) 0));
            tag.putShort("Air", (short) input.getShortOr("Air", (short) 300));
            int frozen = input.getIntOr("TicksFrozen", 0);
            if (frozen > 0) tag.putInt("TicksFrozen", frozen);
            tag.putBoolean("OnGround", input.getBooleanOr("OnGround", false));
            tag.putBoolean("Invulnerable", input.getBooleanOr("Invulnerable", false));
            tag.putInt("PortalCooldown", input.getIntOr("PortalCooldown", 0));
            putCodec(tag, "UUID", UUIDUtil.CODEC, UUIDUtil.uuidFromIntArray(
                    input.getIntArray("UUID").orElse(new int[4])));
            tag.putFloat("Health", input.getFloatOr("Health", 20.0F));
            tag.putShort("HurtTime", (short) input.getShortOr("HurtTime", (short) 0));
            tag.putShort("DeathTime", (short) input.getShortOr("DeathTime", (short) 0));
            tag.putFloat("AbsorptionAmount", input.getFloatOr("AbsorptionAmount", 0.0F));
            tag.putBoolean("FallFlying", input.getBooleanOr("FallFlying", false));
            putCodec(tag, "Inventory", ItemStackWithSlot.CODEC.listOf(), inventoryOf(input, "Inventory"));
            tag.putInt("SelectedItemSlot", input.getIntOr("SelectedItemSlot", 0));
            tag.putShort("SleepTimer", (short) input.getShortOr("SleepTimer", (short) 0));
            tag.putFloat("XpP", input.getFloatOr("XpP", 0.0F));
            tag.putInt("XpLevel", input.getIntOr("XpLevel", 0));
            tag.putInt("XpTotal", input.getIntOr("XpTotal", 0));
            tag.putInt("XpSeed", input.getIntOr("XpSeed", 0));
            tag.putInt("Score", input.getIntOr("Score", 0));
            input.read("LastDeathLocation", net.minecraft.core.GlobalPos.CODEC)
                    .ifPresent(value -> putCodec(tag, "LastDeathLocation",
                            net.minecraft.core.GlobalPos.CODEC, value));
            tag.putInt("foodLevel", input.getIntOr("foodLevel", 20));
            tag.putInt("foodTickTimer", input.getIntOr("foodTickTimer", 0));
            tag.putFloat("foodSaturationLevel", input.getFloatOr("foodSaturationLevel", 5.0F));
            tag.putFloat("foodExhaustionLevel", input.getFloatOr("foodExhaustionLevel", 0.0F));
            // `LivingEntity.addAdditionalSaveData`：存在 `attributes` 时按官方 Packed.LIST_CODEC 往返。
            input.read("attributes", AttributeInstance.Packed.LIST_CODEC)
                    .ifPresent(attrs -> putCodec(tag, "attributes", AttributeInstance.Packed.LIST_CODEC, attrs));
            // `LivingEntity.addAdditionalSaveData`：空表不写键。
            var effects = input.listOrEmpty("active_effects", MobEffectInstance.CODEC)
                    .stream().toList();
            if (!effects.isEmpty()) {
                putCodec(tag, "active_effects", MobEffectInstance.CODEC.listOf(), effects);
            }
            putCodec(tag, "abilities", Abilities.Packed.CODEC, input.read("abilities", Abilities.Packed.CODEC)
                    .orElse(new Abilities.Packed(false, false, false, false, true, 0.05F, 0.1F)));
            putCodec(tag, "EnderItems", ItemStackWithSlot.CODEC.listOf(), inventoryOf(input, "EnderItems"));
            tag.putString("Dimension", input.getStringOr("Dimension", "minecraft:overworld"));
            putCodec(tag, "playerGameType", GameType.LEGACY_ID_CODEC,
                    input.read("playerGameType", GameType.LEGACY_ID_CODEC).orElse(GameType.SURVIVAL));
            tag.putBoolean("seenCredits", input.getBooleanOr("seenCredits", false));
            tag.putBoolean("spawn_extra_particles_on_fall",
                    input.getBooleanOr("spawn_extra_particles_on_fall", false));
            putCodec(tag, "warden_spawn_tracker",
                    net.minecraft.world.entity.monster.warden.WardenSpawnTracker.CODEC,
                    input.read("warden_spawn_tracker",
                            net.minecraft.world.entity.monster.warden.WardenSpawnTracker.CODEC)
                            .orElseGet(net.minecraft.world.entity.monster.warden.WardenSpawnTracker::new));
            tag.putInt("DataVersion", SharedConstants.getCurrentVersion().dataVersion().version());
            // 官方读时忽略的键，Bamboo 会原样保留：官方的写回里也把它们带上，方便按键比较。
            for (String key : original.keySet()) {
                if (!tag.contains(key)) {
                    tag.put(key, original.get(key));
                }
            }
            return tag;
        }

        private List<ItemStackWithSlot> inventoryOf(ValueInput input, String name) {
            return input.listOrEmpty(name, ItemStackWithSlot.CODEC).stream().toList();
        }

        /** 官方造一份玩家文件，字段值来自请求里的一串数（Rust 侧按同一批用例出题）。 */
        private CompoundTag buildPlayerTag(JsonObject request) throws Exception {
            JsonObject spec = request.getAsJsonObject("player");
            CompoundTag tag = new CompoundTag();
            tag.putInt("DataVersion", SharedConstants.getCurrentVersion().dataVersion().version());
            tag.put("Pos", doubles(spec.getAsJsonArray("pos")));
            tag.put("Motion", doubles(spec.getAsJsonArray("motion")));
            tag.put("Rotation", floats(spec.getAsJsonArray("rotation")));
            tag.putDouble("fall_distance", spec.get("fall_distance").getAsDouble());
            tag.putShort("Fire", (short) spec.get("fire").getAsInt());
            tag.putShort("Air", (short) spec.get("air").getAsInt());
            if (spec.has("ticks_frozen") && spec.get("ticks_frozen").getAsInt() > 0) {
                tag.putInt("TicksFrozen", spec.get("ticks_frozen").getAsInt());
            }
            tag.putBoolean("OnGround", spec.get("on_ground").getAsBoolean());
            tag.putBoolean("Invulnerable", spec.get("invulnerable").getAsBoolean());
            tag.putInt("PortalCooldown", spec.get("portal_cooldown").getAsInt());
            putCodec(tag, "UUID", UUIDUtil.CODEC, new UUID(spec.get("uuid_high").getAsLong(), spec.get("uuid_low").getAsLong()));
            tag.putFloat("Health", spec.get("health").getAsFloat());
            tag.putShort("HurtTime", (short) spec.get("hurt_time").getAsInt());
            tag.putShort("DeathTime", (short) spec.get("death_time").getAsInt());
            tag.putFloat("AbsorptionAmount", spec.get("absorption").getAsFloat());
            tag.putBoolean("FallFlying", spec.get("fall_flying").getAsBoolean());
            ListTag inventory = new ListTag();
            for (var element : spec.getAsJsonArray("inventory")) {
                inventory.add(slotTag(element.getAsJsonObject()));
            }
            tag.put("Inventory", inventory);
            tag.putInt("SelectedItemSlot", spec.get("selected_item_slot").getAsInt());
            tag.putShort("SleepTimer", (short) spec.get("sleep_timer").getAsInt());
            tag.putFloat("XpP", spec.get("xp_p").getAsFloat());
            tag.putInt("XpLevel", spec.get("xp_level").getAsInt());
            tag.putInt("XpTotal", spec.get("xp_total").getAsInt());
            tag.putInt("XpSeed", spec.get("xp_seed").getAsInt());
            tag.putInt("Score", spec.get("score").getAsInt());
            if (spec.has("last_death_dimension")) {
                putCodec(tag, "LastDeathLocation", net.minecraft.core.GlobalPos.CODEC,
                        net.minecraft.core.GlobalPos.of(
                                net.minecraft.resources.ResourceKey.create(
                                        net.minecraft.core.registries.Registries.DIMENSION,
                                        net.minecraft.resources.Identifier.parse(
                                                spec.get("last_death_dimension").getAsString())),
                                new BlockPos(
                                        spec.get("last_death_x").getAsInt(),
                                        spec.get("last_death_y").getAsInt(),
                                        spec.get("last_death_z").getAsInt())));
            }
            tag.putInt("foodLevel", spec.get("food_level").getAsInt());
            tag.putInt("foodTickTimer", spec.get("food_tick_timer").getAsInt());
            tag.putFloat("foodSaturationLevel", spec.get("food_saturation").getAsFloat());
            tag.putFloat("foodExhaustionLevel", spec.get("food_exhaustion").getAsFloat());
            if (spec.has("attributes")) {
                List<AttributeInstance.Packed> attrs = new ArrayList<>();
                for (var element : spec.getAsJsonArray("attributes")) {
                    JsonObject a = element.getAsJsonObject();
                    var holder = registries.lookupOrThrow(net.minecraft.core.registries.Registries.ATTRIBUTE)
                            .getOrThrow(net.minecraft.resources.ResourceKey.create(
                                    net.minecraft.core.registries.Registries.ATTRIBUTE,
                                    net.minecraft.resources.Identifier.parse(a.get("id").getAsString())));
                    double base = a.get("base").getAsDouble();
                    List<AttributeModifier> modifiers = new ArrayList<>();
                    if (a.has("modifiers")) {
                        for (var modElement : a.getAsJsonArray("modifiers")) {
                            JsonObject m = modElement.getAsJsonObject();
                            AttributeModifier.Operation op = switch (m.get("operation").getAsString()) {
                                case "add_value" -> AttributeModifier.Operation.ADD_VALUE;
                                case "add_multiplied_base" -> AttributeModifier.Operation.ADD_MULTIPLIED_BASE;
                                case "add_multiplied_total" -> AttributeModifier.Operation.ADD_MULTIPLIED_TOTAL;
                                default -> throw new IllegalArgumentException("unknown op " + m.get("operation"));
                            };
                            modifiers.add(new AttributeModifier(
                                    net.minecraft.resources.Identifier.parse(m.get("id").getAsString()),
                                    m.get("amount").getAsDouble(),
                                    op));
                        }
                    }
                    attrs.add(new AttributeInstance.Packed(holder, base, modifiers));
                }
                putCodec(tag, "attributes", AttributeInstance.Packed.LIST_CODEC, attrs);
            }
            if (spec.has("active_effects")) {
                ListTag effects = new ListTag();
                for (var element : spec.getAsJsonArray("active_effects")) {
                    JsonObject e = element.getAsJsonObject();
                    var holder = registries.lookupOrThrow(net.minecraft.core.registries.Registries.MOB_EFFECT)
                            .getOrThrow(net.minecraft.resources.ResourceKey.create(
                                    net.minecraft.core.registries.Registries.MOB_EFFECT,
                                    net.minecraft.resources.Identifier.parse(e.get("id").getAsString())));
                    // 用官方 codec 解码，确保键名和缺省值都按 26.3。
                    CompoundTag one = new CompoundTag();
                    one.putString("id", e.get("id").getAsString());
                    one.putInt("amplifier", e.get("amplifier").getAsInt());
                    one.putInt("duration", e.get("duration").getAsInt());
                    one.putBoolean("ambient", e.get("ambient").getAsBoolean());
                    one.putBoolean("show_particles", e.get("show_particles").getAsBoolean());
                    if (e.has("show_icon")) {
                        one.putBoolean("show_icon", e.get("show_icon").getAsBoolean());
                    }
                    if (e.has("hidden_effect")) {
                        JsonObject h = e.getAsJsonObject("hidden_effect");
                        CompoundTag hidden = new CompoundTag();
                        hidden.putInt("amplifier", h.get("amplifier").getAsInt());
                        hidden.putInt("duration", h.get("duration").getAsInt());
                        hidden.putBoolean("ambient", h.get("ambient").getAsBoolean());
                        hidden.putBoolean("show_particles", h.get("show_particles").getAsBoolean());
                        hidden.putBoolean("show_icon", h.get("show_particles").getAsBoolean());
                        one.put("hidden_effect", hidden);
                    }
                    var instance = MobEffectInstance.CODEC.parse(ops(), one).getOrThrow();
                    effects.add(MobEffectInstance.CODEC.encodeStart(ops(), instance).getOrThrow());
                }
                tag.put("active_effects", effects);
            }
            JsonObject a = spec.getAsJsonObject("abilities");
            putCodec(tag, "abilities", Abilities.Packed.CODEC, new Abilities.Packed(
                    a.get("invulnerable").getAsBoolean(), a.get("flying").getAsBoolean(),
                    a.get("may_fly").getAsBoolean(), a.get("instabuild").getAsBoolean(),
                    a.get("may_build").getAsBoolean(), a.get("fly_speed").getAsFloat(),
                    a.get("walk_speed").getAsFloat()));
            ListTag ender = new ListTag();
            for (var element : spec.getAsJsonArray("ender_items")) {
                ender.add(slotTag(element.getAsJsonObject()));
            }
            tag.put("EnderItems", ender);
            tag.putString("Dimension", spec.get("dimension").getAsString());
            putCodec(tag, "playerGameType", GameType.LEGACY_ID_CODEC,
                    GameType.byId(spec.get("game_type").getAsInt()));
            if (spec.has("previous_game_type")) {
                tag.storeNullable("previousPlayerGameType", GameType.LEGACY_ID_CODEC,
                        GameType.byId(spec.get("previous_game_type").getAsInt()));
            }
            tag.putBoolean("seenCredits", spec.get("seen_credits").getAsBoolean());
            tag.putBoolean("spawn_extra_particles_on_fall",
                    spec.get("spawn_extra_particles_on_fall").getAsBoolean());
            if (spec.has("warden_spawn_tracker")) {
                JsonObject w = spec.getAsJsonObject("warden_spawn_tracker");
                putCodec(tag, "warden_spawn_tracker",
                        net.minecraft.world.entity.monster.warden.WardenSpawnTracker.CODEC,
                        new net.minecraft.world.entity.monster.warden.WardenSpawnTracker(
                                w.get("ticks_since_last_warning").getAsInt(),
                                w.get("warning_level").getAsInt(),
                                w.get("cooldown_ticks").getAsInt()));
            }
            // 官方有而 Bamboo 不认识的键，用来测 D9 原样保留。
            if (spec.has("unknown_tag")) {
                tag.put("bamboo:unknown", StringTag.valueOf(spec.get("unknown_tag").getAsString()));
            }
            // `ender_pearls` 的每项由 Bamboo 写出、用官方 NbtIo 读回来：
            // 两套 NBT 编解码器对同一份珍珠存档必须逐字节一致。
            if (spec.has("ender_pearls")) {
                ListTag pearls = new ListTag();
                for (var element : spec.getAsJsonArray("ender_pearls")) {
                    pearls.add(readNbt(element.getAsString()));
                }
                tag.put("ender_pearls", pearls);
            }
            // `ServerPlayer.saveParentVehicle` 的 `RootVehicle`：整条复合标签由
            // Bamboo 写出，官方的 `ValueInput.child` 读回后逐键比较。
            if (spec.has("root_vehicle")) {
                tag.put("RootVehicle", readNbt(spec.get("root_vehicle").getAsString()));
            }
            return tag;
        }

        /** 官方 ItemStackWithSlot.CODEC 造一条带槽位的物品。 */
        private Tag slotTag(JsonObject spec) {
            int slot = spec.get("slot").getAsInt();
            int count = spec.get("count").getAsInt();
            ItemStack stack = new ItemStack(Items.STONE, count);
            if (spec.has("component") && spec.get("component").getAsBoolean()) {
                stack = new ItemStack(Items.STONE.builtInRegistryHolder(), count,
                        DataComponentPatch.builder().set(DataComponents.DAMAGE, 3).build());
            }
            return ItemStackWithSlot.CODEC.encodeStart(ops(), new ItemStackWithSlot(slot, stack))
                    .getOrThrow();
        }

        private JsonObject levelFields(PrimaryLevelData parsed, CompoundTag data) {
            JsonObject fields = new JsonObject();
            fields.addProperty("level_name", parsed.getLevelName());
            fields.addProperty("game_type", parsed.getGameType().getId());
            fields.addProperty("time", parsed.getGameTime());
            fields.addProperty("allow_commands", parsed.isAllowCommands());
            fields.addProperty("initialized", parsed.isInitialized());
            fields.add("spawn", canonical(LevelData.RespawnData.CODEC
                    .encodeStart(ops(), parsed.getRespawnData()).getOrThrow()));
            fields.add("difficulty_settings", canonical(LevelSettings.DifficultySettings.CODEC
                    .encodeStart(ops(), parsed.getLevelSettings().difficultySettings()).getOrThrow()));
            fields.add("version_history", canonical(data.getListOrEmpty("version_history")));
            return fields;
        }

        /** 官方 PrimaryLevelData 构造 + createTag；settings / spawn 由请求指定。 */
        private CompoundTag buildLevelTag(JsonObject request) {
            JsonObject spec = request.getAsJsonObject("level");
            String difficulty = spec.get("difficulty").getAsString();
            LevelSettings.DifficultySettings settings = new LevelSettings.DifficultySettings(
                    Difficulty.byName(difficulty), spec.get("hardcore").getAsBoolean(),
                    spec.get("locked").getAsBoolean());
            DataPackConfig packs = new DataPackConfig(List.of("vanilla"), List.of());
            WorldDataConfiguration configuration =
                    new WorldDataConfiguration(packs, net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS);
            LevelSettings levelSettings = new LevelSettings(
                    spec.get("level_name").getAsString(), GameType.byId(spec.get("game_type").getAsInt()),
                    settings, spec.get("allow_commands").getAsBoolean(), configuration);
            PrimaryLevelData level = new PrimaryLevelData(levelSettings,
                    PrimaryLevelData.SpecialWorldProperty.NONE, Lifecycle.stable());
            level.setGameTime(spec.get("time").getAsLong());
            level.setInitialized(spec.get("initialized").getAsBoolean());
            JsonObject spawn = spec.getAsJsonObject("spawn");
            level.setSpawn(LevelData.RespawnData.of(Level.OVERWORLD,
                    new BlockPos(spawn.get("x").getAsInt(), spawn.get("y").getAsInt(), spawn.get("z").getAsInt()),
                    spawn.get("yaw").getAsFloat(), spawn.get("pitch").getAsFloat()));
            CompoundTag data = level.createTag(null);
            // createTag 每次都把 LastPlayed 刷成当前时间；改成固定值才能按键比较。
            data.putLong("LastPlayed", spec.get("last_played").getAsLong());
            if (spec.has("unknown_tag")) {
                data.putString("bamboo:unknown", spec.get("unknown_tag").getAsString());
            }
            return data;
        }

        private static ListTag doubles(JsonArray values) {
            ListTag list = new ListTag();
            for (var value : values) {
                list.add(net.minecraft.nbt.DoubleTag.valueOf(value.getAsDouble()));
            }
            return list;
        }

        private static ListTag floats(JsonArray values) {
            ListTag list = new ListTag();
            for (var value : values) {
                list.add(net.minecraft.nbt.FloatTag.valueOf(value.getAsFloat()));
            }
            return list;
        }

        private static CompoundTag readNbt(String base64) throws Exception {
            byte[] bytes = Base64.getDecoder().decode(base64);
            return net.minecraft.nbt.NbtIo.read(new DataInputStream(new ByteArrayInputStream(bytes)));
        }

        private static Optional<CompoundTag> readGzip(String base64) throws Exception {
            byte[] bytes = Base64.getDecoder().decode(base64);
            return Optional.of(net.minecraft.nbt.NbtIo.readCompressed(
                    new ByteArrayInputStream(bytes), net.minecraft.nbt.NbtAccounter.unlimitedHeap()));
        }

        private static String encode(CompoundTag tag) throws Exception {
            ByteArrayOutputStream buffer = new ByteArrayOutputStream();
            net.minecraft.nbt.NbtIo.write(tag, new DataOutputStream(buffer));
            return Base64.getEncoder().encodeToString(buffer.toByteArray());
        }

        private static String gzip(CompoundTag tag) throws Exception {
            ByteArrayOutputStream buffer = new ByteArrayOutputStream();
            net.minecraft.nbt.NbtIo.writeCompressed(tag, buffer);
            return Base64.getEncoder().encodeToString(buffer.toByteArray());
        }

        /** CompoundTag 用 HashMap，键序不确定：转成按键排序的规范 JSON 再比。 */
        private static JsonObject canonical(Tag tag) {
            JsonObject out = new JsonObject();
            out.addProperty("type", tag.getId());
            switch (tag.getId()) {
                case 1, 2, 3, 4, 5, 6 -> out.addProperty("value",
                        String.valueOf(((net.minecraft.nbt.NumericTag) tag).box()));
                case 8 -> out.addProperty("value", ((net.minecraft.nbt.StringTag) tag).value());
                case 7 -> {
                    JsonArray values = new JsonArray();
                    for (byte value : ((net.minecraft.nbt.ByteArrayTag) tag).getAsByteArray()) {
                        values.add((int) value);
                    }
                    out.add("values", values);
                }
                case 11 -> {
                    JsonArray values = new JsonArray();
                    for (int value : ((IntArrayTag) tag).getAsIntArray()) {
                        values.add(value);
                    }
                    out.add("values", values);
                }
                case 12 -> {
                    JsonArray values = new JsonArray();
                    for (long value : ((net.minecraft.nbt.LongArrayTag) tag).getAsLongArray()) {
                        values.add(value);
                    }
                    out.add("values", values);
                }
                case 9 -> {
                    JsonArray values = new JsonArray();
                    for (Tag value : (ListTag) tag) {
                        values.add(canonical(value));
                    }
                    out.add("values", values);
                }
                case 10 -> {
                    JsonObject values = new JsonObject();
                    List<String> keys = new ArrayList<>(((CompoundTag) tag).keySet());
                    keys.sort(String::compareTo);
                    for (String key : keys) {
                        values.add(key, canonical(((CompoundTag) tag).get(key)));
                    }
                    out.add("values", values);
                }
                default -> throw new IllegalArgumentException("tag type " + tag.getId());
            }
            return out;
        }
    }
}
