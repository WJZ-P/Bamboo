//! 官方 `Mth` 里和方块判定有关的数学函数。
//!
//! 官方的 `sin`、`cos` 是查表的，结果和真正的三角函数不同。客户端用同一张表预测放置方向，
//! 服务端不用它的话，边界角度上两边会算出不同的朝向。

use std::sync::LazyLock;

/// 表的长度，一整圈被切成这么多份。
const SIN_ENTRIES: usize = 65536;
const SIN_MASK: i64 = 65535;
/// `cos(x) = sin(x + 四分之一圈)`。
const COS_OFFSET: f64 = 16384.0;
/// 弧度换算成表下标的比例。官方写的是这个字面量，不是 `65536 / 2π` 的计算结果。
const SIN_SCALE: f64 = 10430.378350470453;

static SIN: LazyLock<Box<[f32]>> = LazyLock::new(|| {
    (0..SIN_ENTRIES)
        .map(|index| (index as f64 / SIN_SCALE).sin() as f32)
        .collect()
});

/// `Mth.sin`。
pub fn sin(radians: f64) -> f32 {
    SIN[table_index(radians * SIN_SCALE)]
}

/// `Mth.cos`。
pub fn cos(radians: f64) -> f32 {
    SIN[table_index(radians * SIN_SCALE + COS_OFFSET)]
}

/// Java 把 `double` 转成 `long` 时向零截断，再取低 16 位。Rust 的 `as` 语义相同。
fn table_index(scaled: f64) -> usize {
    ((scaled as i64) & SIN_MASK) as usize
}

/// `Mth.floor`。
pub fn floor(value: f64) -> i32 {
    value.floor() as i32
}

/// `Mth.ceil`。
pub fn ceil(value: f64) -> i32 {
    value.ceil() as i32
}

/// `Mth.frac`。
pub fn frac(value: f64) -> f64 {
    value - value.floor()
}

/// `Mth.sign`：0 算 0，其余取符号。
pub fn sign(value: f64) -> i32 {
    if value == 0.0 {
        0
    } else if value > 0.0 {
        1
    } else {
        -1
    }
}

/// `Mth.lerp`。
pub fn lerp(factor: f64, from: f64, to: f64) -> f64 {
    from + factor * (to - from)
}

/// `Mth.wrapDegrees`：把角度折回 -180 到 180 之间，结果仍然可能是 180。
pub fn wrap_degrees(value: f32) -> f32 {
    let wrapped = value % 360.0;
    if wrapped >= 180.0 {
        wrapped - 360.0
    } else if wrapped < -180.0 {
        wrapped + 360.0
    } else {
        wrapped
    }
}

/// `Mth.degreesDifferenceAbs`：两个角度之间差多少度，最多 180。
pub fn degrees_difference_abs(first: f32, second: f32) -> f32 {
    wrap_degrees(second - first).abs()
}

/// `Mth.clamp`。比较的顺序和官方相同，所以 `NaN` 的结果也相同。
pub fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min { min } else { value.min(max) }
}

/// `Mth.getSeed`：按坐标算的随机种子，植物的渲染偏移靠它。
///
/// 第一个乘法在官方是 `int` 乘法，溢出后才扩展成 `long`，这里要保留这个行为。
pub const fn seed(x: i32, y: i32, z: i32) -> i64 {
    let mixed = (x.wrapping_mul(3129871) as i64) ^ (z as i64).wrapping_mul(116129781) ^ y as i64;
    let scrambled = mixed
        .wrapping_mul(mixed)
        .wrapping_mul(42317861)
        .wrapping_add(mixed.wrapping_mul(11));
    scrambled >> 16
}

/// `Mth.FRAC_BIAS`：`Double.longBitsToDouble(4805340802404319232L)`。
const FRAC_BIAS: f64 = f64::from_bits(4_805_340_802_404_319_232);

