//! 记账分配器：每次分配抓一段调用栈，按调用栈（「分配点」）累计还活着的字节数。
//!
//! 每块内存前面多放一个 8 字节以上的头，记它属于哪个分配点，释放时从那个分配点
//! 扣掉。分配点表是一张固定大小的开放寻址表，直接向系统要内存，不经过被记账的堆。
//!
//! The accounting allocator: every allocation captures a call stack, and live
//! bytes are summed per call stack (an "allocation site"). Each block carries
//! a header of at least 8 bytes naming its site, so a free can subtract from
//! the right one. The site table is a fixed open-addressing table taken
//! straight from the OS, outside the heap being measured.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    io::{self, Write},
    ptr,
    sync::atomic::{AtomicBool, AtomicI64, AtomicPtr, AtomicU64, AtomicUsize, Ordering},
};

use crate::os;

/// 每个分配点最多记多少层调用栈（从分配函数往外数）。
///
/// Frames kept per site, counted outward from the allocating function.
pub const MAX_FRAMES: usize = 32;
/// 抓栈时最多往外走多少层，以及太深时保留最里面的多少层。
///
/// How far out a capture walks, and how many innermost frames survive when
/// the stack is deeper than [`MAX_FRAMES`].
const DEEP_FRAMES: usize = 160;
const INNER_FRAMES: usize = 10;
/// 分配点表的槽数，2 的幂。
///
/// Slots in the site table; a power of two.
const SLOTS: usize = 1 << 18;
/// 表满到这个程度以后，新的调用栈都记到 0 号「溢出」分配点上。
///
/// Past this fill level every new call stack lands in overflow site 0.
const MAX_USED: usize = SLOTS / 4 * 3;

#[repr(C)]
struct Slot {
    /// 调用栈的哈希，0 表示空槽。最后写入，读到非 0 就说明 `frames` 已经写好。
    ///
    /// Hash of the stack; 0 is empty. Written last, so a non-zero key means
    /// `frames` is complete.
    key: AtomicU64,
    live_bytes: AtomicI64,
    live_count: AtomicI64,
    total_bytes: AtomicU64,
    total_count: AtomicU64,
    depth: AtomicUsize,
    frames: [AtomicUsize; MAX_FRAMES],
}

static TABLE: AtomicPtr<Slot> = AtomicPtr::new(ptr::null_mut());
static INSERT_LOCK: AtomicBool = AtomicBool::new(false);
static USED: AtomicUsize = AtomicUsize::new(0);
static LIVE_BYTES: AtomicI64 = AtomicI64::new(0);
static LIVE_COUNT: AtomicI64 = AtomicI64::new(0);
static PEAK_BYTES: AtomicI64 = AtomicI64::new(0);

pub struct Tracker;

fn table() -> *mut Slot {
    let table = TABLE.load(Ordering::Acquire);
    if !table.is_null() {
        return table;
    }
    let fresh = os::reserve_zeroed(SLOTS * size_of::<Slot>()).cast::<Slot>();
    match TABLE.compare_exchange(ptr::null_mut(), fresh, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => fresh,
        // 输掉竞争的那一份留着不管：只会发生一次，而且从没被碰过，不占物理内存。
        // The losing copy is leaked: it happens once and its pages are never touched.
        Err(winner) => winner,
    }
}

/// 分配点表占的地址范围，统计工作集时要把它剔出去。
///
/// Address range of the site table, excluded from working-set accounting.
pub fn table_range() -> (usize, usize) {
    let start = table() as usize;
    (start, start + SLOTS * size_of::<Slot>())
}

fn hash_frames(frames: &[usize]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for frame in frames {
        hash = (hash ^ *frame as u64).wrapping_mul(0x0000_0100_0000_01b3);
        hash ^= hash >> 29;
    }
    hash | 1
}

fn same_stack(slot: &Slot, frames: &[usize]) -> bool {
    slot.depth.load(Ordering::Relaxed) == frames.len()
        && frames
            .iter()
            .zip(&slot.frames)
            .all(|(frame, stored)| stored.load(Ordering::Relaxed) == *frame)
}

