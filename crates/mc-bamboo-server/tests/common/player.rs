//! 进服之后的假玩家。
//!
//! 后台任务一直读 socket，像真实客户端一样回心跳、确认传送和区块批次，所有收到的包按顺序记下来。
//! 测试只检查这份记录，所以同时有几名玩家时谁也不会因为没人读而超时。
//!
//! 所有等待都有时限。检查顺序靠 [`Player::expect`] 的游标：它只看游标之后的包，找到后把游标移到
//! 这个包后面，所以连续两次 `expect` 就是在断言先后。
//!
//! 「稳定下来」用 [`Player::settle`]：发一个由世界线程处理、一定回包的无副作用请求，
//! 等它的回包到了才继续。固定睡眠不保证服务端已经处理完，也不保证发给别的玩家
//! 的包已经送到。

use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI32, Ordering},
    },
    time::Duration,
};

use mc_bamboo_data::packets::play::{clientbound, serverbound};
use mc_bamboo_protocol::{
    codec::Reader,
    compression,
    frame::{MAX_FRAME_LEN, find_frame},
};
use md5::{Digest, Md5};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::tcp::{OwnedReadHalf, OwnedWriteHalf},
    sync::{Mutex as AsyncMutex, Notify},
    task::JoinHandle,
    time::{Instant, sleep, timeout_at},
};
use uuid::{Builder, Uuid};

use super::{
    Client, chunk_batch_received, encode_outgoing,
    wire::{
        AddEntity, AwardStats, BlockChangedAck, BlockUpdate, BundleDelimiter, CommandSuggestions,
        ContainerSetSlot, Disconnect, DisguisedChat, EntityPositionSync, ForgetLevelChunk, Frame,
        LevelChunk, LevelEvent, Login, MAIN_HAND, MoveEntityPos, MoveEntityPosRot, MoveEntityRot,
        Packet, PlayerChat, PlayerInfoRemove, PlayerInfoUpdate, PlayerPosition, Pos,
        RemoveEntities, RotateHead, START_DESTROY_BLOCK, SectionBlocksUpdate, SetChunkCacheCenter,
        SetEntityData, SetEquipment, SetHeldSlot, SwingAnimation, SystemChat, Tracked, UP,
        accept_teleportation_with_id, assert_bundles_paired, client_tick_end, command_suggestion,
        face_center, move_position, move_rotation, packet_name, player_action, player_loaded,
        replay_block, replay_entity, set_creative_mode_slot, tracked_chunks, use_item_on,
    },
};

/// 等一个包的时限。比 [`TEST_DEADLINE`](super::TEST_DEADLINE) 长：几组测试同时跑时机器很忙。
pub const DEADLINE: Duration = Duration::from_secs(10);
/// 直接读 socket 的测试断言“一段时间没有包”时等的时间，够世界线程跑 6 tick。
/// 假玩家的 [`Player::expect_none`] 不用固定睡眠，而是用一次世界线程往返做屏障。
pub const QUIET: Duration = Duration::from_millis(300);
/// 真实客户端一 tick 发一个移动包。
const CLIENT_TICK: Duration = Duration::from_millis(50);
const FAST_CHUNKS_PER_TICK: f32 = 64.0;

/// `UUIDUtil.createOfflinePlayerUUID`。
pub fn offline_uuid(name: &str) -> Uuid {
    let digest = Md5::digest(format!("OfflinePlayer:{name}"));
    Builder::from_md5_bytes(digest.into()).into_uuid()
}

/// 假客户端自动回应哪些包。默认全部回应，和真实客户端一样。
#[derive(Debug, Clone, Copy)]
pub struct Answers {
    pub keep_alive: bool,
    pub chunk_batches: bool,
    pub teleports: bool,
}

impl Default for Answers {
    fn default() -> Self {
        Self {
            keep_alive: true,
            chunk_batches: true,
            teleports: true,
        }
    }
}

#[derive(Default)]
struct Log {
    frames: Vec<Frame>,
    loaded_chunks: BTreeSet<(i32, i32)>,
    closed: bool,
}

struct Shared {
    log: Mutex<Log>,
    changed: Notify,
    /// 同步屏障用过的 transaction，每名玩家从 1 开始递增。
    next_barrier: AtomicI32,
    answer_keep_alive: AtomicBool,
    answer_chunk_batches: AtomicBool,
    answer_teleports: AtomicBool,
}

