import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Random;
import java.util.Set;
import java.util.concurrent.CompletableFuture;
import net.minecraft.SharedConstants;
import net.minecraft.core.SectionPos;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.level.ChunkHolder;
import net.minecraft.server.level.ChunkMap;
import net.minecraft.server.level.DistanceManager;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.server.level.Ticket;
import net.minecraft.server.level.TicketType;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.TicketStorage;

/**
 * 用官方 26.3 的 DistanceManager + TicketStorage 给 Bamboo 的 TicketTable 出题和给答案。
 *
 * <p>不启动服务端，不需要 ServerLevel。DistanceManager 是抽象类，这里继承它，把三个抽象方法
 * 接到一张假的 ChunkHolder 表上；ChunkMap 用 Unsafe.allocateInstance 跳过构造函数，只填
 * updatingChunkMap（purgeStaleTickets 和 ticketsToRelease 会读它）。玩家用同样的办法造空壳，
 * 只当集合里的身份用。
 *
 * <p>官方的 player_loading 票经过 ThrottlingChunkTaskDispatcher（每刻 4 个在途）。每个操作之后
 * settle() 反复跑 runAllUpdates 并释放在途任务，直到票发完、等级不动；比的是稳定状态。
 * 不含玩家票的操作可以逐刻比：settle 第一次 runAllUpdates 之后等级就已经定了。
 *
 * <p>每步输出窗口内全部区块的两个等级算出来的 64 位 FNV-1a 哈希，加上非默认区块数；
 * --detail 时再输出全部非默认区块的 [x,z,loading,simulation]。输出一行一个 JSON。
 */
public final class TicketOracle {
    /** 比较窗口的半径。票最远在 (±8, ±8)，等级 31 向外传 14 格到 45，24 足够。 */
    private static final int RADIUS = 24;
    /** 假 ChunkMap 里预置 Holder 的范围，比窗口大一圈。 */
    private static final int HOLDERS = 60;
    private static final long FNV_OFFSET = 0xcbf29ce484222325L;
    private static final long FNV_PRIME = 0x100000001b3L;
    private static final int OUTPUT_BUFFER = 1 << 20;

    private static final LevelHeightAccessor HEIGHT = LevelHeightAccessor.create(-64, 384);
    // 不能在 <clinit> 里碰 TicketType：它的静态初始化要注册表，而 Bootstrap 要等 main 里才跑。
    private static TicketType[] types() {
        return new TicketType[] {
            TicketType.PLAYER_SPAWN, TicketType.SPAWN_SEARCH, TicketType.DRAGON,
            TicketType.PLAYER_LOADING, TicketType.PLAYER_SIMULATION, TicketType.FORCED,
            TicketType.PORTAL, TicketType.ENDER_PEARL, TicketType.UNKNOWN
        };
    }

    private TicketOracle() {
    }