/// `Mth.ASIN_TAB`：`Math.asin(i / 256.0)` 的位模式，从官方 JVM 里读出来的。
/// 官方的 `Math.asin` 和 `Math.cos` 和 Rust 的 libm 在一些项上差最后一位，所以不现算。
///
/// `Mth.ASIN_TAB` bit patterns read from the official JVM; its `Math.asin` / `Math.cos`
/// differ from Rust's libm in the last bit for some entries, so they are not recomputed.
const ASIN_TAB_BITS: [u64; 257] = [
    0x0000000000000000,
    0x3f700002aaabdddf,
    0x3f80000aaabdde0c,
    0x3f8800240091cfda,
    0x3f90002aabdde94c,
    0x3f94005358ff0bd8,
    0x3f980090091d9024,
    0x3f9c00e4be5f0304,
    0x3fa000aabde0b9c8,
    0x3fa200f3229fdf2c,
    0x3fa4014d8ffaf8af,
    0x3fa601bc0922e634,
    0x3fa8024091fdb0a9,
    0x3faa02dd2f38ebf6,
    0x3fac0393e65c2c93,
    0x3fae0466bddb929c,
    0x3fb002abde953619,
    0x3fb103347666f892,
    0x3fb203ce2b380cd3,
    0x3fb3047a02794911,
    0x3fb405390240e6fd,
    0x3fb5060c31541da5,
    0x3fb606f49730ccc5,
    0x3fb707f33c173a99,
    0x3fb809092913e52e,
    0x3fb90a3768096840,
    0x3fba0b7f03ba78ac,
    0x3fbb0ce107d3f690,
    0x3fbc0e5e80f7172d,
    0x3fbd0ff87cc3a7a5,
    0x3fbe11b009e269b5,
    0x3fbf1386380f8b9a,
    0x3fc00abe0c129e1e,
    0x3fc08bc95e132e6f,
    0x3fc10ce59ba4a8c4,
    0x3fc18e134f0178af,
    0x3fc20f530308cc20,
    0x3fc290a543442d6a,
    0x3fc3120a9bed2f46,
    0x3fc3938399f32b5c,
    0x3fc41510cb011423,
    0x3fc496b2bd835ab9,
    0x3fc5186a00ade974,
    0x3fc59a37248233ea,
    0x3fc61c1ab9d55d30,
    0x3fc69e1552567517,
    0x3fc720278094cd3c,
    0x3fc7a251d80666ab,
    0x3fc82494ed0e78fc,
    0x3fc8a6f1550413c4,
    0x3fc92967a638db38,
    0x3fc9abf877ffe0e8,
    0x3fca2ea462b4998e,
    0x3fcab16bffc1f0dd,
    0x3fcb344fe9a97c4d,
    0x3fcbb750bc0acdef,
    0x3fcc3a6f13aae84b,
    0x3fccbdab8e7bd466,
    0x3fcd4106cba45b08,
    0x3fcdc4816b87e25e,
    0x3fce481c0fce7134,
    0x3fcecbd75b6cd8f0,
    0x3fcf4fb3f2ad079b,
    0x3fcfd3b27b368330,
    0x3fd02be9ce0b87cd,
    0x3fd06e0bfee5c057,
    0x3fd0b04025245ccc,
    0x3fd0f28696826d95,
    0x3fd134dfa9805147,
    0x3fd1774bb5687cf9,
    0x3fd1b9cb12545e62,
    0x3fd1fc5e19315893,
    0x3fd23f0523c5dc2b,
    0x3fd281c08cb69be8,
    0x3fd2c490af8bde81,
    0x3fd30775e8b6eeb9,
    0x3fd34a709597aab1,
    0x3fd38d8114823369,
    0x3fd3d0a7c4c4bd9c,
    0x3fd413e506ad84ee,
    0x3fd457393b90e2aa,
    0x3fd49aa4c5cf8926,
    0x3fd4de2808dce513,
    0x3fd521c36945a5f2,
    0x3fd565774cb66f02,
    0x3fd5a9441a02b1fd,
    0x3fd5ed2a392bb50f,
    0x3fd6312a1367c57c,
    0x3fd675441329986e,
    0x3fd6b978a427db95,
    0x3fd6fdc83364f719,
    0x3fd742332f3702b4,
    0x3fd786ba074fef93,
    0x3fd7cb5d2cc5e8ed,
    0x3fd8101d121bed2d,
    0x3fd854fa2b4aa1a3,
    0x3fd899f4edc962d3,
    0x3fd8df0dd0979384,
    0x3fd924454c462cc4,
    0x3fd9699bdb019139,
    0x3fd9af11f89ba61c,
    0x3fd9f4a82296347b,
    0x3fda3a5ed82d9537,
    0x3fda80369a63aaa8,
    0x3fdac62fec0b2a92,
    0x3fdb0c4b51d33b86,
    0x3fdb5289525368ab,
    0x3fdb98ea7617ef3a,
    0x3fdbdf6f47ae6904,
    0x3fdc261853b2d785,
    0x3fdc6ce628dd132c,
    0x3fdcb3d9580ea2b8,
    0x3fdcfaf27460fe9f,
    0x3fdd4232133444ad,
    0x3fdd8998cc3e6049,
    0x3fddd127399aabe7,
    0x3fde18ddf7da106b,
    0x3fde60bda613a78f,
    0x3fdea8c6e5f5e67f,
    0x3fdef0fa5bd85625,
    0x3fdf3958aecddef4,
    0x3fdf81e288b7ae20,
    0x3fdfca989658baaf,
    0x3fe009bdc3b4f877,
    0x3fe02e46075785a1,
    0x3fe052e571060fd4,
    0x3fe0779c5d4df4b8,
    0x3fe09c6b2a636bb7,
    0x3fe0c152382d7366,
    0x3fe0e651e85229ce,
    0x3fe10b6a9e43942f,
    0x3fe1309cbf4cdb24,
    0x3fe155e8b2a00052,
    0x3fe17b4ee1641318,
    0x3fe1a0cfb6c3e9eb,
    0x3fe1c66b9ffd666d,
    0x3fe1ec230c714a96,
    0x3fe211f66db3a5a1,
    0x3fe237e6379cdfc6,
    0x3fe25df2e05b6c41,
    0x3fe2841ce0862974,
    0x3fe2aa64b32f7783,
    0x3fe2d0cad5f90e20,
    0x3fe2f74fc9289adc,
    0x3fe31df40fbd31cd,
    0x3fe344b82f859adf,
    0x3fe36b9cb13786e1,
    0x3fe392a22087b7e9,
    0x3fe3b9c90c43296d,
    0x3fe3e11206694523,
    0x3fe4087da4473296,
    0x3fe4300c7e945024,
    0x3fe457bf318fe517,
    0x3fe47f965d201d78,
    0x3fe4a792a4f26152,
    0x3fe4cfb4b09d1a3e,
    0x3fe4f7fd2bc2fb34,
    0x3fe5206cc637e012,
    0x3fe5490434275b92,
    0x3fe571c42e3d0be7,
    0x3fe59aad71ced00f,
    0x3fe5c3c0c108f95c,
    0x3fe5ecfee31c96e7,
    0x3fe61668a46ffa82,
    0x3fe63ffed6d198f6,
    0x3fe669c251ad69e7,
    0x3fe693b3f244ee17,
    0x3fe6bdd49bea05ce,
    0x3fe6e825383cc40b,
    0x3fe712a6b76c6e92,
    0x3fe73d5a107bde74,
    0x3fe76840418978a7,
    0x3fe7935a501afa78,
    0x3fe7bea9496d5a54,
    0x3fe7ea2e42c9027a,
    0x3fe815ea59dab0a2,
    0x3fe841deb5114bb4,
    0x3fe86e0c84010764,
    0x3fe89a74ffcc34a4,
    0x3fe8c7196b9225de,
    0x3fe8f3fb14e496b4,
    0x3fe9211b54441083,
    0x3fe94e7b8da3cf7a,
    0x3fe97c1d30f5b7d2,
    0x3fe9aa01babef75e,
    0x3fe9d82ab4b5fdfd,
    0x3fea0699b66a8718,
    0x3fea355065f87fa4,
    0x3fea645078c6a78c,
    0x3fea939bb451e2a0,
    0x3feac333ef06451a,
    0x3feaf31b1127022e,
    0x3feb235315c680dc,
    0x3feb53de0bcffc24,
    0x3feb84be172438ef,
    0x3febb5f571cb0571,
    0x3febe7866d3b6480,
    0x3fec197373bc7bf1,
    0x3fec4bbf09e19830,
    0x3fec7e6bd023da76,
    0x3fecb17c849c7288,
    0x3fece4f404e29b3a,
    0x3fed18d55010f295,
    0x3fed4d2388f6360b,
    0x3fed81e1f875ea8d,
    0x3fedb714101e0a0e,
    0x3fedecbd6cf77786,
    0x3fee22e1da97bb17,
    0x3fee5985567b665d,
    0x3fee90ac13b18234,
    0x3feec85a7ee191da,
    0x3fef009542b712f2,
    0x3fef39614cbef7d4,
    0x3fef72c3d2c5752a,
    0x3fefacc258c4aaf9,
    0x3fefe762b77744d5,
    0x3ff0115591d29d12,
    0x3ff02f511b223c0f,
    0x3ff04da77ac5c9e5,
    0x3ff06c5c6f8ce9cc,
    0x3ff08b73f9af1058,
    0x3ff0aaf2613700b3,
    0x3ff0cadc3d4378b1,
    0x3ff0eb367c3fd618,
    0x3ff10c066d3e6932,
    0x3ff12d51caa6b58b,
    0x3ff14f1ec67484ed,
    0x3ff1717418520341,
    0x3ff194590de7e7f6,
    0x3ff1b7d59dd40ba2,
    0x3ff1dbf27dd2221a,
    0x3ff200b93cc5a540,
    0x3ff2263461820ad8,
    0x3ff24c6f8f6affb2,
    0x3ff27377b2570a1e,
    0x3ff29b5b338b7c8d,
    0x3ff2c42a3a3c7a87,
    0x3ff2edf6fac7f5b8,
    0x3ff318d619008ed9,
    0x3ff344df237486d7,
    0x3ff3722d2feb24c8,
    0x3ff3a0dfa4bb4aff,
    0x3ff3d11b3fc3b697,
    0x3ff4030b73c55372,
    0x3ff436e4418e69c0,
    0x3ff46ce4c738c4ea,
    0x3ff4a55ae332c7a5,
    0x3ff4e0a887c40a9c,
    0x3ff51f4bd13f8591,
    0x3ff561ebd9c18cd3,
    0x3ff5a96e34bc532b,
    0x3ff5f71d7ff42c8a,
    0x3ff64cf55148366e,
    0x3ff6ae4c63222736,
    0x3ff721a5d8718655,
    0x3ff7b7d33b928c5b,
    0x3ff921fb54442d18,
];

