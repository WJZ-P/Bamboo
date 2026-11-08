//! 递归解码快到栈底时，把这一层挪到一条新线程上。
//!
//! 连接任务跑在 tokio 的 worker 上，默认栈大约 2 MB。文本组件、谓词和嵌套物品都按官方的
//! 结构递归。Rust 的栈溢出会直接结束进程，一个客户端就能把服务器打崩。`main` 不能改
//! 运行时的栈大小。
//!
//! 这里不用 `stacker`：它在 Windows 上要另外编译一段 C，而这台构建机器没有 C 编译器。
//! 剩下的栈不到 1 MB 时，开一条 8 MB 栈的线程把当前这一层跑完再回来。新线程的栈由系统
//! 划在地址空间里，不占原来那条栈。平常的浅层数据包不会走到这一步。
//!
//! When recursive decoding gets near the bottom of the stack, run that frame on a
//! new thread.
//!
//! The connection task runs on a tokio worker with about 2 MB of stack. Text,
//! predicates and nested items recurse the way the official codecs do. A Rust stack
//! overflow aborts the process, so one client could take the server down. `main`
//! cannot change the runtime stack size.
//!
//! `stacker` is not used: on Windows it compiles a C file, and this build machine
//! has no C compiler. When less than 1 MB remains, a thread with an 8 MB stack runs
//! the current frame and returns. That stack is reserved by the OS and does not use
//! the caller's stack. Shallow packets never take this path.

use std::cell::Cell;

/// 低于这个剩余量就换一条栈。比 tokio 的默认栈小，1 MB 的测试线程会立刻换走。
const RED_ZONE: usize = 1024 * 1024;
/// 新线程的栈。512 层文本在调试构建里一帧很大，8 MB 装得下一批，装不下会再换一条。
const HOP_STACK: usize = 8 * 1024 * 1024;
/// 连续换栈的上限，防止剩余栈量读错时无限开线程。
const MAX_HOPS: u32 = 64;

thread_local! {
    static HOPS: Cell<u32> = const { Cell::new(0) };
    /// 这条线程的栈底（低地址端）。`None` 是还没查过，`Some(None)` 是这个平台查不到。
    ///
    /// The low end of this thread's stack. `None` means not looked up yet, and
    /// `Some(None)` means this platform cannot tell.
    static LOW: Cell<Option<Option<usize>>> = const { Cell::new(None) };
    /// 换栈线程开始时的栈位置，0 表示这条线程不是换出来的。查不到栈底的平台靠它量剩余栈。
    ///
    /// Where a hop thread's stack started, or 0 when this thread was not started by a
    /// hop. Platforms that cannot report the stack bounds measure from here.
    static HOP_TOP: Cell<usize> = const { Cell::new(0) };
}

/// 保证 `body` 开始时至少还有 [`RED_ZONE`] 字节的栈，或者已经换到新线程上。
pub(crate) fn guard<T: Send>(body: impl FnOnce() -> T + Send) -> T {
    if HOPS.get() >= MAX_HOPS || remaining_stack() >= RED_ZONE {
        return body();
    }
    relay(body)
}

