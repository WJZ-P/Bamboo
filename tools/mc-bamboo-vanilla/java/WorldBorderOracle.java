import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import io.netty.buffer.Unpooled;
import java.io.BufferedWriter;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.Locale;
import java.util.Random;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.game.ClientboundInitializeBorderPacket;
import net.minecraft.network.protocol.game.ClientboundSetBorderCenterPacket;
import net.minecraft.network.protocol.game.ClientboundSetBorderLerpSizePacket;
import net.minecraft.network.protocol.game.ClientboundSetBorderSizePacket;
import net.minecraft.network.protocol.game.ClientboundSetBorderWarningDelayPacket;
import net.minecraft.network.protocol.game.ClientboundSetBorderWarningDistancePacket;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.entity.Marker;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.border.WorldBorder;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.phys.shapes.Shapes;
import net.minecraft.world.phys.shapes.VoxelShape;

/**
 * 用官方 26.3 的 {@code WorldBorder} 给 Bamboo 的 {@code world_border} 出题和给答案。
 *
 * <p>不启动服务端，不需要 ServerLevel，只调用 {@code WorldBorder} 自己的方法：随机的中心、大小、
 * 绝对最大值和渐变，加上随机点，比对距离、几种 {@code isWithinBounds}、{@code clampToBounds}、
 * 碰撞判断（{@code isInsideCloseToBorder} 和 {@code Shapes.collide}），以及逐刻渐变后的尺寸。
 * 另外把六个 clientbound 边界包按官方 {@code STREAM_CODEC} 编码成十六进制。
 *
 * <p>{@code isInsideCloseToBorder} 需要一个 Entity；这里用 Unsafe 造一个空的 {@code Marker}，
 * 只反射写 {@code Entity.position}，这样走的仍是官方那个方法本身。
 */
public final class WorldBorderOracle {
    private static final int OUTPUT_BUFFER = 1 << 20;
    private static final int POINTS = 8;
    private static final int CLOSE_QUERIES = 6;
    private static final int COLLIDE_QUERIES = 6;
    private static final int TICK_SAMPLES = 12;

    private WorldBorderOracle() {
    }

    public static void main(String[] args) throws Exception {
        PrintStream log = System.err;
        try {
            int cases = 400;
            long seed = 1L;
            for (int i = 0; i < args.length; i++) {
                switch (args[i]) {
                    case "--cases" -> cases = Integer.parseInt(args[++i]);
                    case "--seed" -> seed = Long.parseLong(args[++i]);
                    default -> throw new IllegalArgumentException(args[i]);
                }
            }
            run(cases, seed);
        } catch (Exception error) {
            error.printStackTrace(log);
            System.exit(1);
        }
        System.exit(0);
    }

    private static void run(int cases, long seed) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        Marker marker = emptyMarker();
        Field position = net.minecraft.world.entity.Entity.class.getDeclaredField("position");
        position.setAccessible(true);