/// `Mth.COS_TAB`：参考 JVM 关闭 `Math.cos` 内建函数后导出的位模式。
///
/// `Mth.COS_TAB` bits exported from the reference JVM with the cosine intrinsic disabled.
const COS_TAB_BITS: [u64; 257] = [
    0x3ff0000000000000,
    0x3fefffeffffbfffe,
    0x3fefffbfffbfff80,
    0x3fefff6ffebbfa4e,
    0x3feffefffbffdfff,
    0x3feffe6ff63b85e7,
    0x3feffdbfebbe9360,
    0x3feffcefda786870,
    0x3feffbffbff7fec0,
    0x3feffaef996bc4e8,
    0x3feff9bf63a1740b,
    0x3feff86f1b05dfb0,
    0x3feff6febba4bfea,
    0x3feff56e412875ab,
    0x3feff3bda6d9c950,
    0x3feff1ece79fa355,
    0x3fefeffbfdfebf1f,
    0x3fefedeae41957e6,
    0x3fefebb993aecf99,
    0x3fefe968061b4fc8,
    0x3fefe6f634576477,
    0x3fefe46416f790d1,
    0x3fefe1b1a62bddad,
    0x3fefdeded9bf61d3,
    0x3fefdbeba917c3f5,
    0x3fefd8d80b34b63e,
    0x3fefd5a3f6af6b74,
    0x3fefd24f61ba0590,
    0x3fefceda421efdb5,
    0x3fefcb448d40857c,
    0x3fefc78e3817e16e,
    0x3fefc3b73734bca2,
    0x3fefbfbf7ebc755f,
    0x3fefbba7026962a7,
    0x3fefb76db58a1299,
    0x3fefb3138b008181,
    0x3fefae987541497f,
    0x3fefa9fc6652caa7,
    0x3fefa53f4fcc4b79,
    0x3fefa06122d5118c,
    0x3fef9b61d0237250,
    0x3fef964147fbdbbf,
    0x3fef90ff7a2fd4d2,
    0x3fef8b9c561cf5a1,
    0x3fef8617caabd6f6,
    0x3fef8071c64ef938,
    0x3fef7aaa3701a270,
    0x3fef74c10a46b354,
    0x3fef6eb62d27730d,
    0x3fef68898c325199,
    0x3fef623b1379a09b,
    0x3fef5bcaae92424e,
    0x3fef553848924e81,
    0x3fef4e83cc0fad49,
    0x3fef47ad231ea746,
    0x3fef40b437506b2f,
    0x3fef3998f1b1886c,
    0x3fef325b3ac85e81,
    0x3fef2afafa9380f9,
    0x3fef237818880fa4,
    0x3fef1bd27b9002c4,
    0x3fef140a0a086af1,
    0x3fef0c1ea9bfa45f,
    0x3fef04103ff37d41,
    0x3feefbdeb14f4eda,
    0x3feef389e1ea090d,
    0x3feeeb11b5442ff1,
    0x3feee2760e45cb23,
    0x3feed9b6cf3c4663,
    0x3feed0d3d9d8432f,
    0x3feec7cd0f2b5adf,
    0x3feebea24fa5d0ec,
    0x3feeb5537b1434da,
    0x3feeabe0709cf371,
    0x3feea2490ebdd6b8,
    0x3fee988d33497437,
    0x3fee8eacbb648910,
    0x3fee84a78383435d,
    0x3fee7a7d6766784b,
    0x3fee702e4218c668,
    0x3fee65b9edeba38e,
    0x3fee5b20447455cb,
    0x3fee50611e88d6b5,
    0x3fee457c543ca073,
    0x3fee3a71bcdd63de,
    0x3fee2f412eefa6f9,
    0x3fee23ea802b4b1a,
    0x3fee186d8577f9ec,
    0x3fee0cca12e97895,
    0x3fee00fffbbbe023,
    0x3fedf50f124fba75,
    0x3fede8f7282602ae,
    0x3feddcb80ddc085b,
    0x3fedd05193273445,
    0x3fedc3c386d0ae09,
    0x3fedb70db6b0e156,
    0x3fedaa2fefaae1d8,
    0x3fed9d29fda7ac9c,
    0x3fed8ffbab9145d5,
    0x3fed82a4c34db1c6,
    0x3fed75250db9c792,
    0x3fed677c52a3dc9b,
    0x3fed59aa58c6471c,
    0x3fed4baee5c1b694,
    0x3fed3d89be176072,
    0x3fed2f3aa522ff95,
    0x3fed20c15d14a4e5,
    0x3fed121da6ea5769,
    0x3fed034f42698214,
    0x3fecf455ee182d6d,
    0x3fece5316736032e,
    0x3fecd5e169b519d7,
    0x3fecc665b0328622,
    0x3fecb6bdf3eeb01f,
    0x3feca6e9ecc569b9,
    0x3fec96e95125c43e,
    0x3fec86bbd609a260,
    0x3fec76612eed0424,
    0x3fec65d90dc509f4,
    0x3fec552322f6abf5,
    0x3fec443f1d4d22af,
    0x3fec332ca9effdd0,
    0x3fec21eb7458e5cc,
    0x3fec107b264904d9,
    0x3febfedb67be13b3,
    0x3febed0bdee7064d,
    0x3febdb0c30185485,
    0x3febc8dbfdbfda88,
    0x3febb67ae8584caa,
    0x3feba3e88e5c39ec,
    0x3feb91248c38986b,
    0x3feb7e2e7c3ed68e,
    0x3feb6b05f6966b9b,
    0x3feb57aa912de205,
    0x3feb441bdfab5580,
    0x3feb3059735c5e90,
    0x3feb1c62db2564ff,
    0x3feb0837a370523a,
    0x3feaf3d7561a9c44,
    0x3feadf417a62a16d,
    0x3feaca7594d44cbd,
    0x3feab5732734fa47,
    0x3feaa039b06e926d,
    0x3fea8ac8ac79d249,
    0x3fea751f9447b724,
    0x3fea5f3dddaa0225,
    0x3fea4922fb3ac8c2,
    0x3fea32ce5c4305ef,
    0x3fea1c3f6ca01f29,
    0x3fea057594a84fc8,
    0x3fe9ee70390dec3c,
    0x3fe9d72ebac16dd6,
    0x3fe9bfb076d236eb,
    0x3fe9a7f4c64dfe15,
    0x3fe98ffafe1ece30,
    0x3fe977c26ee7878a,
    0x3fe95f4a64decda8,
    0x3fe9469227a84b4c,
    0x3fe92d98fa2c355d,
    0x3fe9145e1a6cf381,
    0x3fe8fae0c15ad38a,
    0x3fe8e12022a5ab3a,
    0x3fe8c71b6c8c49b4,
    0x3fe8acd1c7a997f1,
    0x3fe8924256bf4545,
    0x3fe8776c367dda86,
    0x3fe85c4e7d4a0bb1,
    0x3fe840e83aff1d1d,
    0x3fe8253878ae2e09,
    0x3fe8093e385a3700,
    0x3fe7ecf874b086df,
    0x3fe7d06620bd8524,
    0x3fe7b386279d7bf3,
    0x3fe796576c292765,
    0x3fe778d8c89dc27c,
    0x3fe75b090e40447a,
    0x3fe73ce704fb7b23,
    0x3fe71e716af8a794,
    0x3fe6ffa6f4323c0d,
    0x3fe6e0864a0050d6,
    0x3fe6c10e0a9e5d66,
    0x3fe6a13cc8a9b946,
    0x3fe681110a985d4d,
    0x3fe660894a2751cc,
    0x3fe63fa3f3c02962,
    0x3fe61e5f65d4d978,
    0x3fe5fcb9f031317b,
    0x3fe5dab1d341202a,
    0x3fe5b8453f4ae294,
    0x3fe59572539c229b,
    0x3fe572371da8f26e,
    0x3fe54e91981b7778,
    0x3fe52a7fa9d2f8ea,
    0x3fe505ff24d0e46b,
    0x3fe4e10dc51235a7,
    0x3fe4bba92f53830a,
    0x3fe495ceefbdc28b,
    0x3fe46f7c7879a3bc,
    0x3fe448af2027201c,
    0x3fe4216420369e50,
    0x3fe3f9989320b7f7,
    0x3fe3d14972795a11,
    0x3fe3a87394da947a,
    0x3fe37f13aba2fbb6,
    0x3fe355264082fea0,
    0x3fe32aa7b2d3fd8f,
    0x3fe2ff9434b34639,
    0x3fe2d3e7c7da540a,
    0x3fe2a79e3a2cd2e6,
    0x3fe27ab321f3dcf1,
    0x3fe24d21d9bcbd2d,
    0x3fe21ee57bd01fd8,
    0x3fe1eff8dd34fde3,
    0x3fe1c0568830ae9d,
    0x3fe18ff8b63353dc,
    0x3fe15ed9491d3888,
    0x3fe12cf1c3c6a214,
    0x3fe0fa3b41afe8ac,
    0x3fe0c6ae6dbb479d,
    0x3fe0924377cc95c0,
    0x3fe05cf20924c255,
    0x3fe026b137474aca,
    0x3fdfdeeeea5d2083,
    0x3fdf6e75051127be,
    0x3fdefbdeb14f4ed9,
    0x3fde87142982b1b2,
    0x3fde0ffbbe00c34f,
    0x3fdd96799ba6f66c,
    0x3fdd1a6f89821640,
    0x3fdc9bbc9bb903fc,
    0x3fdc1a3cd9862dfc,
    0x3fdb95c8d37c8daa,
    0x3fdb0e35269b38f4,
    0x3fda8351e7be222b,
    0x3fd9f4e9f1b5859e,
    0x3fd962c20e982fea,
    0x3fd8cc97f2912db7,
    0x3fd83220fb33564e,
    0x3fd79308a1dd964b,
    0x3fd6eeee8953be07,
    0x3fd645640568c1c3,
    0x3fd595e8ede2700c,
    0x3fd4dfe7790ba697,
    0x3fd422aeba618155,
    0x3fd35d6b2ed1914a,
    0x3fd28f1c6c540fb9,
    0x3fd1b6867c415165,
    0x3fd0d21c6aeb714f,
    0x3fcfbfbf7ebc755d,
    0x3fcdba59d10bfe0a,
    0x3fcb8cc9d3952a43,
    0x3fc92ca4f010700e,
    0x3fc689f26c6b01d5,
    0x3fc389d6226c1299,
    0x3fbfeffbfdfebf21,
    0x3fb69af589b35960,
    0x3c91a62633145c07,
];

