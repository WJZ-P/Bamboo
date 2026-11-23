// 官方 `EntitySelector` 的解析与过滤 / 排序对拍。
// 不启动服务端、不读存档、不接受 EULA：JVM 内只加载官方选择器，按固定实体列表
// 逐个跑官方 predicate / order，输出选中的 ID 列表；Rust 侧重放同一套输入。
//
// Oracle for official `EntitySelector` parsing, filtering and ordering.
// No server, no save, no EULA: loads the official selector and runs its
// predicate / order over a fixed entity list, emitting the selected ids.

import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.ParseResults;
import com.mojang.brigadier.StringReader;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.SharedConstants;
import net.minecraft.commands.arguments.selector.EntitySelector;
import net.minecraft.commands.arguments.selector.EntitySelectorParser;
import net.minecraft.server.Bootstrap;
import sun.misc.Unsafe;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.phys.Vec3;

public final class CommandOracle {
    private static final Unsafe UNSAFE = loadUnsafe();
    private static final int OUTPUT_BUFFER = 1 << 20;

    /** 和 Rust 侧 `SELECTORS` 一致。 */
    private static final String[] SELECTORS = {
        "@e",
        "@e[type=minecraft:cow]",
        "@e[type=!minecraft:cow]",
        "@e[type=#minecraft:animals]",
        "@e[type=!#minecraft:animals]",
        "@e[type=minecraft:player]",
        "@e[type=item,distance=..3]",
        "@e[distance=2..]",
        "@e[x=8,y=0,z=8,dx=3,dy=3,dz=3]",
        "@e[sort=nearest,limit=2]",
        "@e[sort=furthest,limit=2]",
        "@e[limit=1]",
        "@e[type=minecraft:cow,name=Cow]",
        "@e[type=minecraft:cow,name=!Cow]",
        "@e[type=minecraft:cow,tag=]",
        "@e[type=minecraft:cow,tag=!]",
        "@e[gamemode=creative]",
        "@e[gamemode=!creative]",
        "@e[level=0]",
        "@e[level=1]",
        "@s",
        "@n",
        "@a",
        "@p",
    };

    /** 和 Rust 侧固定场景一致：玩家 1、牛 1<<24、物品 (1<<24)+1。 */
    private static final int[] IDS = {1, 1 << 24, (1 << 24) + 1};

    /** `/advancement` 的语法题：官方 `AdvancementCommands.register` 直接注册到 dispatcher。 */
    private static final String[] ADVANCEMENT_COMMANDS = {
        "advancement grant @s everything",
        "advancement revoke @s everything",
        "advancement grant @s only minecraft:story/root",
        "advancement grant @s only minecraft:story/root crafting_table",
        "advancement revoke @s only minecraft:story/mine_stone get_stone",
        "advancement grant @s from minecraft:story/root",
        "advancement revoke @s until minecraft:story/mine_stone",
        "advancement grant @s through minecraft:story/upgrade_tools",
        "advancement grant Kitten everything",
        "advancement grant @s only minecraft:does_not_exist",
        "advancement grant @s nope",
        "advancement grant @s everything extra",
    };

    private CommandOracle() {
    }

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            run(args, log);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(String[] args, PrintStream log) throws Exception {
        if (args.length != 0) {
            throw new IllegalArgumentException("unexpected arguments");
        }
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        System.setOut(log);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        try (Writer out = new BufferedWriter(
                new OutputStreamWriter(stdout, StandardCharsets.UTF_8), OUTPUT_BUFFER)) {
            out.write("{\"type\":\"header\",\"version\":\"");
            out.write(SharedConstants.getCurrentVersion().id());
            out.write("\",\"protocol\":");
            out.write(Integer.toString(SharedConstants.getCurrentVersion().protocolVersion()));
            out.write("}\n");
            for (String input : SELECTORS) {
                emit(out, input);
            }
            // 官方 `Commands` 的构造器注册全部命令；这里只用它的 dispatcher 解析
            // `/advancement`，不构造 ServerLevel、不执行命令。
            CommandDispatcher<net.minecraft.commands.CommandSourceStack> dispatcher =
                    new CommandDispatcher<>();
            net.minecraft.server.commands.AdvancementCommands.register(dispatcher);
            // `Commands.LEVEL_GAMEMASTERS` 读的是 source 的 PermissionSetSupplier，
            // 这里用 Unsafe 分配一个未初始化的 `CommandSourceStack`，只填权限字段。
            net.minecraft.commands.CommandSourceStack source =
                    (net.minecraft.commands.CommandSourceStack) UNSAFE.allocateInstance(
                            net.minecraft.commands.CommandSourceStack.class);
            java.lang.reflect.Field permissions =
                    net.minecraft.commands.CommandSourceStack.class.getDeclaredField("permissions");
            UNSAFE.putObject(source, UNSAFE.objectFieldOffset(permissions),
                    net.minecraft.server.permissions.LevelBasedPermissionSet.GAMEMASTER);
            for (String input : ADVANCEMENT_COMMANDS) {
                emitAdvancement(out, dispatcher, source, input);
            }
            out.write("{\"type\":\"end\"}\n");
            out.flush();
        }
    }