impl Shared {
    fn new(answers: Answers) -> Self {
        Self {
            log: Mutex::default(),
            changed: Notify::new(),
            next_barrier: AtomicI32::new(1),
            answer_keep_alive: AtomicBool::new(answers.keep_alive),
            answer_chunk_batches: AtomicBool::new(answers.chunk_batches),
            answer_teleports: AtomicBool::new(answers.teleports),
        }
    }

    fn record(&self, frame: Frame) {
        let mut log = self.log.lock().unwrap();
        match frame.id {
            clientbound::LEVEL_CHUNK_WITH_LIGHT => {
                let mut reader = Reader::new(&frame.body);
                let x = reader.i32().expect("chunk x");
                let z = reader.i32().expect("chunk z");
                log.loaded_chunks.insert((x, z));
            }
            clientbound::FORGET_LEVEL_CHUNK => {
                let packet: ForgetLevelChunk = frame.decode();
                log.loaded_chunks.remove(&(packet.x, packet.z));
            }
            _ => {}
        }
        log.frames.push(frame);
        drop(log);
        // `notify_one` 带一个 permit：包在等待者注册之前到达时也不会丢掉这次唤醒。
        self.changed.notify_one();
    }

    fn close(&self) {
        self.log.lock().unwrap().closed = true;
        self.changed.notify_waiters();
        // 留一个 permit：在 close 之后才开始等的 waiter 也能立刻看到连接已结束。
        self.changed.notify_one();
    }
}

type Writer = Arc<AsyncMutex<OwnedWriteHalf>>;

async fn read_loop(
    mut stream: OwnedReadHalf,
    mut buffer: Vec<u8>,
    writer: Writer,
    shared: Arc<Shared>,
    compression: Option<i32>,
) {
    loop {
        loop {
            let found = match find_frame(&buffer, MAX_FRAME_LEN) {
                Ok(found) => found,
                Err(_) => {
                    shared.close();
                    return;
                }
            };
            let Some(bounds) = found else {
                break;
            };
            let end = bounds.header_len + bounds.payload_len;
            let payload = &buffer[bounds.header_len..end];
            let payload = match compression {
                Some(threshold) => match compression::decode(payload, threshold) {
                    Ok(payload) => payload,
                    Err(_) => {
                        shared.close();
                        return;
                    }
                },
                None => payload.to_vec(),
            };
            buffer.drain(..end);
            let frame = Frame::parse(&payload);
            answer(&frame, &writer, &shared, compression).await;
            shared.record(frame);
        }
        if !matches!(stream.read_buf(&mut buffer).await, Ok(1..)) {
            shared.close();
            return;
        }
    }
}

/// 真实客户端会自动发的回应。写失败说明连接已经结束，读循环马上也会发现。
async fn answer(frame: &Frame, writer: &Writer, shared: &Shared, compression: Option<i32>) {
    let reply = match frame.id {
        clientbound::KEEP_ALIVE if shared.answer_keep_alive.load(Ordering::Relaxed) => {
            let mut payload = vec![serverbound::KEEP_ALIVE as u8];
            payload.extend_from_slice(&frame.body);
            super::frame(&payload)
        }
        clientbound::CHUNK_BATCH_FINISHED
            if shared.answer_chunk_batches.load(Ordering::Relaxed) =>
        {
            chunk_batch_received(FAST_CHUNKS_PER_TICK)
        }
        clientbound::PLAYER_POSITION if shared.answer_teleports.load(Ordering::Relaxed) => {
            let packet: PlayerPosition = frame.decode();
            accept_teleportation_with_id(packet.id, packet.x, packet.y, packet.z)
        }
        _ => return,
    };
    let reply = encode_outgoing(&reply, compression);
    let _ = writer.lock().await.write_all(&reply).await;
}

/// 已经进入 Play 状态的假玩家。
pub struct Player {
    pub name: String,
    pub uuid: Uuid,
    /// `login` 包里的实体 ID。
    pub entity_id: i32,
    /// 这名玩家最后发给服务端的位置。
    pub position: (f64, f64, f64),
    writer: Writer,
    shared: Arc<Shared>,
    task: JoinHandle<()>,
    cursor: usize,
    /// 上一次用的 sequence。真实客户端先加一再用，所以第一个值是 1。
    sequence: i32,
    /// 已经发过 `player_loaded`。没发的话服务端会丢掉移动。
    reported_load: bool,
    /// 登录时服务端声明的压缩阈值，关着压缩时是 `None`。
    compression: Option<i32>,
}