enum Insert {
    Done,
    Full,
    Raced,
}

/// 找到（或新建）这段调用栈的槽号。
///
/// Finds or creates the slot of a call stack.
fn site_of(frames: &[usize]) -> usize {
    let key = hash_frames(frames);
    let mut index = (key >> 20) as usize & (SLOTS - 1);
    loop {
        if index == 0 {
            index = 1;
        }
        let candidate = slot(index);
        let stored = candidate.key.load(Ordering::Acquire);
        if stored == key && same_stack(candidate, frames) {
            return index;
        }
        if stored == 0 {
            match insert(candidate, key, frames) {
                Insert::Done => return index,
                Insert::Full => return 0,
                Insert::Raced => continue,
            }
        }
        index = (index + 1) & (SLOTS - 1);
    }
}

fn insert(slot: &Slot, key: u64, frames: &[usize]) -> Insert {
    while INSERT_LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        std::hint::spin_loop();
    }
    let outcome = if slot.key.load(Ordering::Acquire) != 0 {
        Insert::Raced
    } else if USED.load(Ordering::Relaxed) >= MAX_USED {
        Insert::Full
    } else {
        for (stored, frame) in slot.frames.iter().zip(frames) {
            stored.store(*frame, Ordering::Relaxed);
        }
        slot.depth.store(frames.len(), Ordering::Relaxed);
        slot.key.store(key, Ordering::Release);
        USED.fetch_add(1, Ordering::Relaxed);
        Insert::Done
    };
    INSERT_LOCK.store(false, Ordering::Release);
    outcome
}

fn slot(index: usize) -> &'static Slot {
    // 槽号先按表长取模，表的内存从不释放。
    // The index is masked to the table, whose memory is never released.
    unsafe { &*table().add(index & (SLOTS - 1)) }
}

/// 抓调用栈。太深的栈（递归解析、递归克隆）留最里面 [`INNER_FRAMES`] 层和最外面的
/// 其余层，中间的丢掉：归属看的是「谁在分配」和「哪条线程上的哪个入口」。
///
/// Captures the stack. Very deep stacks (recursive parsing or cloning) keep
/// the innermost [`INNER_FRAMES`] frames and the outermost rest, dropping the
/// middle: ownership needs "who allocates" and "which entry point".
fn capture(frames: &mut [usize; MAX_FRAMES]) -> usize {
    let mut deep = [0usize; DEEP_FRAMES];
    let depth = os::capture_stack(&mut deep);
    if depth <= MAX_FRAMES {
        frames[..depth].copy_from_slice(&deep[..depth]);
        return depth;
    }
    frames[..INNER_FRAMES].copy_from_slice(&deep[..INNER_FRAMES]);
    let outer = MAX_FRAMES - INNER_FRAMES;
    frames[INNER_FRAMES..].copy_from_slice(&deep[depth - outer..depth]);
    MAX_FRAMES
}

fn charge(size: usize) -> usize {
    let mut frames = [0usize; MAX_FRAMES];
    let depth = capture(&mut frames);
    let index = site_of(&frames[..depth]);
    let site = slot(index);
    site.live_bytes.fetch_add(size as i64, Ordering::Relaxed);
    site.live_count.fetch_add(1, Ordering::Relaxed);
    site.total_bytes.fetch_add(size as u64, Ordering::Relaxed);
    site.total_count.fetch_add(1, Ordering::Relaxed);
    let live = LIVE_BYTES.fetch_add(size as i64, Ordering::Relaxed) + size as i64;
    LIVE_COUNT.fetch_add(1, Ordering::Relaxed);
    PEAK_BYTES.fetch_max(live, Ordering::Relaxed);
    index
}

fn refund(index: usize, size: usize) {
    let site = slot(index);
    site.live_bytes.fetch_sub(size as i64, Ordering::Relaxed);
    site.live_count.fetch_sub(1, Ordering::Relaxed);
    LIVE_BYTES.fetch_sub(size as i64, Ordering::Relaxed);
    LIVE_COUNT.fetch_sub(1, Ordering::Relaxed);
}