    /**
     * 官方 Brigadier 对一整行 `/advancement` 的解析结果：能否解析、有没有剩余输入、
     * 以及失败时的光标。Rust 侧用同一组输入跑自己的命令树并比较。
     */
    private static void emitAdvancement(
            Writer out,
            CommandDispatcher<net.minecraft.commands.CommandSourceStack> dispatcher,
            net.minecraft.commands.CommandSourceStack source,
            String input) throws Exception {
        ParseResults<net.minecraft.commands.CommandSourceStack> parsed = dispatcher.parse(input, source);
        String result;
        if (parsed.getReader().canRead()) {
            result = "trailing:" + parsed.getReader().getCursor();
        } else if (parsed.getExceptions().isEmpty()) {
            result = "ok";
        } else {
            result = "error:" + parsed.getExceptions().size();
        }
        out.write("{\"type\":\"advancement\",\"input\":" + quote(input)
                + ",\"result\":" + quote(result) + "}\n");
    }

    /**
     * 用官方 `EntitySelectorParser` 解析选择器，再逐项问官方过滤谓词。
     * 实体本身用摆好字段的最小 `Entity` 子类，不需要 `ServerLevel`。
     */
    private static void emit(Writer out, String input) throws Exception {
        String result;
        try {
            EntitySelector selector = new EntitySelectorParser(new StringReader(input), true).parse();
            Vec3 origin = new Vec3(8.0, 0.0, 8.0);
            List<FakeEntity> candidates = new ArrayList<>();
            for (int id : IDS) {
                candidates.add(new FakeEntity(id));
            }
            List<FakeEntity> found = new ArrayList<>();
            for (FakeEntity entity : candidates) {
                if (predicate(selector, origin, entity)) {
                    found.add(entity);
                }
            }
            // `sortAndLimit` 的排序器只在多于一个候选时生效，然后截断。
            order(selector, origin, found);
            int limit = selector.getMaxResults();
            if (found.size() > limit) {
                found = new ArrayList<>(found.subList(0, limit));
            }
            List<String> names = new ArrayList<>();
            for (FakeEntity entity : found) {
                names.add(entity.name() + "@" + entity.id());
            }
            result = String.join(",", names);
        } catch (CommandSyntaxException error) {
            result = "error:" + error.getRawMessage().getString();
        }
        out.write("{\"type\":\"selector\",\"input\":" + quote(input)
                + ",\"result\":" + quote(result) + "}\n");
    }

    /** 官方 `EntitySelector.getPredicate` 的可见字段通过反射读出来逐项问。 */
    private static boolean predicate(EntitySelector selector, Vec3 origin, FakeEntity entity)
            throws ReflectiveOperationException {
        for (Object item : field(selector, "contextFreePredicates", List.class)) {
            @SuppressWarnings("unchecked")
            java.util.function.Predicate<Entity> test = (java.util.function.Predicate<Entity>) item;
            if (!test.test(entity)) {
                return false;
            }
        }
        Object range = field(selector, "range", Object.class);
        if (range != null && !((net.minecraft.advancements.predicates.MinMaxBounds.Doubles) range)
                .matchesSqr(entity.distanceToSqr(origin))) {
            return false;
        }
        Object box = field(selector, "aabb", Object.class);
        if (box != null) {
            net.minecraft.world.phys.AABB moved =
                    ((net.minecraft.world.phys.AABB) box).move(origin);
            if (!moved.intersects(entity.getBoundingBox())) {
                return false;
            }
        }
        return true;
    }