/// 两张表一起建：反正弦和余弦。
type AtanTables = (Box<[f64]>, Box<[f64]>);

static ATAN_TABLES: LazyLock<AtanTables> = LazyLock::new(|| {
    let asin: Box<[f64]> = ASIN_TAB_BITS
        .iter()
        .map(|&bits| f64::from_bits(bits))
        .collect();
    let cos: Box<[f64]> = COS_TAB_BITS
        .iter()
        .map(|&bits| f64::from_bits(bits))
        .collect();
    (asin, cos)
});

/// 官方查表的两张反正弦表，对拍时和官方逐位比较用。
///
/// The two lookup tables behind `Mth.atan2`, exposed for the oracle to compare bit by bit.
pub fn atan_tables() -> (&'static [f64], &'static [f64]) {
    (&ATAN_TABLES.0, &ATAN_TABLES.1)
}

/// `Mth.SIN` 表本身，对拍时和官方逐位比较用。
///
/// The `Mth.SIN` table itself, for the oracle to compare bit by bit.
pub fn sin_table() -> &'static [f32] {
    &SIN
}

/// `Mth.fastInvSqrt`：一次牛顿迭代的平方根倒数。
pub fn fast_inv_sqrt(value: f64) -> f64 {
    let half = 0.5 * value;
    let bits = 6_910_469_410_427_058_090_i64.wrapping_sub((value.to_bits() as i64) >> 1);
    let guess = f64::from_bits(bits as u64);
    guess * (1.5 - half * guess * guess)
}