        Random random = new Random(seed);
        try (Writer out = new BufferedWriter(
                new OutputStreamWriter(new FileOutputStream(FileDescriptor.out), StandardCharsets.UTF_8),
                OUTPUT_BUFFER)) {
            JsonObject header = new JsonObject();
            header.addProperty("type", "header");
            header.addProperty("version", SharedConstants.getCurrentVersion().id());
            header.addProperty("protocol", SharedConstants.getCurrentVersion().protocolVersion());
            header.addProperty("cases", cases);
            out.write(header + "\n");

            for (int index = 0; index < cases; index++) {
                out.write(caseJson(random, index, marker, position) + "\n");
            }

            JsonObject end = new JsonObject();
            end.addProperty("type", "end");
            end.addProperty("cases", cases);
            out.write(end + "\n");
        }
    }

    private static JsonObject caseJson(Random random, int index, Marker marker, Field position)
            throws Exception {
        double centerX = uniform(random, -5_000_000.0, 5_000_000.0);
        double centerZ = uniform(random, -5_000_000.0, 5_000_000.0);
        int absoluteMax = new int[] {100, 1_000, 29_999_984, 60_000_000}[random.nextInt(4)];
        double size = uniform(random, 1.0, 20_000_000.0);
        boolean lerp = random.nextBoolean();
        double from = size;
        double to = uniform(random, 1.0, 20_000_000.0);
        // 官方 `WorldBorderCommand.setSize` 只在 ticks > 0 时才开渐变；0 会让
        // `MovingBorderExtent.calculateSize` 除零，不是游戏里会出现的状态。
        //
        // Vanilla `WorldBorderCommand.setSize` only starts a lerp when ticks > 0;
        // zero would divide by zero in `MovingBorderExtent.calculateSize`.
        long ticks = 1 + random.nextInt(40);
        long gameTime = random.nextInt(100_000);
        double damage = uniform(random, 0.0, 10.0);
        double safeZone = uniform(random, 0.0, 20.0);
        int warningBlocks = random.nextInt(50);
        int warningTime = random.nextInt(600);

        WorldBorder border = new WorldBorder();
        border.applyInitialSettings(gameTime);
        border.setCenter(centerX, centerZ);
        border.setAbsoluteMaxSize(absoluteMax);
        border.setDamagePerBlock(damage);
        border.setSafeZone(safeZone);
        border.setWarningBlocks(warningBlocks);
        border.setWarningTime(warningTime);
        if (lerp) {
            border.lerpSizeBetween(from, to, ticks, gameTime);
        } else {
            border.setSize(size);
        }

        JsonObject root = new JsonObject();
        root.addProperty("type", "case");
        root.addProperty("index", index);
        root.addProperty("center_x", centerX);
        root.addProperty("center_z", centerZ);
        root.addProperty("absolute_max_size", absoluteMax);
        root.addProperty("damage_per_block", damage);
        root.addProperty("safe_zone", safeZone);
        root.addProperty("warning_blocks", warningBlocks);
        root.addProperty("warning_time", warningTime);
        root.addProperty("lerp", lerp);
        root.addProperty("size", size);
        root.addProperty("from", from);
        root.addProperty("to", to);
        root.addProperty("ticks", ticks);
        root.addProperty("game_time", gameTime);

        JsonArray points = new JsonArray();
        for (int i = 0; i < POINTS; i++) {
            double x = uniform(random, centerX - 25_000_000.0, centerX + 25_000_000.0);
            double z = uniform(random, centerZ - 25_000_000.0, centerZ + 25_000_000.0);
            double margin = uniform(random, 0.0, 10.0);
            double y = uniform(random, -64.0, 320.0);
            double half = uniform(random, 0.1, 5.0);
            AABB box = new AABB(x - half, y, z - half, x + half, y + 1.8, z + half);
            BlockPos block = BlockPos.containing(x, y, z);
            ChunkPos chunk = new ChunkPos(block.getX() >> 4, block.getZ() >> 4);

            JsonObject point = new JsonObject();
            point.addProperty("x", x);
            point.addProperty("z", z);
            point.addProperty("y", y);
            point.addProperty("margin", margin);
            point.add("box", numbers(box.minX, box.minY, box.minZ, box.maxX, box.maxY, box.maxZ));
            point.addProperty("block_x", block.getX());
            point.addProperty("block_z", block.getZ());
            point.addProperty("chunk_x", chunk.getMinBlockX() >> 4);
            point.addProperty("chunk_z", chunk.getMinBlockZ() >> 4);
            point.addProperty("inside", border.isWithinBounds(x, z));
            point.addProperty("inside_margin", border.isWithinBounds(x, z, margin));
            point.addProperty("inside_block", border.isWithinBounds(block));
            point.addProperty("inside_aabb", border.isWithinBounds(box));
            point.addProperty("inside_chunk", border.isWithinBounds(chunk));
            point.addProperty("distance", border.getDistanceToBorder(x, z));
            Vec3 clamped = border.clampVec3ToBound(new Vec3(x, y, z));
            point.add("clamp", numbers(clamped.x, clamped.y, clamped.z));
            BlockPos clampedPos = border.clampToBounds(new Vec3(x, y, z));
            point.add("clamp_pos", numbers(clampedPos.getX(), clampedPos.getY(), clampedPos.getZ()));
            points.add(point);
        }
        root.add("points", points);

        // `isInsideCloseToBorder(Entity, AABB)`：只用到实体的 X / Z。
        //
        // `isInsideCloseToBorder(Entity, AABB)`: only the entity's X / Z are read.
        JsonArray close = new JsonArray();
        for (int i = 0; i < CLOSE_QUERIES; i++) {
            // 一半贴着边界取点，才能测到 `true` 那一支。
            //
            // Half of the samples sit right on the edge so the `true` branch is covered.
            double x = i % 2 == 0 ? nearEdge(random, border, true)
                    : uniform(random, centerX - 25_000_000.0, centerX + 25_000_000.0);
            double z = i % 3 == 0 ? nearEdge(random, border, false)
                    : uniform(random, centerZ - 25_000_000.0, centerZ + 25_000_000.0);
            double half = uniform(random, 0.1, 8.0);
            AABB box = new AABB(x - half, 0.0, z - half, x + half, 1.8, z + half);
            position.set(marker, new Vec3(x, 64.0, z));
            JsonObject query = new JsonObject();
            query.addProperty("x", x);
            query.addProperty("z", z);
            query.add("box", numbers(box.minX, box.minY, box.minZ, box.maxX, box.maxY, box.maxZ));
            query.addProperty("inside", border.isInsideCloseToBorder(marker, box));
            close.add(query);
        }
        root.add("close", close);

        // `Shapes.collide` 只带边界形状，和 `collide_border_only` 的四堵墙对拍。
        //
        // `Shapes.collide` with only the border shape, compared against the four-wall
        // representation in `collide_border_only`.
        VoxelShape shape = border.getCollisionShape();
        JsonArray collides = new JsonArray();
        for (int i = 0; i < COLLIDE_QUERIES; i++) {
            // 贴着边界给盒子和小位移，墙才会真的挡住。
            //
            // Put the box and a small movement right at the edge so the wall blocks it.
            double x = nearEdge(random, border, true);
            double y = uniform(random, -64.0, 64.0);
            double z = i % 2 == 0 ? centerZ : nearEdge(random, border, false);
            AABB box = new AABB(x - 0.3, y, z - 0.3, x + 0.3, y + 1.8, z + 0.3);
            Vec3 movement = new Vec3(
                    uniform(random, -20.0, 20.0),
                    uniform(random, -5.0, 5.0),
                    uniform(random, -20.0, 20.0));
            Vec3 resolved = collide(shape, box, movement);
            JsonObject query = new JsonObject();
            query.add("box", numbers(box.minX, box.minY, box.minZ, box.maxX, box.maxY, box.maxZ));
            query.add("movement", numbers(movement.x, movement.y, movement.z));
            query.add("resolved", numbers(resolved.x, resolved.y, resolved.z));
            collides.add(query);
        }
        root.add("collides", collides);

        // 六个包按官方 `STREAM_CODEC` 编码，不含包 ID。
        //
        // The six packets encoded with vanilla `STREAM_CODEC`, without the packet ID.
        JsonArray packets = new JsonArray();
        addPacket(packets, "initialize",
                ClientboundInitializeBorderPacket.STREAM_CODEC,
                new ClientboundInitializeBorderPacket(border));
        addPacket(packets, "center",
                ClientboundSetBorderCenterPacket.STREAM_CODEC,
                new ClientboundSetBorderCenterPacket(border));
        addPacket(packets, "lerp_size",
                ClientboundSetBorderLerpSizePacket.STREAM_CODEC,
                new ClientboundSetBorderLerpSizePacket(border));
        addPacket(packets, "size",
                ClientboundSetBorderSizePacket.STREAM_CODEC,
                new ClientboundSetBorderSizePacket(border));
        addPacket(packets, "warning_delay",
                ClientboundSetBorderWarningDelayPacket.STREAM_CODEC,
                new ClientboundSetBorderWarningDelayPacket(border));
        addPacket(packets, "warning_distance",
                ClientboundSetBorderWarningDistancePacket.STREAM_CODEC,
                new ClientboundSetBorderWarningDistancePacket(border));
        root.add("packets", packets);

        // 逐刻推进渐变，记录每一步的可观察量。
        //
        // Advance the lerp one tick at a time and record the observable state.
        JsonArray steps = new JsonArray();
        int samples = (int) Math.min(TICK_SAMPLES, ticks + 3);
        for (int step = 0; step < samples; step++) {
            border.tick();
            JsonObject state = new JsonObject();
            state.addProperty("size", border.getSize());
            state.addProperty("lerp_time", border.getLerpTime());
            state.addProperty("lerp_target", border.getLerpTarget());
            state.addProperty("lerp_speed", border.getLerpSpeed());
            state.addProperty("min_x", border.getMinX());
            state.addProperty("max_x", border.getMaxX());
            state.addProperty("min_z", border.getMinZ());
            state.addProperty("max_z", border.getMaxZ());
            state.addProperty("status", border.getStatus().name());
            state.addProperty("clamp_x", (double) border.clampToBounds(new Vec3(centerX + 1.0e9, 64.0, centerZ)).getX());
            // `LevelChunk.isTicking(pos)` 的第一条：方块实体按这一刻的边界看自己的方块坐标。
            // 取四条边两侧各两格的方块，渐变中每一刻都比。
            //
            // The first clause of `LevelChunk.isTicking(pos)`: a block entity checks its own
            // block position against this tick's border. Blocks two either side of each edge.
            JsonArray ticking = new JsonArray();
            for (BlockPos block : edgeBlocks(border)) {
                JsonObject entry = new JsonObject();
                entry.addProperty("x", block.getX());
                entry.addProperty("z", block.getZ());
                entry.addProperty("inside", border.isWithinBounds(block));
                ticking.add(entry);
            }
            state.add("ticking_blocks", ticking);
            steps.add(state);
        }
        root.add("steps", steps);
        return root;
    }

    /** `Entity.collideWithShapes` 的单形状版本。 */
    private static Vec3 collide(VoxelShape shape, AABB box, Vec3 movement) {
        Vec3 resolved = Vec3.ZERO;
        for (Direction.Axis axis : Direction.axisStepOrder(movement)) {
            double amount = movement.get(axis);
            if (amount == 0.0) {
                continue;
            }
            AABB moved = box.move(resolved.x, resolved.y, resolved.z);
            double distance = Shapes.collide(axis, moved, List.of(shape), amount);
            resolved = resolved.with(axis, distance);
        }
        return resolved;
    }

    private static <T> void addPacket(JsonArray array, String kind, StreamCodec<FriendlyByteBuf, T> codec, T packet) {
        FriendlyByteBuf buffer = new FriendlyByteBuf(Unpooled.buffer());
        codec.encode(buffer, packet);
        byte[] bytes = new byte[buffer.readableBytes()];
        buffer.getBytes(buffer.readerIndex(), bytes);
        JsonObject entry = new JsonObject();
        entry.addProperty("kind", kind);
        entry.addProperty("bytes", hex(bytes));
        array.add(entry);
    }

    private static JsonArray numbers(double... values) {
        JsonArray array = new JsonArray();
        for (double value : values) {
            array.add(value);
        }
        return array;
    }

    private static String hex(byte[] bytes) {
        StringBuilder text = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) {
            text.append(String.format(Locale.ROOT, "%02x", value & 0xff));
        }
        return text.toString();
    }

    /** 四条边两侧各两格的方块坐标，另一轴取边界中心所在的方块。 */
    private static List<BlockPos> edgeBlocks(WorldBorder border) {
        int midX = (int) Math.floor(border.getCenterX());
        int midZ = (int) Math.floor(border.getCenterZ());
        java.util.ArrayList<BlockPos> out = new java.util.ArrayList<>();
        for (double edge : new double[] {border.getMinX(), border.getMaxX()}) {
            int base = (int) Math.floor(edge);
            for (int dx = -2; dx <= 2; dx++) {
                out.add(new BlockPos(base + dx, 64, midZ));
            }
        }
        for (double edge : new double[] {border.getMinZ(), border.getMaxZ()}) {
            int base = (int) Math.floor(edge);
            for (int dz = -2; dz <= 2; dz++) {
                out.add(new BlockPos(midX, 64, base + dz));
            }
        }
        return out;
    }

    /** 在 X 或 Z 的一条边界附近取点，偏移不超过 8 格。 */
    private static double nearEdge(Random random, WorldBorder border, boolean xAxis) {
        double edge = xAxis
                ? (random.nextBoolean() ? border.getMinX() : border.getMaxX())
                : (random.nextBoolean() ? border.getMinZ() : border.getMaxZ());
        return edge + uniform(random, -8.0, 8.0);
    }

    private static double uniform(Random random, double min, double max) {
        return min + (max - min) * random.nextDouble();
    }

    /** 用 Unsafe 造一个不跑构造函数的 Marker；`isInsideCloseToBorder` 只读它的坐标。 */
    private static Marker emptyMarker() throws Exception {
        Field f = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
        f.setAccessible(true);
        sun.misc.Unsafe unsafe = (sun.misc.Unsafe) f.get(null);
        return (Marker) unsafe.allocateInstance(Marker.class);
    }
}
