//! 全项目共用的基础类型，对应官方的 `net.minecraft.core` 包。
//!
//! 协议层和世界层互不依赖，但都要表达“世界里的一个位置”，所以这些类型放在它们下面。

pub mod mth;

mod shape;

pub use shape::Aabb;

/// 方块坐标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// 朝 `direction` 方向相邻的方块。
    pub const fn relative(self, direction: Direction) -> Self {
        let (dx, dy, dz) = direction.normal();
        Self::new(self.x + dx, self.y + dy, self.z + dz)
    }

    /// 朝 `direction` 方向走 `distance` 格。
    pub const fn relative_by(self, direction: Direction, distance: i32) -> Self {
        let (dx, dy, dz) = direction.normal();
        self.offset(dx * distance, dy * distance, dz * distance)
    }

    /// 平移后的坐标。
    pub const fn offset(self, dx: i32, dy: i32, dz: i32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.z + dz)
    }

    pub const fn above(self) -> Self {
        self.relative(Direction::Up)
    }

    pub const fn below(self) -> Self {
        self.relative(Direction::Down)
    }

    pub const fn north(self) -> Self {
        self.relative(Direction::North)
    }

    pub const fn south(self) -> Self {
        self.relative(Direction::South)
    }

    pub const fn west(self) -> Self {
        self.relative(Direction::West)
    }

    pub const fn east(self) -> Self {
        self.relative(Direction::East)
    }

    /// 包含这个世界坐标的方块，官方的 `BlockPos.containing`。
    pub fn containing(position: Vec3) -> Self {
        Self::new(
            mth::floor(position.x),
            mth::floor(position.y),
            mth::floor(position.z),
        )
    }

    /// 这个方块最小的角，也就是它的世界坐标。
    pub fn corner(self) -> Vec3 {
        Vec3::new(f64::from(self.x), f64::from(self.y), f64::from(self.z))
    }

    /// 这个方块所在的区块。
    pub const fn chunk(self) -> ChunkPos {
        ChunkPos::new(self.x >> 4, self.z >> 4)
    }

    /// 网络上的编码：一个 64 位整数，从高位到低位是 `x(26) | z(26) | y(12)`。
    pub const fn pack(self) -> i64 {
        ((self.x as i64 & 0x3ff_ffff) << 38)
            | ((self.z as i64 & 0x3ff_ffff) << 12)
            | (self.y as i64 & 0xfff)
    }

    /// [`pack`](Self::pack) 的逆操作。`>>` 是算术右移，所以符号位会自动扩展。
    pub const fn unpack(packed: i64) -> Self {
        Self::new(
            (packed >> 38) as i32,
            (packed << 52 >> 52) as i32,
            (packed << 26 >> 38) as i32,
        )
    }
}

/// 区块坐标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

impl ChunkPos {
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// 世界坐标所在的区块。先向下取整，所以 `-0.5` 落在 -1 号区块。
    pub fn containing(x: f64, z: f64) -> Self {
        Self::new((x.floor() as i32) >> 4, (z.floor() as i32) >> 4)
    }
}

/// 世界坐标。玩家和实体的位置都用它，方块用 [`BlockPos`]。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub const fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    pub const fn subtract(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    pub const fn scale(self, factor: f64) -> Self {
        Self::new(self.x * factor, self.y * factor, self.z * factor)
    }

    /// 到 `other` 距离的平方。
    pub const fn distance_to_sqr(self, other: Self) -> f64 {
        let delta = self.subtract(other);
        delta.x * delta.x + delta.y * delta.y + delta.z * delta.z
    }

    /// `Entity.calculateViewVector`：`pitch` 和 `yaw` 对应的视线方向，长度是 1。
    pub fn view_vector(yaw: f32, pitch: f32) -> Self {
        let pitch_radians = pitch * DEG_TO_RAD;
        let yaw_radians = -yaw * DEG_TO_RAD;
        let yaw_cos = mth::cos(f64::from(yaw_radians));
        let yaw_sin = mth::sin(f64::from(yaw_radians));
        let pitch_cos = mth::cos(f64::from(pitch_radians));
        let pitch_sin = mth::sin(f64::from(pitch_radians));
        // 官方的乘法在 `float` 上做完才变成 `double`。
        Self::new(
            f64::from(yaw_sin * pitch_cos),
            f64::from(-pitch_sin),
            f64::from(yaw_cos * pitch_cos),
        )
    }
}