    public static void main(String[] args) {
        PrintStream log = System.err;
        try {
            int scenes = 400;
            long seed = 1L;
            boolean detail = false;
            int only = -1;
            for (int i = 0; i < args.length; i++) {
                switch (args[i]) {
                    case "--scenes" -> scenes = Integer.parseInt(args[++i]);
                    case "--seed" -> seed = Long.parseLong(args[++i]);
                    case "--scene" -> only = Integer.parseInt(args[++i]);
                    case "--detail" -> detail = true;
                    default -> throw new IllegalArgumentException(args[i]);
                }
            }
            run(scenes, seed, detail, only);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        // 官方的后台线程不是守护线程，不主动退出进程就不会结束。
        System.exit(0);
    }

    private static void run(int scenes, long seed, boolean detail, int only) throws Exception {
        FileOutputStream stdout = new FileOutputStream(FileDescriptor.out);
        // 官方日志默认写 stdout，要抢在日志框架初始化之前换掉，否则日志会混进 JSON。
        System.setOut(System.err);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        try (Writer out = new BufferedWriter(new OutputStreamWriter(stdout, StandardCharsets.UTF_8), OUTPUT_BUFFER)) {
            JsonObject header = new JsonObject();
            header.addProperty("type", "header");
            header.addProperty("version", SharedConstants.getCurrentVersion().id());
            header.addProperty("protocol", SharedConstants.getCurrentVersion().protocolVersion());
            header.addProperty("radius", RADIUS);
            out.write(header + "\n");

            int written = 0;
            int first = only < 0 ? 0 : only;
            int last = only < 0 ? scenes : only + 1;
            for (int scene = first; scene < last; scene++) {
                Scene s = new Scene(seed + scene * 0x9e3779b97f4a7c15L, scene, out, detail);
                s.play();
                written++;
            }
            JsonObject end = new JsonObject();
            end.addProperty("type", "end");
            end.addProperty("scenes", written);
            out.write(end + "\n");
            out.flush();
        }
    }

    /**
     * 假的 ChunkMap。实例用 allocateInstance 造（构造函数要真 ServerLevel），但类型是这个子类，
     * 于是 getUpdatingChunkIfPresent 能按需补 Holder，任何坐标都不会抛 IllegalStateException。
     */
    static final class StubMap extends ChunkMap {
        StubMap() { super(null, null, null, null, Runnable::run, null, null, null, null, null, 4, false); }
        it.unimi.dsi.fastutil.longs.Long2ObjectLinkedOpenHashMap<ChunkHolder> holders;
        @Override public ChunkHolder getUpdatingChunkIfPresent(long pos) {
            ChunkHolder holder = holders.get(pos);
            if (holder == null) {
                holder = new ChunkHolder(ChunkPos.unpack(pos), 45, HEIGHT, null, null, null);
                holders.put(pos, holder);
            }
            return holder;
        }
        @Override public void close() { }
    }

    /**
     * 攒着 Runnable 的执行器。官方主线把玩家票的增删 post 到主线程，等
     * updateViewDistance 把新的 viewDistance 字段写完才跑；同步执行器会让那批票全被
     * haveTicketFor 判掉。这里攒起来，在 settle 里按 FIFO 放行。
     */
    static final class Deferred implements java.util.concurrent.Executor {
        final java.util.ArrayDeque<Runnable> tasks = new java.util.ArrayDeque<>();
        @Override public void execute(Runnable task) { tasks.addLast(task); }
        void drain() {
            while (!tasks.isEmpty()) tasks.removeFirst().run();
        }
    }

    static Deferred currentDeferred;
    static java.util.concurrent.Executor pendingDeferred() {
        currentDeferred = new Deferred();
        return currentDeferred;
    }

    /** 直接驱动官方 DistanceManager 的假实现。 */
    static final class DM extends DistanceManager {
        final it.unimi.dsi.fastutil.longs.Long2ObjectLinkedOpenHashMap<ChunkHolder> holders;
        final Deferred deferred;

        DM(TicketStorage storage, it.unimi.dsi.fastutil.longs.Long2ObjectLinkedOpenHashMap<ChunkHolder> holders) {
            // 主线程执行器用延迟的；调度器自己的执行器保持同步，让 submit/release 立刻排队。
            super(storage, Runnable::run, pendingDeferred());
            this.holders = holders;
            this.deferred = currentDeferred;
        }

        void view(int distance) { updatePlayerTickets(distance); }
        void sim(int distance) { updateSimulationDistance(distance); }
        void add(SectionPos section, ServerPlayer player) { addPlayer(section, player); }
        void remove(SectionPos section, ServerPlayer player) { removePlayer(section, player); }

        @Override protected boolean isChunkToRemove(long pos) { return false; }
        @Override protected ChunkHolder getChunk(long pos) { return holders.get(pos); }
        @Override protected ChunkHolder updateChunkScheduling(long pos, int level, ChunkHolder holder, int old) {
            if (holder != null) holder.setTicketLevel(level);
            return null;
        }

        Object field(String name) {
            try {
                Field f = DistanceManager.class.getDeclaredField(name);
                f.setAccessible(true);
                return f.get(this);
            } catch (ReflectiveOperationException e) { throw new RuntimeException(e); }
        }

        @SuppressWarnings("unchecked")
        Set<Long> inExecution() {
            try {
                Object dispatcher = field("ticketDispatcher");
                Field f = dispatcher.getClass().getDeclaredField("chunkPositionsInExecution");
                f.setAccessible(true);
                Set<Long> out = new HashSet<>();
                for (Object value : (Set<?>) f.get(dispatcher)) out.add(((Number) value).longValue());
                return out;
            } catch (ReflectiveOperationException e) { throw new RuntimeException(e); }
        }

        void release(long pos) {
            try {
                Object dispatcher = field("ticketDispatcher");
                Method m = dispatcher.getClass().getMethod("release", long.class, Runnable.class, boolean.class);
                m.invoke(dispatcher, pos, (Runnable) () -> {}, false);
            } catch (ReflectiveOperationException e) { throw new RuntimeException(e); }
        }

        /** 跑完官方的等级传播和玩家票节流，直到在途任务清空。 */
        void settle(ChunkMap map) {
            for (int i = 0; i < 2000; i++) {
                ensureHolders();
                runAllUpdates(map);
                // 放行主线任务（真正增删 player_loading 票），再跑一次等级传播。
                if (!deferred.tasks.isEmpty()) {
                    deferred.drain();
                    runAllUpdates(map);
                }
                Set<Long> busy = inExecution();
                if (busy.isEmpty() && deferred.tasks.isEmpty()) return;
                for (long pos : busy) release(pos);
            }
            throw new IllegalStateException("player ticket throttle did not settle");
        }

        @SuppressWarnings("unchecked")
        void ensureHolders() {
            // ticketsToRelease 是 LongSet，元素是 long，遍历用 LongIterator。
            it.unimi.dsi.fastutil.longs.LongSet set =
                (it.unimi.dsi.fastutil.longs.LongSet) field("ticketsToRelease");
            it.unimi.dsi.fastutil.longs.LongIterator it = set.iterator();
            while (it.hasNext()) {
                long pos = it.nextLong();
                if (!holders.containsKey(pos)) {
                    holders.put(pos, new ChunkHolder(ChunkPos.unpack(pos), 45, HEIGHT, null, null, null));
                }
            }
        }

        int loading(long pos) { return getChunkLevel(pos, false); }
        int simulation(long pos) { return getChunkLevel(pos, true); }
    }

    static final class Scene {
        final Random random;
        final int index;
        final Writer out;
        final boolean detail;
        final TicketStorage storage = new TicketStorage();
        final it.unimi.dsi.fastutil.longs.Long2ObjectLinkedOpenHashMap<ChunkHolder> holders =
                new it.unimi.dsi.fastutil.longs.Long2ObjectLinkedOpenHashMap<>();
        final StubMap map;
        final DM dm;
        /** 玩家 id → (当前区块, 供 addPlayer/removePlayer 用的空壳)。 */
        final ServerPlayer[] players = new ServerPlayer[3];
        final ChunkPos[] positions = new ChunkPos[3];

        Scene(long seed, int index, Writer out, boolean detail) throws Exception {
            this.random = new Random(seed);
            this.index = index;
            this.out = out;
            this.detail = detail;
            for (int i = 0; i < players.length; i++) {
                players[i] = allocate(ServerPlayer.class);
                players[i].setId(1000 + i);
            }
            // updatingChunkMap 是 getUpdatingChunkIfPresent 唯一读的字段，预先铺满；
            // 窗口外的坐标在 DM.settle 里按需补。
            map = (StubMap) unsafe().allocateInstance(StubMap.class);
            map.holders = holders;
            Field f = ChunkMap.class.getDeclaredField("updatingChunkMap");
            f.setAccessible(true);
            f.set(map, holders);
            for (int x = -HOLDERS; x <= HOLDERS; x++) {
                for (int z = -HOLDERS; z <= HOLDERS; z++) {
                    long packed = ChunkPos.pack(x, z);
                    holders.put(packed, new ChunkHolder(ChunkPos.unpack(packed), 45, HEIGHT, null, null, null));
                }
            }
            dm = new DM(storage, holders);
            dm.view(viewDistance());
        }

        private int viewDistance() { return 4; }

        void play() throws Exception {
            int steps = 8 + random.nextInt(7);
            int simulationDistance = 10;
            for (int step = 0; step < steps; step++) {
                JsonObject op = new JsonObject();
                int kind = random.nextInt(100);
                if (kind < 30) {
                    addRemove(op, false);
                } else if (kind < 42) {
                    addRemove(op, true);
                } else if (kind < 58) {
                    movePlayer(op);
                } else if (kind < 66) {
                    int distance = 2 + random.nextInt(7);
                    op.addProperty("op", "view");
                    op.addProperty("distance", distance);
                    dm.view(distance);
                } else if (kind < 74) {
                    simulationDistance = random.nextInt(15);
                    op.addProperty("op", "sim");
                    op.addProperty("distance", simulationDistance);
                    dm.sim(simulationDistance);
                } else if (kind < 82) {
                    int x = randPos();
                    int z = randPos();
                    boolean value = random.nextBoolean();
                    op.addProperty("op", "forced");
                    op.addProperty("x", x);
                    op.addProperty("z", z);
                    op.addProperty("value", value);
                    storage.updateChunkForced(new ChunkPos(x, z), value);
                } else {
                    op.addProperty("op", "tick");
                    JsonArray busy = new JsonArray();
                    // 一半的题让若干区块"没准备好存盘"，覆盖 canTicketExpire 的两个分支。
                    if (random.nextBoolean()) {
                        for (int i = 0; i < 3; i++) {
                            int x = randPos();
                            int z = randPos();
                            JsonArray pair = new JsonArray();
                            pair.add(x);
                            pair.add(z);
                            busy.add(pair);
                            ChunkHolder holder = holders.get(ChunkPos.pack(x, z));
                            Field f = ChunkHolder.class.getDeclaredField("saveSync");
                            f.setAccessible(true);
                            f.set(holder, new CompletableFuture<Void>());
                        }
                    }
                    op.add("busy", busy);
                    storage.purgeStaleTickets(map);
                    for (int x = -HOLDERS; x <= HOLDERS; x++) {
                        for (int z = -HOLDERS; z <= HOLDERS; z++) {
                            ChunkHolder holder = holders.get(ChunkPos.pack(x, z));
                            Field f = ChunkHolder.class.getDeclaredField("saveSync");
                            f.setAccessible(true);
                            Object future = f.get(holder);
                            if (future instanceof CompletableFuture<?> cf && !cf.isDone()) cf.complete(null);
                        }
                    }
                }
                dm.settle(map);
                emit(step, op);
            }
        }

        private void addRemove(JsonObject op, boolean withRadius) {
            TicketType[] types = types();
            int type = random.nextInt(types.length);
            int x = randPos();
            int z = randPos();
            int value = withRadius ? random.nextInt(6) : 28 + random.nextInt(10);
            boolean add = random.nextInt(3) != 0;
            op.addProperty("op", withRadius ? (add ? "addRadius" : "removeRadius") : (add ? "add" : "remove"));
            op.addProperty("type", type);
            op.addProperty("x", x);
            op.addProperty("z", z);
            op.addProperty("value", value);
            ChunkPos pos = new ChunkPos(x, z);
            if (withRadius) {
                if (add) storage.addTicketWithRadius(types[type], pos, value);
                else storage.removeTicketWithRadius(types[type], pos, value);
            } else {
                Ticket ticket = new Ticket(types[type], value);
                if (add) storage.addTicket(ticket, pos);
                else storage.removeTicket(ticket, pos);
            }
        }

        private void movePlayer(JsonObject op) {
            int id = random.nextInt(players.length);
            boolean leave = positions[id] != null && random.nextInt(4) == 0;
            int x = randPos();
            int z = randPos();
            ChunkPos next = leave ? null : new ChunkPos(x, z);
            ChunkPos previous = positions[id];
            // SectionPos.of(int,int,int) 收的是区块段坐标；y=0，x/z 就等于区块坐标。
            if (previous != null) dm.remove(SectionPos.of(previous.x(), 0, previous.z()), players[id]);
            if (next != null) dm.add(SectionPos.of(next.x(), 0, next.z()), players[id]);
            // 玩家所在区块一定在 HOLDERS 内，留个断言免得坐标换算错了还看不出来。
            if (next != null && !holders.containsKey(next.pack())) {
                throw new IllegalStateException("missing holder at " + next);
            }
            positions[id] = next;
            op.addProperty("op", "player");
            op.addProperty("id", id);
            op.addProperty("has", next != null);
            op.addProperty("x", x);
            op.addProperty("z", z);
        }

        private int randPos() { return random.nextInt(13) - 6; }

        private void emit(int step, JsonObject op) throws Exception {
            long hash = FNV_OFFSET;
            JsonArray levels = new JsonArray();
            int count = 0;
            for (int x = -RADIUS; x <= RADIUS; x++) {
                for (int z = -RADIUS; z <= RADIUS; z++) {
                    long packed = ChunkPos.pack(x, z);
                    int loading = dm.loading(packed);
                    int simulation = dm.simulation(packed);
                    hash = mix(hash, x);
                    hash = mix(hash, z);
                    hash = mix(hash, loading);
                    hash = mix(hash, simulation);
                    if (loading != 45 || simulation != 33) {
                        count++;
                        if (detail) {
                            JsonArray entry = new JsonArray();
                            entry.add(x);
                            entry.add(z);
                            entry.add(loading);
                            entry.add(simulation);
                            levels.add(entry);
                        }
                    }
                }
            }
            JsonObject line = new JsonObject();
            line.addProperty("type", "step");
            line.addProperty("scene", index);
            line.addProperty("step", step);
            line.add("op", op);
            line.addProperty("hash", Long.toUnsignedString(hash));
            line.addProperty("count", count);
            if (detail) line.add("levels", levels);
            out.write(line + "\n");
        }
    }

    private static long mix(long hash, int value) {
        for (int i = 0; i < 4; i++) {
            hash ^= (value >>> (i * 8)) & 0xffL;
            hash *= FNV_PRIME;
        }
        return hash;
    }

    private static sun.misc.Unsafe unsafe() throws Exception {
        Field f = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
        f.setAccessible(true);
        return (sun.misc.Unsafe) f.get(null);
    }

    @SuppressWarnings("unchecked")
    private static <T> T allocate(Class<T> type) throws Exception {
        return (T) unsafe().allocateInstance(type);
    }
}