/// 头的大小：至少 8 字节，并且不破坏调用方要的对齐。
///
/// Header size: at least 8 bytes and a multiple of the requested alignment.
fn header(layout: Layout) -> usize {
    layout.align().max(8)
}

fn outer(layout: Layout, size: usize) -> Layout {
    let header = header(layout);
    // 调用方的 Layout 已经合法，再加一个头不会变成非法。
    // The caller's layout is valid; one more header keeps it valid.
    unsafe { Layout::from_size_align_unchecked(size + header, header) }
}

/// 在系统给的块上写头，返回交给调用方的指针。
///
/// Writes the header onto a block from the system and returns the user pointer.
unsafe fn finish(base: *mut u8, layout: Layout, size: usize) -> *mut u8 {
    if base.is_null() {
        return base;
    }
    let index = charge(size);
    unsafe {
        let user = base.add(header(layout));
        user.sub(8).cast::<u64>().write(index as u64);
        user
    }
}

unsafe impl GlobalAlloc for Tracker {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        unsafe { finish(System.alloc(outer(layout, size)), layout, size) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        unsafe { finish(System.alloc_zeroed(outer(layout, size)), layout, size) }
    }

    unsafe fn dealloc(&self, user: *mut u8, layout: Layout) {
        unsafe {
            refund(user.sub(8).cast::<u64>().read() as usize, layout.size());
            System.dealloc(user.sub(header(layout)), outer(layout, layout.size()));
        }
    }

    unsafe fn realloc(&self, user: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe {
            let old_index = user.sub(8).cast::<u64>().read() as usize;
            let base = System.realloc(
                user.sub(header(layout)),
                outer(layout, layout.size()),
                new_size + header(layout),
            );
            if base.is_null() {
                return base;
            }
            refund(old_index, layout.size());
            finish(base, layout, new_size)
        }
    }
}

/// 全局的活跃字节数、活跃块数和活跃字节数的历史峰值。
///
/// Global live bytes, live block count and the peak of live bytes.
pub fn totals() -> (i64, i64, i64) {
    (
        LIVE_BYTES.load(Ordering::Relaxed),
        LIVE_COUNT.load(Ordering::Relaxed),
        PEAK_BYTES.load(Ordering::Relaxed),
    )
}

pub fn sites_used() -> usize {
    USED.load(Ordering::Relaxed)
}

/// 把还有活跃内存的分配点逐行写出去：
/// `活跃字节 活跃块数 累计字节 累计块数 栈帧地址...`（十六进制地址，从内到外）。
///
/// Writes every site that still holds memory, one per line:
/// `live_bytes live_count total_bytes total_count frames...` (hex addresses,
/// innermost first).
pub fn dump(out: &mut impl Write) -> io::Result<()> {
    for index in 0..SLOTS {
        let site = slot(index);
        let live = site.live_bytes.load(Ordering::Relaxed);
        if live <= 0 || (index != 0 && site.key.load(Ordering::Acquire) == 0) {
            continue;
        }
        write!(
            out,
            "{live} {} {} {}",
            site.live_count.load(Ordering::Relaxed),
            site.total_bytes.load(Ordering::Relaxed),
            site.total_count.load(Ordering::Relaxed)
        )?;
        for frame in &site.frames[..site.depth.load(Ordering::Relaxed)] {
            write!(out, " {:x}", frame.load(Ordering::Relaxed))?;
        }
        writeln!(out)?;
    }
    Ok(())
}

/// 表里出现过的所有栈帧地址（去重由调用方做）。
///
/// Every frame address in the table; the caller deduplicates.
pub fn for_each_frame(mut visit: impl FnMut(usize)) {
    for index in 1..SLOTS {
        let site = slot(index);
        if site.key.load(Ordering::Acquire) == 0 {
            continue;
        }
        for frame in &site.frames[..site.depth.load(Ordering::Relaxed)] {
            visit(frame.load(Ordering::Relaxed));
        }
    }
}
