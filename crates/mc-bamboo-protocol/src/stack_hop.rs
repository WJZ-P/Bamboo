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
                body()
            })
            .expect("decode stack thread");
        match handle.join() {
            Ok(value) => value,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    })
}

fn remaining_stack() -> usize {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
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
        unsafe { GetCurrentThreadStackLimits(&raw mut low, &raw mut high) };
        let mut pointer = 0usize;
        unsafe {
            std::arch::asm!(
                "mov {}, rsp",
                out(reg) pointer,
                options(nostack, preserves_flags)
            );
        }
        pointer.saturating_sub(low).saturating_sub(64 * 1024)
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        0
    }
}