/// 官方写的是 `(float) (Math.PI / 180.0)`，比 `f32` 上的除法多一点精度。
const DEG_TO_RAD: f32 = (std::f64::consts::PI / 180.0) as f32;

/// 三根坐标轴。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub const fn is_vertical(self) -> bool {
        matches!(self, Self::Y)
    }

    pub const fn is_horizontal(self) -> bool {
        !self.is_vertical()
    }

    /// 朝这根轴正方向的面。
    pub const fn positive(self) -> Direction {
        match self {
            Self::X => Direction::East,
            Self::Y => Direction::Up,
            Self::Z => Direction::South,
        }
    }

    /// 朝这根轴负方向的面。
    pub const fn negative(self) -> Direction {
        match self {
            Self::X => Direction::West,
            Self::Y => Direction::Down,
            Self::Z => Direction::North,
        }
    }
}

/// 方块的六个面。枚举值就是网络上的 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Down = 0,
    Up = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

impl Direction {
    /// 全部六个面，顺序就是官方的 `Direction.values()`。
    pub const ALL: [Self; 6] = [
        Self::Down,
        Self::Up,
        Self::North,
        Self::South,
        Self::West,
        Self::East,
    ];

    /// `Direction.Plane.HORIZONTAL`，顺序和 `values()` 不同。
    pub const HORIZONTAL: [Self; 4] = [Self::North, Self::East, Self::South, Self::West];

    /// `Direction.Plane.VERTICAL`。
    pub const VERTICAL: [Self; 2] = [Self::Up, Self::Down];

    /// 官方 `data2d` 的顺序，`from_yaw` 用它。
    const BY_2D_DATA: [Self; 4] = [Self::South, Self::West, Self::North, Self::East];

    /// 从网络 ID 解码。越界时和官方一样取模回绕，不算错误。
    pub const fn from_id(id: i32) -> Self {
        Self::ALL[id.rem_euclid(6) as usize]
    }

    /// 网络 ID，官方的 `get3DDataValue`。
    pub const fn id(self) -> i32 {
        self as i32
    }

    /// 这个面的法向量。
    pub const fn normal(self) -> (i32, i32, i32) {
        match self {
            Self::Down => (0, -1, 0),
            Self::Up => (0, 1, 0),
            Self::North => (0, 0, -1),
            Self::South => (0, 0, 1),
            Self::West => (-1, 0, 0),
            Self::East => (1, 0, 0),
        }
    }