/// `Mth.atan2(y, x)`：官方查表近似，不是 `Math.atan2`。
///
/// `Mth.atan2(y, x)`: the table approximation, not `Math.atan2`.
pub fn atan2(y: f64, x: f64) -> f64 {
    let (mut y, mut x) = (y, x);
    let length_sqr = x * x + y * y;
    if length_sqr.is_nan() {
        return f64::NAN;
    }
    let negative_y = y < 0.0;
    if negative_y {
        y = -y;
    }
    let negative_x = x < 0.0;
    if negative_x {
        x = -x;
    }
    let steep = y > x;
    if steep {
        std::mem::swap(&mut x, &mut y);
    }
    let inverse = fast_inv_sqrt(length_sqr);
    x *= inverse;
    y *= inverse;
    let biased = FRAC_BIAS + y;
    let index = biased.to_bits() as i32 as usize;
    let (asin, cos) = (&ATAN_TABLES.0, &ATAN_TABLES.1);
    let asin_value = asin[index];
    let cos_value = cos[index];
    let sin_value = biased - FRAC_BIAS;
    let error = y * cos_value - x * sin_value;
    let correction = (6.0 + error * error) * error * 0.166_666_666_666_666_66;
    let mut result = asin_value + correction;
    if steep {
        result = std::f64::consts::FRAC_PI_2 - result;
    }
    if negative_x {
        result = std::f64::consts::PI - result;
    }
    if negative_y {
        result = -result;
    }
    result
}