impl Drop for Player {
    /// 读任务一结束 socket 就关了，服务端看到的和强杀客户端进程一样。
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Player {
    /// 接管 `finish_configuration` 之后的连接，等到 `login`。游标留在最前面，测试能看到整个进服过程。
    pub async fn attach(client: Client, name: &str, answers: Answers) -> Self {
        let Client {
            stream,
            buffer,
            compression,
            ..
        } = client;
        let (read_half, write_half) = stream.into_split();
        let writer = Arc::new(AsyncMutex::new(write_half));
        let shared = Arc::new(Shared::new(answers));
        let task = tokio::spawn(read_loop(
            read_half,
            buffer,
            Arc::clone(&writer),
            Arc::clone(&shared),
            compression,
        ));

        let mut player = Self {
            name: name.to_owned(),
            uuid: offline_uuid(name),
            entity_id: 0,
            position: (8.5, -60.0, 8.5),
            writer,
            shared,
            task,
            cursor: 0,
            sequence: 0,
            compression,
            reported_load: false,
        };
        player.entity_id = player.expect::<Login>(|_| true).await.entity_id;
        player.cursor = 0;
        // 真实客户端画出世界之后发 `player_loaded`。没发的话服务端丢掉移动。
        //
        // A real client sends player_loaded once the world is on screen. Until then the
        // server drops movement.
        player.reported_load = true;
        player.send(&player_loaded()).await;
        player
    }

    /// 服务端可能已经把这名玩家踢了，写不进去也不算错。
    pub async fn send_ignoring_errors(&self, bytes: &[u8]) {
        let bytes = encode_outgoing(bytes, self.compression);
        let _ = self.writer.lock().await.write_all(&bytes).await;
    }

    pub async fn send(&self, bytes: &[u8]) {
        let bytes = encode_outgoing(bytes, self.compression);
        self.writer
            .lock()
            .await
            .write_all(&bytes)
            .await
            .unwrap_or_else(|error| panic!("{}: cannot write to the server: {error}", self.name));
    }

    /// 收到的全部包的副本。
    pub fn frames(&self) -> Vec<Frame> {
        self.shared.log.lock().unwrap().frames.clone()
    }

    /// 现在收到了多少个包，配合 [`since`](Self::since) 用。
    pub fn mark(&self) -> usize {
        self.shared.log.lock().unwrap().frames.len()
    }

    /// 把游标移到最新，只看从现在开始收到的包。
    pub fn skip_received(&mut self) {
        self.cursor = self.mark();
    }

    /// `mark` 之后收到的所有 `P`。
    pub fn since<P: Packet>(&self, mark: usize) -> Vec<P> {
        let log = self.shared.log.lock().unwrap();
        log.frames[mark..]
            .iter()
            .filter(|frame| frame.is::<P>())
            .map(Frame::decode)
            .collect()
    }

    /// `mark` 之后收到的所有包的 ID。
    pub fn ids_since(&self, mark: usize) -> Vec<i32> {
        let log = self.shared.log.lock().unwrap();
        log.frames[mark..].iter().map(|frame| frame.id).collect()
    }

    /// 等服务端把之前的所有入站请求处理完、把它们的回包都送给这名玩家。
    ///
    /// `command_suggestion` 由世界线程按入站顺序处理，而且一定回一个 ID 相同的包；它不改
    /// 游戏状态。回包和之前发给这名玩家的所有包走同一条出站队列，所以收到它时，之前
    /// 世界线程发出的包一定已经被读循环记下来了。心跳和 ping 由连接任务就地处理，
    /// 不经过世界线程，不能用来当屏障。
    pub async fn settle(&self) {
        let transaction = self.shared.next_barrier.fetch_add(1, Ordering::Relaxed);
        // 先记下位置：负载高时回包可能在 send 返回之前就已经被读循环记下来了。
        let from = self.mark();
        self.send(&command_suggestion(transaction, "")).await;
        let what = format!("the barrier reply {transaction}");
        self.poll(&what, from, |log| {
            log.frames[from..]
                .iter()
                .any(|frame| {
                    frame.is::<CommandSuggestions>()
                        && Frame::decode::<CommandSuggestions>(frame).transaction == transaction
                })
                .then_some(())
        })
        .await;
    }

    fn describe_since(&self, mark: usize) -> String {
        describe(&self.shared.log.lock().unwrap().frames[mark..])
    }

    /// 反复调用 `check`，有新包或连接结束时才再查一次，超过时限或连接结束时让测试失败。
    /// 先注册通知再检查日志，所以新包夹在两者之间到达时也不会白等一轮。
    async fn poll<R>(
        &self,
        what: &str,
        from: usize,
        mut check: impl FnMut(&Log) -> Option<R>,
    ) -> R {
        let deadline = Instant::now() + DEADLINE;
        loop {
            let changed = self.shared.changed.notified();
            tokio::pin!(changed);
            let closed = {
                let log = self.shared.log.lock().unwrap();
                if let Some(result) = check(&log) {
                    return result;
                }
                log.closed
            };
            assert!(
                !closed,
                "{}: the connection ended while waiting for {what}, received: {}",
                self.name,
                self.describe_since(from)
            );
            assert!(
                timeout_at(deadline, changed).await.is_ok(),
                "{}: {what} did not happen within {DEADLINE:?}, received: {}",
                self.name,
                self.describe_since(from)
            );
        }
    }

    /// 等游标之后第一个符合条件的 `P`，把游标移到它后面。
    pub async fn expect<P: Packet>(&mut self, matches: impl Fn(&P) -> bool) -> P {
        let mut next = self.cursor;
        let what = format!("a matching {}", packet_name(P::ID));
        let (packet, after) = self
            .poll(&what, self.cursor, |log| {
                while let Some(frame) = log.frames.get(next) {
                    next += 1;
                    if frame.is::<P>() {
                        let packet: P = frame.decode();
                        if matches(&packet) {
                            return Some((packet, next));
                        }
                    }
                }
                None
            })
            .await;
        self.cursor = after;
        packet
    }

    /// 等世界线程处理完之前的请求、把它们的回包都送到，再断言 `mark` 之后没有 `P`。
    pub async fn expect_none<P: Packet>(&self, mark: usize, matches: impl Fn(&P) -> bool) {
        self.settle().await;
        let found = self.since::<P>(mark).iter().any(matches);
        assert!(
            !found,
            "{}: got an unexpected {}, received: {}",
            self.name,
            packet_name(P::ID),
            self.describe_since(mark)
        );
    }

    /// 等到 `check` 对收到的全部包成立。
    pub async fn wait_until(&self, what: &str, check: impl Fn(&[Frame]) -> bool) {
        let from = self.mark();
        self.poll(what, from, |log| check(&log.frames).then_some(()))
            .await;
    }

    /// 等服务端关闭连接。
    pub async fn wait_closed(&self) {
        let deadline = Instant::now() + DEADLINE;
        loop {
            let changed = self.shared.changed.notified();
            if self.shared.log.lock().unwrap().closed {
                return;
            }
            assert!(
                timeout_at(deadline, changed).await.is_ok(),
                "{}: the connection stayed open",
                self.name
            );
        }
    }

    pub fn is_closed(&self) -> bool {
        self.shared.log.lock().unwrap().closed
    }

    /// 等客户端手里的区块正好是 `T(center, view_distance)`。
    pub async fn wait_for_chunks(&self, center: (i32, i32), view_distance: i32) {
        let expected: BTreeSet<(i32, i32)> =
            tracked_chunks(center, view_distance).into_iter().collect();
        let what = format!("the chunks around {center:?}");
        self.poll(&what, 0, |log| {
            (log.loaded_chunks == expected).then_some(())
        })
        .await;
    }

    /// 这名玩家眼里 `entity` 现在的样子。
    pub fn tracked(&self, entity: i32) -> Option<Tracked> {
        replay_entity(&self.shared.log.lock().unwrap().frames, entity)
    }

    /// 这名玩家眼里这一格的方块状态，还没收到这个区块时是 `None`。
    pub fn block_at(&self, pos: Pos) -> Option<u32> {
        replay_block(&self.shared.log.lock().unwrap().frames, pos)
    }

    /// 最新收到的 `pos` 所在的区块。
    pub fn chunk_at(&self, pos: Pos) -> LevelChunk {
        let log = self.shared.log.lock().unwrap();
        log.frames
            .iter()
            .rev()
            .filter(|frame| frame.is::<LevelChunk>())
            .map(Frame::decode::<LevelChunk>)
            .find(|chunk| (chunk.x, chunk.z) == pos.chunk())
            .unwrap_or_else(|| panic!("{}: never received {:?}", self.name, pos.chunk()))
    }

    /// 不再自动回心跳，服务端会以为这个客户端挂了。
    pub fn stop_answering_keep_alives(&self) {
        self.shared
            .answer_keep_alive
            .store(false, Ordering::Relaxed);
    }

    /// 补上一次批次确认，之后自动确认。
    pub async fn start_acknowledging_chunks(&self) {
        self.shared
            .answer_chunk_batches
            .store(true, Ordering::Relaxed);
        self.send(&chunk_batch_received(FAST_CHUNKS_PER_TICK)).await;
    }

    /// 走到这里，脚踩地面。太远就拆成服务端一个 tick 能接受的几步。
    pub async fn move_to(&mut self, x: f64, y: f64, z: f64) {
        self.travel(x, y, z, true).await;
    }

    /// 飞到这里，脚不着地。太远就拆成服务端一个 tick 能接受的几步。
    pub async fn fly_to(&mut self, x: f64, y: f64, z: f64) {
        self.travel(x, y, z, false).await;
    }

    /// 一步最多 10 格，正好是速度检查放行的距离。
    ///
    /// 每一步都做一次经过世界线程的往返：这样服务端一定已经处理完这一步的移动包，
    /// 下一步的速度检查不会把几步挤进同一个 tick（高负载时固定 50ms 的睡眠做不到）。
    async fn travel(&mut self, x: f64, y: f64, z: f64, on_ground: bool) {
        if !self.reported_load {
            self.reported_load = true;
            self.send(&player_loaded()).await;
        }
        let (mut cx, mut cy, mut cz) = self.position;
        loop {
            let dx = x - cx;
            let dy = y - cy;
            let dz = z - cz;
            let distance_sqr = dx * dx + dy * dy + dz * dz;
            // 正好 10 格（平方 100）官方放行。再远才拆开，避免一个合法的包被拆成中间位置。
            //
            // Exactly 10 blocks (squared distance 100) is allowed. Split only past that, so a
            // legal packet is not turned into intermediate positions.
            if distance_sqr <= 100.0 {
                break;
            }
            let scale = 10.0 / distance_sqr.sqrt();
            cx += dx * scale;
            cy += dy * scale;
            cz += dz * scale;
            self.send(&move_position(cx, cy, cz, false)).await;
            self.send(&client_tick_end()).await;
            self.settle().await;
        }
        self.position = (x, y, z);
        self.send(&move_position(x, y, z, on_ground)).await;
        self.send(&client_tick_end()).await;
        self.settle().await;
    }

    /// 走到 (x, z)，一 tick 走 `step` 格，和真实客户端一样每 tick 一个包。
    pub async fn walk_to(&mut self, x: f64, z: f64, step: f64) {
        let (start_x, y, start_z) = self.position;
        let distance = (x - start_x).hypot(z - start_z);
        let steps = (distance / step).ceil().max(1.0) as i32;
        for index in 1..=steps {
            let progress = f64::from(index) / f64::from(steps);
            let next_x = start_x + (x - start_x) * progress;
            let next_z = start_z + (z - start_z) * progress;
            self.move_to(next_x, y, next_z).await;
            sleep(CLIENT_TICK).await;
        }
    }

    pub async fn turn(&self, yaw: f32, pitch: f32) {
        self.send(&move_rotation(yaw, pitch, true)).await;
    }

    /// 世界线程还在转：一次经过世界线程的往返能回来就说明它既没卡住也没掉包。
    ///
    /// 以前这里靠走到隔壁区块等 `set_chunk_cache_center`，但移动包在负载下可能被速度检查
    /// 拒绝、也可能在 `skip_received` 之前就到了，那个包就会永远等不到。
    pub async fn assert_world_ticks(&self) {
        self.settle().await;
    }

    /// 下一个 sequence，真实客户端开始预测时才加一。
    pub fn next_sequence(&mut self) -> i32 {
        self.sequence += 1;
        self.sequence
    }

    /// 把物品放进快捷栏第 `hotbar_slot` 格。
    pub async fn put_in_hotbar(&self, hotbar_slot: i16, item: i32, count: i32) {
        self.send(&set_creative_mode_slot(36 + hotbar_slot, item, count))
            .await;
    }

    /// 左键破坏方块，返回用掉的 sequence。
    pub async fn break_block(&mut self, pos: Pos) -> i32 {
        let sequence = self.next_sequence();
        self.send(&player_action(START_DESTROY_BLOCK, pos, UP, sequence))
            .await;
        sequence
    }

    /// 主手右键 `pos` 的 `face` 面中心，返回用掉的 sequence。
    pub async fn click(&mut self, pos: Pos, face: u8) -> i32 {
        let sequence = self.next_sequence();
        self.send(&use_item_on(
            MAIN_HAND,
            pos,
            face,
            face_center(face),
            sequence,
        ))
        .await;
        sequence
    }

    /// 等到确认了 `sequence` 的 `block_changed_ack`，并等世界线程把这一 tick 的
    /// 其它回包（方块更新、世界事件等）都送到，之后断言才不会漏掉在途的包。
    pub async fn expect_ack(&mut self, sequence: i32) {
        self.expect::<BlockChangedAck>(|ack| ack.sequence >= sequence)
            .await;
        self.settle().await;
    }

    /// 收到的全部包都能解码、没有多余字节，bundle 分隔符成对。
    pub fn assert_stream_is_valid(&self) {
        let frames = self.frames();
        for frame in &frames {
            decode_known(frame);
        }
        assert_bundles_paired(&self.name, &frames);
    }
}

fn decodes<P: Packet>(frame: &Frame) {
    let _: P = frame.decode();
}

/// 认识的包解码一次，解码器会检查格式和多余字节。
fn decode_known(frame: &Frame) {
    match frame.id {
        AddEntity::ID => decodes::<AddEntity>(frame),
        AwardStats::ID => decodes::<AwardStats>(frame),
        BlockChangedAck::ID => decodes::<BlockChangedAck>(frame),
        BlockUpdate::ID => decodes::<BlockUpdate>(frame),
        BundleDelimiter::ID => decodes::<BundleDelimiter>(frame),
        ContainerSetSlot::ID => decodes::<ContainerSetSlot>(frame),
        Disconnect::ID => decodes::<Disconnect>(frame),
        DisguisedChat::ID => decodes::<DisguisedChat>(frame),
        PlayerChat::ID => decodes::<PlayerChat>(frame),
        EntityPositionSync::ID => decodes::<EntityPositionSync>(frame),
        ForgetLevelChunk::ID => decodes::<ForgetLevelChunk>(frame),
        LevelChunk::ID => decodes::<LevelChunk>(frame),
        LevelEvent::ID => decodes::<LevelEvent>(frame),
        MoveEntityPos::ID => decodes::<MoveEntityPos>(frame),
        MoveEntityPosRot::ID => decodes::<MoveEntityPosRot>(frame),
        MoveEntityRot::ID => decodes::<MoveEntityRot>(frame),
        PlayerInfoRemove::ID => decodes::<PlayerInfoRemove>(frame),
        PlayerInfoUpdate::ID => decodes::<PlayerInfoUpdate>(frame),
        PlayerPosition::ID => decodes::<PlayerPosition>(frame),
        RemoveEntities::ID => decodes::<RemoveEntities>(frame),
        RotateHead::ID => decodes::<RotateHead>(frame),
        SectionBlocksUpdate::ID => decodes::<SectionBlocksUpdate>(frame),
        SetChunkCacheCenter::ID => decodes::<SetChunkCacheCenter>(frame),
        SetEntityData::ID => decodes::<SetEntityData>(frame),
        SetEquipment::ID => decodes::<SetEquipment>(frame),
        SetHeldSlot::ID => decodes::<SetHeldSlot>(frame),
        SwingAnimation::ID => decodes::<SwingAnimation>(frame),
        SystemChat::ID => decodes::<SystemChat>(frame),
        _ => {}
    }
}

/// 把一串包写成 `login, level_chunk_with_light x9, ...`。
fn describe(frames: &[Frame]) -> String {
    let mut runs: Vec<(i32, usize)> = Vec::new();
    for frame in frames {
        match runs.last_mut() {
            Some((id, count)) if *id == frame.id => *count += 1,
            _ => runs.push((frame.id, 1)),
        }
    }
    let parts: Vec<String> = runs
        .into_iter()
        .map(|(id, count)| match count {
            1 => packet_name(id),
            _ => format!("{} x{count}", packet_name(id)),
        })
        .collect();
    format!("[{}]", parts.join(", "))
}