    /// 相反的面。
    pub const fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }

    /// 这个面所在的轴。
    pub const fn axis(self) -> Axis {
        match self {
            Self::Down | Self::Up => Axis::Y,
            Self::North | Self::South => Axis::Z,
            Self::West | Self::East => Axis::X,
        }
    }

    /// 朝轴的正方向。
    pub const fn is_positive(self) -> bool {
        matches!(self, Self::Up | Self::South | Self::East)
    }

    /// 从上往下看顺时针转 90 度。上下两面没有定义，和官方一样 panic。
    pub const fn clockwise(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::South => Self::West,
            Self::West => Self::North,
            Self::East => Self::South,
            Self::Down | Self::Up => panic!("vertical directions have no Y rotation"),
        }
    }

    /// 从上往下看逆时针转 90 度。上下两面没有定义，和官方一样 panic。
    pub const fn counter_clockwise(self) -> Self {
        match self {
            Self::North => Self::West,
            Self::South => Self::East,
            Self::West => Self::South,
            Self::East => Self::North,
            Self::Down | Self::Up => panic!("vertical directions have no Y rotation"),
        }
    }

    /// 绕 `axis` 顺时针转 90 度。这根轴上的两个面不动。
    pub const fn clockwise_around(self, axis: Axis) -> Self {
        match (axis, self) {
            (Axis::X, Self::West | Self::East)
            | (Axis::Y, Self::Up | Self::Down)
            | (Axis::Z, Self::North | Self::South) => self,
            (Axis::Y, _) => self.clockwise(),
            (Axis::X, Self::Down) => Self::South,
            (Axis::X, Self::Up) => Self::North,
            (Axis::X, Self::North) => Self::Down,
            (Axis::X, Self::South) => Self::Up,
            (Axis::Z, Self::Down) => Self::West,
            (Axis::Z, Self::Up) => Self::East,
            (Axis::Z, Self::West) => Self::Up,
            (Axis::Z, Self::East) => Self::Down,
        }
    }

    /// 绕 `axis` 逆时针转 90 度。这根轴上的两个面不动。
    pub const fn counter_clockwise_around(self, axis: Axis) -> Self {
        // 顺时针转三次就是逆时针转一次。
        self.clockwise_around(axis)
            .clockwise_around(axis)
            .clockwise_around(axis)
    }

    /// `Direction.fromYRot`：这个朝向最接近的水平面。
    pub fn from_yaw(yaw: f64) -> Self {
        let quarter = mth::floor(yaw / 90.0 + 0.5) & 3;
        Self::BY_2D_DATA[quarter as usize]
    }

    /// `Direction.toYRot`：这个水平面对应的朝向。上下两面的 `data2d` 是 -1，和官方一样算成 270 度。
    pub const fn to_yaw(self) -> f32 {
        match self {
            Self::South => 0.0,
            Self::West => 90.0,
            Self::North => 180.0,
            Self::East | Self::Up | Self::Down => 270.0,
        }
    }

    /// `Direction.orderedByNearest`：按离视线的远近排列六个面，第一个就是玩家朝着的面。
    ///
    /// 这里必须用查表的 `sin`、`cos`，否则边界角度上和客户端的预测不一致。
    pub fn ordered_by_nearest(yaw: f32, pitch: f32) -> [Self; 6] {
        let pitch_radians = f64::from(pitch * DEG_TO_RAD);
        let yaw_radians = f64::from(-yaw * DEG_TO_RAD);
        let pitch_sin = mth::sin(pitch_radians);
        let pitch_cos = mth::cos(pitch_radians);
        let yaw_sin = mth::sin(yaw_radians);
        let yaw_cos = mth::cos(yaw_radians);

        let x_positive = yaw_sin > 0.0;
        let y_positive = pitch_sin < 0.0;
        let z_positive = yaw_cos > 0.0;
        let x_yaw = if x_positive { yaw_sin } else { -yaw_sin };
        let y_magnitude = if y_positive { -pitch_sin } else { pitch_sin };
        let z_yaw = if z_positive { yaw_cos } else { -yaw_cos };
        let x_magnitude = x_yaw * pitch_cos;
        let z_magnitude = z_yaw * pitch_cos;

        let axis_x = if x_positive { Self::East } else { Self::West };
        let axis_y = if y_positive { Self::Up } else { Self::Down };
        let axis_z = if z_positive { Self::South } else { Self::North };

        if x_yaw > z_yaw {
            if y_magnitude > x_magnitude {
                Self::nearest_order(axis_y, axis_x, axis_z)
            } else if z_magnitude > y_magnitude {
                Self::nearest_order(axis_x, axis_z, axis_y)
            } else {
                Self::nearest_order(axis_x, axis_y, axis_z)
            }
        } else if y_magnitude > z_magnitude {
            Self::nearest_order(axis_y, axis_z, axis_x)
        } else if x_magnitude > y_magnitude {
            Self::nearest_order(axis_z, axis_x, axis_y)
        } else {
            Self::nearest_order(axis_z, axis_y, axis_x)
        }
    }

    const fn nearest_order(first: Self, second: Self, third: Self) -> [Self; 6] {
        [
            first,
            second,
            third,
            third.opposite(),
            second.opposite(),
            first.opposite(),
        ]
    }

    /// `Direction.getFacingAxis`：玩家在这根轴上朝着哪个方向。
    pub fn facing_axis(yaw: f32, pitch: f32, axis: Axis) -> Self {
        match axis {
            Axis::X if Self::East.is_facing_angle(yaw) => Self::East,
            Axis::X => Self::West,
            Axis::Y if pitch < 0.0 => Self::Up,
            Axis::Y => Self::Down,
            Axis::Z if Self::South.is_facing_angle(yaw) => Self::South,
            Axis::Z => Self::North,
        }
    }

    /// `Direction.isFacingAngle`。
    pub fn is_facing_angle(self, yaw: f32) -> bool {
        let radians = f64::from(yaw * DEG_TO_RAD);
        let dx = -mth::sin(radians);
        let dz = mth::cos(radians);
        let (normal_x, _, normal_z) = self.normal();
        normal_x as f32 * dx + normal_z as f32 * dz > 0.0
    }

    /// `Direction.getApproximateNearest`：和这个向量点积最大的面。
    pub fn nearest_to(x: f64, y: f64, z: f64) -> Self {
        let (x, y, z) = (x as f32, y as f32, z as f32);
        let mut result = Self::North;
        // 官方的初值是 `Float.MIN_VALUE`，也就是最小的正数，不是最小的负数。
        let mut highest = f32::from_bits(1);
        for direction in Self::ALL {
            let (normal_x, normal_y, normal_z) = direction.normal();
            let dot = x * normal_x as f32 + y * normal_y as f32 + z * normal_z as f32;
            if dot > highest {
                highest = dot;
                result = direction;
            }
        }
        result
    }

    /// 官方的序列化名，比如 `north`。
    pub const fn name(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Up => "up",
            Self::North => "north",
            Self::South => "south",
            Self::West => "west",
            Self::East => "east",
        }
    }
}