/// `Mth.wrapDegrees(double)`。
pub fn wrap_degrees_f64(value: f64) -> f64 {
    let mut wrapped = value % 360.0;
    if wrapped >= 180.0 {
        wrapped -= 360.0;
    }
    if wrapped < -180.0 {
        wrapped += 360.0;
    }
    wrapped
}

/// `Mth.clamp(float, float, float)`：先比下界，再用 `Math.min`。
pub fn clamp_f32(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else {
        java_min_f32(value, max)
    }
}

/// Java 的 `Math.min(float, float)`：有 NaN 时得 NaN，`-0.0` 小于 `0.0`。
pub fn java_min_f32(first: f32, second: f32) -> f32 {
    if first.is_nan() || second.is_nan() {
        return f32::NAN;
    }
    if first == 0.0 && second == 0.0 {
        return if first.is_sign_negative() {
            first
        } else {
            second
        };
    }
    if first <= second { first } else { second }
}

/// Java 的 `Math.max(float, float)`。
pub fn java_max_f32(first: f32, second: f32) -> f32 {
    if first.is_nan() || second.is_nan() {
        return f32::NAN;
    }
    if first == 0.0 && second == 0.0 {
        return if first.is_sign_negative() {
            second
        } else {
            first
        };
    }
    if first >= second { first } else { second }
}

