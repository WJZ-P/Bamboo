import java.util.Random;

/**
 * 生成几千条「挖掘速度」对拍用例，逐位对照 {@code Player.getDestroySpeed} 里
 * {@code ItemStack.getDestroySpeed} 之后的那一段。
 *
 * <p>这一段是纯 {@code float} 运算（属性的 {@code double} 已在
 * {@code getAttributeValue(...)} / {@code getValue()} 后 {@code d2f}），只有挖掘疲劳用了
 * {@code (float) Math.pow(0.3D, amplifier + 1)}。下面的算术逐条照 26.3
 * {@code Player.getDestroySpeed} 的字节码（{@code javap -c -p}）转写，没有抄反编译源码。
 * Java {@code float} 与 Rust {@code f32} 都是 IEEE-754 单精度、无扩展精度，逐位一致；
 * {@code Math.pow} 与 {@code f64::powf} 在本平台逐位一致（已单独核对 1..300）。
 *
 * <p>纯 Java，不依赖服务端、不加载注册表、不启动世界：{@code java MiningSpeedOracle.java}
 * 直接把表打到 stdout。固定种子，输出可复现。每行 11 个十进制整数：
 * <pre>
 * speed_bits mining_efficiency_bits has_dig_speed dig_amp has_fatigue fatigue_amp
 *     block_break_speed_bits submerged_mining_speed_bits eye_in_water on_ground result_bits
 * </pre>
 * 其中 {@code *_bits} 是 {@code Float.floatToRawIntBits} 的值；{@code has_fatigue=0} 时
 * {@code fatigue_amp} 无意义。Rust 侧用 {@code f32::from_bits} 还原输入，调
 * {@code destroy_speed_from(speed, &miner)}，比 {@code result.to_bits()}。
 *
 * <p>输入全部有限且有界，结果不会溢出成 NaN/Inf，避免两边 NaN 规范化的分歧；
 * 负的 mining_efficiency 只用于确认越界值不 panic。
 */
public final class MiningSpeedOracle {
    private static final int CASES = 4000;
    private static final long SEED = 0x4D494E45L; // "MINE"

    private MiningSpeedOracle() {
    }

    public static void main(String[] args) {
        Random random = new Random(SEED);
        StringBuilder out = new StringBuilder(CASES * 72);
        for (int i = 0; i < CASES; i++) {
            float speed = speed(random, i);
            float miningEfficiency = efficiency(random, i);
            boolean hasDigSpeed = random.nextBoolean();
            int digAmp = hasDigSpeed ? random.nextInt(256) : 0;
            boolean hasFatigue = random.nextBoolean();
            int fatigueAmp = hasFatigue ? random.nextInt(64) : 0;
            float blockBreakSpeed = multiplier(random, i + 1);
            float submergedSpeed = multiplier(random, i + 2);
            boolean eyeInWater = random.nextBoolean();
            boolean onGround = random.nextBoolean();

            // Player.getDestroySpeed from `fstore_2` (tool speed) onward.
            float s = speed;
            if (s > 1.0f) {
                s += miningEfficiency;
            }
            if (hasDigSpeed) {
                s *= 1.0f + (float) (digAmp + 1) * 0.2f;
            }
            if (hasFatigue) {
                s *= (float) Math.pow(0.3d, fatigueAmp + 1);
            }
            s *= blockBreakSpeed;
            if (eyeInWater) {
                s *= submergedSpeed;
            }
            if (!onGround) {
                s /= 5.0f;
            }

            out.append(Float.floatToRawIntBits(speed)).append(' ')
                    .append(Float.floatToRawIntBits(miningEfficiency)).append(' ')
                    .append(hasDigSpeed ? 1 : 0).append(' ')
                    .append(digAmp).append(' ')
                    .append(hasFatigue ? 1 : 0).append(' ')
                    .append(fatigueAmp).append(' ')
                    .append(Float.floatToRawIntBits(blockBreakSpeed)).append(' ')
                    .append(Float.floatToRawIntBits(submergedSpeed)).append(' ')
                    .append(eyeInWater ? 1 : 0).append(' ')
                    .append(onGround ? 1 : 0).append(' ')
                    .append(Float.floatToRawIntBits(s)).append('\n');
        }
        System.out.print(out);
    }

    /** 工具速度：覆盖 speed>1 的两侧边界、0、以及一段随机值。 */
    private static float speed(Random r, int i) {
        switch (i % 7) {
            case 0: return 1.0f;
            case 1: return 1.0000001f; // 刚好大于 1
            case 2: return 0.9999999f; // 刚好小于 1
            case 3: return 0.0f;
            case 4: return (float) r.nextInt(12);
            case 5: return r.nextFloat() * 2.0f;
            default: return r.nextFloat() * 8.0f;
        }
    }

    /** MINING_EFFICIENCY：0、1、效率 V 的 26、负数（防御）和随机值。 */
    private static float efficiency(Random r, int i) {
        switch (i % 6) {
            case 0: return 0.0f;
            case 1: return 1.0f;
            case 2: return 26.0f;
            case 3: return -2.0f;
            case 4: return r.nextFloat() * 30.0f;
            default: return r.nextFloat() * 3.0f;
        }
    }

    /** BLOCK_BREAK_SPEED / SUBMERGED_MINING_SPEED：0、1、0.2、放大值和随机值。 */
    private static float multiplier(Random r, int i) {
        switch (i % 6) {
            case 0: return 1.0f;
            case 1: return 0.0f;
            case 2: return 0.2f;
            case 3: return 5.0f;
            case 4: return r.nextFloat() * 4.0f;
            default: return r.nextFloat();
        }
    }
}