/// 两种光照，官方的 `LightLayer`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LightLayer {
    /// 天空光：从天上照下来，被挡住的地方往四周散开。
    Sky,
    /// 方块光：火把、萤石这些方块自己发的光。
    Block,
}

/// 玩家的两只手。枚举值就是网络上的 ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hand {
    Main = 0,
    Off = 1,
}

impl Hand {
    /// 从网络 ID 解码。官方把越界的值当作主手。
    pub const fn from_id(id: i32) -> Self {
        match id {
            1 => Self::Off,
            _ => Self::Main,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_pos_packs_like_the_game() {
        let cases = [
            (BlockPos::new(18, -61, -7), 0x0000_04bf_ffff_9fc3_u64),
            (BlockPos::new(-1, -64, 1), 0xffff_ffc0_0000_1fc0_u64),
        ];
        for (pos, packed) in cases {
            assert_eq!(pos.pack() as u64, packed, "packing {pos:?}");
            assert_eq!(
                BlockPos::unpack(packed as i64),
                pos,
                "unpacking {packed:#x}"
            );
        }
    }

    #[test]
    fn block_pos_finds_its_chunk_and_neighbours() {
        let pos = BlockPos::new(-1, 70, 16);

        assert_eq!(pos.chunk(), ChunkPos::new(-1, 1));
        assert_eq!(pos.relative(Direction::Up), BlockPos::new(-1, 71, 16));
        assert_eq!(pos.relative(Direction::West), BlockPos::new(-2, 70, 16));
    }

    #[test]
    fn chunk_pos_rounds_down() {
        assert_eq!(ChunkPos::containing(-0.5, 15.9), ChunkPos::new(-1, 0));
        assert_eq!(ChunkPos::containing(16.0, -16.1), ChunkPos::new(1, -2));
    }

    #[test]
    fn direction_ids_wrap_around() {
        assert_eq!(Direction::from_id(5), Direction::East);
        assert_eq!(Direction::from_id(6), Direction::Down);
        assert_eq!(Direction::from_id(-1), Direction::East);
    }

    #[test]
    fn opposite_faces_are_on_the_same_axis() {
        for direction in Direction::ALL {
            assert_eq!(direction.opposite().opposite(), direction);
            assert_eq!(direction.opposite().axis(), direction.axis());
            assert_ne!(direction.opposite().is_positive(), direction.is_positive());
        }
        assert_eq!(Axis::X.positive(), Direction::East);
        assert_eq!(Axis::Y.negative(), Direction::Down);
        assert!(Axis::Z.is_horizontal());
    }

    #[test]
    fn rotating_four_times_goes_back_to_the_start() {
        for direction in Direction::HORIZONTAL {
            assert_eq!(direction.clockwise().counter_clockwise(), direction);
            assert_eq!(direction.clockwise().clockwise(), direction.opposite());
        }
        assert_eq!(Direction::North.clockwise(), Direction::East);
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            for direction in Direction::ALL {
                let once = direction.clockwise_around(axis);
                assert_eq!(once.counter_clockwise_around(axis), direction);
            }
        }
        assert_eq!(Direction::Up.clockwise_around(Axis::X), Direction::North);
        assert_eq!(Direction::Up.clockwise_around(Axis::Z), Direction::East);
    }

    #[test]
    fn yaw_maps_to_the_nearest_horizontal_face() {
        assert_eq!(Direction::from_yaw(0.0), Direction::South);
        assert_eq!(Direction::from_yaw(90.0), Direction::West);
        assert_eq!(Direction::from_yaw(180.0), Direction::North);
        assert_eq!(Direction::from_yaw(-90.0), Direction::East);
        assert_eq!(Direction::from_yaw(44.9), Direction::South);
        assert_eq!(Direction::from_yaw(45.0), Direction::West);
        for direction in Direction::HORIZONTAL {
            assert_eq!(
                Direction::from_yaw(f64::from(direction.to_yaw())),
                direction
            );
        }
    }

    #[test]
    fn ordered_by_nearest_starts_with_the_looked_at_face() {
        assert_eq!(Direction::ordered_by_nearest(0.0, 0.0)[0], Direction::South);
        assert_eq!(Direction::ordered_by_nearest(90.0, 0.0)[0], Direction::West);
        assert_eq!(Direction::ordered_by_nearest(0.0, 90.0)[0], Direction::Down);
        assert_eq!(Direction::ordered_by_nearest(0.0, -90.0)[0], Direction::Up);

        let order = Direction::ordered_by_nearest(20.0, 30.0);
        for (index, direction) in order.iter().enumerate() {
            assert_eq!(order[5 - index], direction.opposite());
        }
    }

    /// 官方 `orderedByNearest` 在这张角度网格上的结果依次做 FNV-1a。
    #[test]
    fn ordered_by_nearest_matches_the_official_sweep() {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        let mut count = 0;
        for yaw in -1440..=1440 {
            for pitch in (-360..=360).step_by(3) {
                for direction in Direction::ordered_by_nearest(yaw as f32 / 4.0, pitch as f32 / 4.0)
                {
                    hash = (hash ^ direction.id() as u64).wrapping_mul(0x0100_0000_01b3);
                }
                count += 1;
            }
        }
        assert_eq!(count, 694321);
        assert_eq!(hash, 8944787946467644810);
    }

    #[test]
    fn facing_axis_follows_the_view() {
        assert_eq!(Direction::facing_axis(0.0, 10.0, Axis::Y), Direction::Down);
        assert_eq!(Direction::facing_axis(0.0, -10.0, Axis::Y), Direction::Up);
        assert_eq!(Direction::facing_axis(0.0, 0.0, Axis::Z), Direction::South);
        assert_eq!(
            Direction::facing_axis(180.0, 0.0, Axis::Z),
            Direction::North
        );
        assert_eq!(Direction::facing_axis(-90.0, 0.0, Axis::X), Direction::East);
    }

    #[test]
    fn nearest_face_needs_a_positive_dot_product() {
        assert_eq!(Direction::nearest_to(0.0, 2.0, 1.0), Direction::Up);
        assert_eq!(Direction::nearest_to(-3.0, 2.0, 1.0), Direction::West);
        assert_eq!(Direction::nearest_to(0.0, 0.0, 0.0), Direction::North);
    }

    #[test]
    fn view_vector_points_where_the_player_looks() {
        let south = Vec3::view_vector(0.0, 0.0);
        assert_eq!(south, Vec3::new(0.0, 0.0, 1.0));
        let down = Vec3::view_vector(0.0, 90.0);
        assert_eq!(down.y, -1.0);
        // 官方 `calculateViewVector(33.3, -123.4)` 的结果。
        assert_eq!(
            Vec3::view_vector(-123.4, 33.3),
            Vec3::new(0.6977880597114563, -0.5490164160728455, -0.4600791335105896)
        );
    }

    #[test]
    fn block_pos_steps_in_every_direction() {
        let pos = BlockPos::new(1, 2, 3);
        assert_eq!(pos.above(), BlockPos::new(1, 3, 3));
        assert_eq!(pos.below(), BlockPos::new(1, 1, 3));
        assert_eq!(pos.north(), BlockPos::new(1, 2, 2));
        assert_eq!(pos.south(), BlockPos::new(1, 2, 4));
        assert_eq!(pos.west(), BlockPos::new(0, 2, 3));
        assert_eq!(pos.east(), BlockPos::new(2, 2, 3));
        assert_eq!(pos.relative_by(Direction::Up, 3), BlockPos::new(1, 5, 3));
        assert_eq!(
            BlockPos::containing(Vec3::new(-0.5, 1.5, 2.0)),
            BlockPos::new(-1, 1, 2)
        );
    }

    #[test]
    fn unknown_hand_is_the_main_hand() {
        assert_eq!(Hand::from_id(1), Hand::Off);
        assert_eq!(Hand::from_id(7), Hand::Main);
    }
}