/// Java 的 `Math.max(double, double)`。
pub fn java_max(first: f64, second: f64) -> f64 {
    if first.is_nan() || second.is_nan() {
        return f64::NAN;
    }
    if first == 0.0 && second == 0.0 {
        return if first.is_sign_negative() {
            second
        } else {
            first
        };
    }
    if first >= second { first } else { second }
}

/// Java 的 `Math.min(double, double)`。
pub fn java_min(first: f64, second: f64) -> f64 {
    if first.is_nan() || second.is_nan() {
        return f64::NAN;
    }
    if first == 0.0 && second == 0.0 {
        return if first.is_sign_negative() {
            first
        } else {
            second
        };
    }
    if first <= second { first } else { second }
}

/// `Mth.lerp(float, float, float)`。
pub fn lerp_f32(factor: f32, from: f32, to: f32) -> f32 {
    from + factor * (to - from)
}

/// `Mth.degreesDifference`：`wrapDegrees(to - from)`。
pub fn degrees_difference(from: f32, to: f32) -> f32 {
    wrap_degrees(to - from)
}

/// `Mth.rotateIfNecessary`。
pub fn rotate_if_necessary(current: f32, target: f32, max: f32) -> f32 {
    let difference = degrees_difference(current, target);
    let clamped = clamp_f32(difference, -max, max);
    target - clamped
}