    /** 官方 `EntitySelector.sortAndLimit` 里的 order 是 `BiConsumer`。 */
    private static void order(EntitySelector selector, Vec3 origin, List<FakeEntity> found)
            throws ReflectiveOperationException {
        if (found.size() <= 1) {
            return;
        }
        @SuppressWarnings("unchecked")
        java.util.function.BiConsumer<Vec3, List<? extends Entity>> order =
                (java.util.function.BiConsumer<Vec3, List<? extends Entity>>) field(
                        selector, "order", Object.class);
        order.accept(origin, found);
    }

    private static <T> T field(Object owner, String name, Class<T> type)
            throws ReflectiveOperationException {
        java.lang.reflect.Field field = owner.getClass().getDeclaredField(name);
        field.setAccessible(true);
        return type.cast(field.get(owner));
    }

    /** 只有 `EntitySelector` 的选项会读到的字段：类型、名字、位置、标签、队伍、游戏模式。 */
    private static final class FakeEntity extends Entity {
        private static final net.minecraft.world.entity.EntityType<?> PLAYER =
                net.minecraft.world.entity.EntityTypes.PLAYER;
        private static final net.minecraft.world.entity.EntityType<?> COW =
                net.minecraft.world.entity.EntityTypes.COW;
        private static final net.minecraft.world.entity.EntityType<?> ITEM =
                net.minecraft.world.entity.EntityTypes.ITEM;

        private final int id;

        FakeEntity(int id) {
            super(typeOf(id), fakeLevel());
            this.id = id;
            setPos(switch (id) {
                case 1 -> 8.5;
                case 1 << 24 -> 9.5;
                default -> 10.5;
            }, 0.0, 8.5);
        }

        private static net.minecraft.world.entity.EntityType<?> typeOf(int id) {
            if (id == 1) {
                return PLAYER;
            }
            return id == (1 << 24) ? COW : ITEM;
        }

        int id() {
            return id;
        }

        String name() {
            return switch (id) {
                case 1 -> "player";
                case 1 << 24 -> "cow";
                default -> "item";
            };
        }

        @Override
        public int getId() {
            return id;
        }

        @Override
        protected void defineSynchedData(net.minecraft.network.syncher.SynchedEntityData.Builder builder) {
        }

        @Override
        protected void readAdditionalSaveData(net.minecraft.world.level.storage.ValueInput input) {
        }

        @Override
        protected void addAdditionalSaveData(net.minecraft.world.level.storage.ValueOutput output) {
        }

        @Override
        public boolean isAttackable() {
            return false;
        }

        @Override
        public boolean hurtServer(net.minecraft.server.level.ServerLevel level,
                net.minecraft.world.damagesource.DamageSource source, float amount) {
            return false;
        }

        @Override
        public net.minecraft.network.chat.Component getDisplayName() {
            return net.minecraft.network.chat.Component.literal(name());
        }
    }

    /** 官方 `Entity` 构造函数只要 `level.getNextEntityId()`；用一个最小假世界顶替。 */
    private static net.minecraft.server.level.ServerLevel fakeLevel() {
        try {
            return (net.minecraft.server.level.ServerLevel) UNSAFE.allocateInstance(FakeLevel.class);
        } catch (InstantiationException error) {
            throw new IllegalStateException(error);
        }
    }

    /** 只覆盖 `getNextEntityId()`，官方 `Entity` 构造函数不再碰区块缓存。 */
    private static final class FakeLevel extends net.minecraft.server.level.ServerLevel {
        private FakeLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false);
        }

        @Override
        public int getNextEntityId() {
            return 1;
        }
    }

    private static Unsafe loadUnsafe() {
        try {
            java.lang.reflect.Field field = Unsafe.class.getDeclaredField("theUnsafe");
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
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (c < 0x20) {
                        out.append(String.format("\\u%04x", (int) c));
                    } else {
                        out.append(c);
                    }
                }
            }
        }
        return out.append('"').toString();
    }
}