fn relay<T: Send, F: FnOnce() -> T + Send>(body: F) -> T {
    let hops = HOPS.get();
    // `scope` 允许闭包借用父线程的解码器。父线程停在这里，等子线程跑完这一层。
    //
    // `scope` lets the closure borrow the parent's decoder. The parent waits here
    // until the child finishes this frame.
    std::thread::scope(|scope| {
        let handle = std::thread::Builder::new()
            .name("bamboo-decode-stack".to_owned())
            .stack_size(HOP_STACK)
            .spawn_scoped(scope, move || {
                HOPS.set(hops.saturating_add(1));
                HOP_TOP.set(stack_position());
                body()
            })
            .expect("decode stack thread");
        match handle.join() {
            Ok(value) => value,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    })
}

/// 当前位置到栈底还剩多少字节，减去一段余量。
///
/// 查得到栈底的平台（Windows、Linux、Android、苹果系）按系统给的栈底算。查不到的平台在
/// 普通线程上返回 0，第一次 `guard` 就换到大小已知的线程上，之后从换栈时的位置量。
///
/// How many bytes remain between here and the low end of the stack, minus a margin.
///
/// Where the OS reports the stack bounds (Windows, Linux, Android, Apple), this uses
/// them. Elsewhere it returns 0 on an ordinary thread, so the first `guard` hops to a
/// thread of known size, and later calls measure from where that hop started.
fn remaining_stack() -> usize {
    const MARGIN: usize = 64 * 1024;
    let here = stack_position();
    let low = LOW.get().unwrap_or_else(|| {
        let low = stack_low();
        LOW.set(Some(low));
        low
    });
    if let Some(low) = low {
        return here.saturating_sub(low).saturating_sub(MARGIN);
    }
    match HOP_TOP.get() {
        0 => 0,
        top => HOP_STACK
            .saturating_sub(top.saturating_sub(here))
            .saturating_sub(MARGIN),
    }
}

/// 当前栈帧里一个局部变量的地址，约等于栈指针。支持的平台上栈都往低地址长。
///
/// The address of a local in the current frame, close to the stack pointer. On
/// every supported platform the stack grows toward lower addresses.
#[inline(always)]
fn stack_position() -> usize {
    let marker = 0u8;
    std::ptr::from_ref(std::hint::black_box(&marker)) as usize
}

#[cfg(windows)]
fn stack_low() -> Option<usize> {
    // `GetCurrentThreadStackLimits` 给出保留栈的两端。已提交的 `StackLimit` 只比
    // 栈指针低一两页，不能拿来判断还剩多少。
    //
    // `GetCurrentThreadStackLimits` reports the reserved stack. The committed
    // `StackLimit` sits only a page or two under the stack pointer, so it is
    // not the space still available.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThreadStackLimits(low: *mut usize, high: *mut usize);
    }
    let mut low = 0usize;
    let mut high = 0usize;
    // SAFETY: both pointers refer to live locals that the call writes.
    unsafe { GetCurrentThreadStackLimits(&raw mut low, &raw mut high) };
    (low != 0).then_some(low)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn stack_low() -> Option<usize> {
    // `pthread_getattr_np` 给出这条线程的栈，起始地址就是低地址端。主线程上 glibc 会读
    // `/proc/self/maps`，所以结果按线程缓存在 `LOW` 里。
    //
    // `pthread_getattr_np` reports this thread's stack, whose address is the low end.
    // On the main thread glibc reads `/proc/self/maps`, so `LOW` caches the result per
    // thread.
    let mut attr = std::mem::MaybeUninit::<libc::pthread_attr_t>::zeroed();
    // SAFETY: `pthread_getattr_np` initialises `attr` before it is read, and `attr`
    // is destroyed exactly once after that call succeeds.
    unsafe {
        if libc::pthread_getattr_np(libc::pthread_self(), attr.as_mut_ptr()) != 0 {
            return None;
        }
        let mut address: *mut libc::c_void = std::ptr::null_mut();
        let mut size: libc::size_t = 0;
        let found =
            libc::pthread_attr_getstack(attr.as_ptr(), &raw mut address, &raw mut size) == 0;
        libc::pthread_attr_destroy(attr.as_mut_ptr());
        (found && !address.is_null()).then_some(address as usize)
    }
}

#[cfg(target_vendor = "apple")]
fn stack_low() -> Option<usize> {
    // 苹果系给的是栈顶（高地址端）和栈的大小。
    //
    // Apple platforms report the high end of the stack and its size.
    // SAFETY: both calls only read the current thread's own attributes.
    unsafe {
        let thread = libc::pthread_self();
        let high = libc::pthread_get_stackaddr_np(thread) as usize;
        high.checked_sub(libc::pthread_get_stacksize_np(thread))
    }
}

#[cfg(not(any(
    windows,
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple"
)))]
fn stack_low() -> Option<usize> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on_thread<T: Send + 'static>(stack: usize, body: impl FnOnce() -> T + Send + 'static) -> T {
        std::thread::Builder::new()
            .stack_size(stack)
            .spawn(body)
            .expect("test thread")
            .join()
            .expect("test thread result")
    }

    /// 查得到栈底的平台：4 MB 的线程上剩余栈超过红线，也不会比整条栈还大。
    ///
    /// Where the bounds are known, a 4 MB thread has more than the red zone left and
    /// never more than the whole stack.
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "android",
        target_vendor = "apple"
    ))]
    #[test]
    fn the_remaining_stack_is_measured() {
        let remaining = on_thread(4 * 1024 * 1024, remaining_stack);
        assert!(remaining > RED_ZONE, "{remaining}");
        assert!(remaining < 5 * 1024 * 1024, "{remaining}");
    }

    /// 查得到栈底的平台：浅层调用不换线程。
    ///
    /// Where the bounds are known, a shallow call stays on its thread.
    #[cfg(any(
        windows,
        target_os = "linux",
        target_os = "android",
        target_vendor = "apple"
    ))]
    #[test]
    fn a_shallow_guard_stays_on_the_thread() {
        let stayed = on_thread(4 * 1024 * 1024, || {
            let outer = std::thread::current().id();
            guard(|| std::thread::current().id() == outer)
        });
        assert!(stayed);
    }

    /// 剩余栈不够时换到新线程，换出来的线程记下自己的起点，从那里量还剩多少。
    ///
    /// When the stack runs low the frame moves to a new thread, which records where
    /// its stack started and measures from there.
    #[test]
    fn a_low_stack_hops_to_a_new_thread() {
        let (moved, top, remaining) = on_thread(512 * 1024, || {
            let outer = std::thread::current().id();
            guard(|| {
                (
                    std::thread::current().id() != outer,
                    HOP_TOP.get(),
                    remaining_stack(),
                )
            })
        });
        assert!(moved);
        assert_ne!(top, 0);
        assert!(remaining > RED_ZONE, "{remaining}");
    }
}