/// `Mth.positiveCeilDiv`：`-Math.floorDiv(-a, b)`。
pub const fn positive_ceil_div(value: i32, divisor: i32) -> i32 {
    -(-value).div_euclid(divisor)
}

/// `Mth.floor(float)`：先转成 `double` 再向下取整。
pub fn floor_f32(value: f32) -> i32 {
    f64::from(value).floor() as i32
}

/// `Mth.sqrt(float)`：按 `double` 开方再截回 `float`。
pub fn sqrt_f32(value: f32) -> f32 {
    f64::from(value).sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atan2_is_close_to_the_real_one_in_every_quadrant() {
        for (y, x) in [
            (1.0, 2.0),
            (-1.0, 2.0),
            (1.0, -2.0),
            (-3.0, -0.5),
            (0.0, 1.0),
        ] {
            let ours: f64 = atan2(y, x);
            assert!((ours - f64::atan2(y, x)).abs() < 1e-5, "{y} {x}");
        }
    }

    #[test]
    fn positive_ceil_div_rounds_up() {
        assert_eq!(positive_ceil_div(120, 2), 60);
        assert_eq!(positive_ceil_div(79, 2), 40);
        assert_eq!(positive_ceil_div(0, 2), 0);
    }

    /// 官方 `Mth.SIN` 全部 65536 个数的位模式依次做 FNV-1a 的结果。
    const OFFICIAL_TABLE_HASH: u64 = 2435040284621619975;

    fn table_hash() -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for value in SIN.iter() {
            hash = (hash ^ u64::from(value.to_bits())).wrapping_mul(0x0100_0000_01b3);
        }
        hash
    }

    #[test]
    fn sin_table_matches_the_official_one() {
        assert_eq!(table_hash(), OFFICIAL_TABLE_HASH);
    }

    #[test]
    fn table_lookup_is_not_the_exact_sine() {
        // 查表把角度截断到 1/65536 圈，这个角度上和真正的 sin 差了一点。
        let radians = 1.0_f64;
        assert_ne!(f64::from(sin(radians)), radians.sin());
        assert!((f64::from(sin(radians)) - radians.sin()).abs() < 1e-3);
    }

    #[test]
    fn cos_is_a_quarter_turn_ahead_of_sin() {
        assert_eq!(cos(0.0), 1.0);
        assert_eq!(sin(0.0), 0.0);
    }

    #[test]
    fn negative_angles_wrap_around_the_table() {
        assert_eq!(sin(-1.0), -sin(1.0));
    }

    #[test]
    fn seed_follows_the_int_overflow_of_the_first_multiply() {
        assert_eq!(seed(0, 0, 0), 0);
        // 这些数字来自官方 `Mth.getSeed`。
        assert_eq!(seed(1, 0, 1), 60165349410229);
        assert_eq!(seed(-1, 0, -1), 60165349410229);
        assert_eq!(seed(1000000, 64, -3000000), 80424206700872);
        assert_eq!(seed(5, 0, -7), -113315081188586);
        assert_eq!(seed(30000000, 0, 29999999), 20650537332405);
    }

    #[test]
    fn wrap_degrees_folds_angles_into_one_turn() {
        assert_eq!(wrap_degrees(0.0), 0.0);
        assert_eq!(wrap_degrees(90.0), 90.0);
        // 官方的判断是 `>= 180`，所以 180 折成 -180。
        assert_eq!(wrap_degrees(180.0), -180.0);
        assert_eq!(wrap_degrees(270.0), -90.0);
        assert_eq!(wrap_degrees(-270.0), 90.0);
        assert_eq!(wrap_degrees(720.0), 0.0);
        assert_eq!(degrees_difference_abs(170.0, -170.0), 20.0);
        assert_eq!(degrees_difference_abs(180.0, -180.0), 0.0);
    }

    #[test]
    fn sign_and_frac_follow_mth() {
        assert_eq!(sign(-0.5), -1);
        assert_eq!(sign(0.0), 0);
        assert_eq!(sign(3.0), 1);
        assert_eq!(frac(-0.25), 0.75);
        assert_eq!(floor(-0.25), -1);
    }
}
