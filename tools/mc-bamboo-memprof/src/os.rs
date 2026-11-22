//! 和操作系统打交道的部分：抓调用栈、进程内存读数、遍历堆和地址空间、符号解析。
//!
//! 只在 Windows 上有真实实现；别的平台编得过，但读数全是 0。
//!
//! Everything that talks to the OS: stack capture, process memory counters,
//! heap and address-space walks, symbol lookup. Only Windows has a real
//! implementation; other platforms compile and report zeros.

/// 进程级的内存读数，单位字节。
///
/// Process-level memory readings in bytes.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessMemory {
    pub working_set: usize,
    pub peak_working_set: usize,
    /// 私有提交量（任务管理器的「提交大小」）。
    ///
    /// Private commit charge.
    pub private_commit: usize,
}

/// 把所有堆走一遍的结果，单位字节。
///
/// Totals from walking every heap, in bytes.
#[derive(Debug, Default, Clone, Copy)]
pub struct HeapTotals {
    pub heaps: usize,
    pub busy_bytes: usize,
    pub busy_blocks: usize,
    /// 堆给每个在用块附加的管理开销。
    ///
    /// Per-block bookkeeping the heap adds to busy blocks.
    pub overhead_bytes: usize,
    /// 已提交但空闲的块：碎片，或者堆留着没还给系统的内存。
    ///
    /// Committed but free blocks: fragmentation, or memory the heap keeps.
    pub free_bytes: usize,
    pub free_blocks: usize,
    pub uncommitted_bytes: usize,
}

/// 地址空间里已提交内存的分类，`*_resident` 是其中在工作集里的部分。
///
/// Committed memory by kind; `*_resident` is the part in the working set.
#[derive(Debug, Default, Clone, Copy)]
pub struct AddressSpace {
    pub image: usize,
    pub image_resident: usize,
    pub mapped: usize,
    pub mapped_resident: usize,
    pub private: usize,
    pub private_resident: usize,
    /// 线程栈（带保护页的私有分配）里已提交的部分。
    ///
    /// Committed part of thread stacks (private allocations with a guard page).
    pub stacks: usize,
    pub stacks_resident: usize,
    pub stack_count: usize,
    /// 分析器自己的分配点表。
    ///
    /// The profiler's own site table.
    pub profiler: usize,
    pub profiler_resident: usize,
}

#[cfg(not(windows))]
mod imp {
    use super::{AddressSpace, HeapTotals, ProcessMemory};

    pub fn reserve_zeroed(bytes: usize) -> *mut u8 {
        let layout = std::alloc::Layout::from_size_align(bytes, 4096).expect("valid table layout");
        unsafe { std::alloc::GlobalAlloc::alloc_zeroed(&std::alloc::System, layout) }
    }

    pub fn capture_stack(_frames: &mut [usize]) -> usize {
        0
    }

    pub fn process_memory() -> ProcessMemory {
        ProcessMemory::default()
    }

    pub fn walk_heaps() -> HeapTotals {
        HeapTotals::default()
    }

    pub fn address_space(_profiler: (usize, usize)) -> AddressSpace {
        AddressSpace::default()
    }

    pub fn symbolize(_address: usize) -> Vec<String> {
        Vec::new()
    }

    pub fn compact_heaps() {}
}

#[cfg(windows)]
mod imp {
    use std::{collections::HashMap, ffi::c_void, ptr};

    use super::{AddressSpace, HeapTotals, ProcessMemory};

    type Handle = *mut c_void;

    #[repr(C)]
    #[derive(Default)]
    struct ProcessMemoryCountersEx {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
        private_usage: usize,
    }

    #[repr(C)]
    struct MemoryBasicInformation {
        base_address: *mut c_void,
        allocation_base: *mut c_void,
        allocation_protect: u32,
        partition_id: u16,
        region_size: usize,
        state: u32,
        protect: u32,
        kind: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct WorkingSetExInformation {
        virtual_address: *mut c_void,
        attributes: usize,
    }

    #[repr(C)]
    struct ProcessHeapEntry {
        data: *mut c_void,
        data_bytes: u32,
        overhead_bytes: u8,
        region_index: u8,
        flags: u16,
        committed_size: u32,
        uncommitted_size: u32,
        first_block: *mut c_void,
        last_block: *mut c_void,
    }

    const MEM_COMMIT: u32 = 0x1000;
    const MEM_RESERVE: u32 = 0x2000;
    const MEM_PRIVATE: u32 = 0x2_0000;
    const MEM_MAPPED: u32 = 0x4_0000;
    const MEM_IMAGE: u32 = 0x100_0000;
    const PAGE_READWRITE: u32 = 0x04;
    const PAGE_GUARD: u32 = 0x100;
    const PROCESS_HEAP_UNCOMMITTED_RANGE: u16 = 0x0002;
    const PROCESS_HEAP_ENTRY_BUSY: u16 = 0x0004;
    const PROCESS_HEAP_REGION: u16 = 0x0001;
    const SYMOPT_UNDNAME: u32 = 0x0002;
    const SYMOPT_DEFERRED_LOADS: u32 = 0x0004;
    const PAGE: usize = 4096;
    /// `SYMBOL_INFO` 定长部分的大小，以及其中几个字段的偏移。
    ///
    /// Size of the fixed part of `SYMBOL_INFO` and the offsets of a few fields.
    const SYMBOL_INFO_SIZE: usize = 88;
    const SYMBOL_NAME_LEN_OFFSET: usize = 76;
    const SYMBOL_MAX_NAME_LEN_OFFSET: usize = 80;
    const SYMBOL_NAME_OFFSET: usize = 84;
    const SYMBOL_MAX_NAME: usize = 2000;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn K32GetProcessMemoryInfo(
            process: Handle,
            counters: *mut ProcessMemoryCountersEx,
            size: u32,
        ) -> i32;
        fn K32QueryWorkingSetEx(
            process: Handle,
            buffer: *mut WorkingSetExInformation,
            size: u32,
        ) -> i32;
        fn VirtualAlloc(address: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
        fn VirtualQuery(
            address: *const c_void,
            buffer: *mut MemoryBasicInformation,
            length: usize,
        ) -> usize;
        fn GetProcessHeaps(count: u32, heaps: *mut Handle) -> u32;
        fn HeapLock(heap: Handle) -> i32;
        fn HeapUnlock(heap: Handle) -> i32;
        fn HeapWalk(heap: Handle, entry: *mut ProcessHeapEntry) -> i32;
        fn HeapCompact(heap: Handle, flags: u32) -> usize;
        fn RtlCaptureStackBackTrace(
            skip: u32,
            capture: u32,
            frames: *mut *mut c_void,
            hash: *mut u32,
        ) -> u16;
    }

    #[link(name = "dbghelp")]
    unsafe extern "system" {
        fn SymSetOptions(options: u32) -> u32;
        fn SymInitialize(process: Handle, search_path: *const u8, invade: i32) -> i32;
        fn SymFromAddr(
            process: Handle,
            address: u64,
            displacement: *mut u64,
            symbol: *mut u64,
        ) -> i32;
        fn SymAddrIncludeInlineTrace(process: Handle, address: u64) -> u32;
        fn SymQueryInlineTrace(
            process: Handle,
            start_address: u64,
            start_context: u32,
            start_return_address: u64,
            current_address: u64,
            current_context: *mut u32,
            current_frame_index: *mut u32,
        ) -> i32;
        fn SymFromInlineContext(
            process: Handle,
            address: u64,
            inline_context: u32,
            displacement: *mut u64,
            symbol: *mut u64,
        ) -> i32;
    }

    pub fn reserve_zeroed(bytes: usize) -> *mut u8 {
        let memory = unsafe {
            VirtualAlloc(
                ptr::null_mut(),
                bytes,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        assert!(!memory.is_null(), "cannot reserve the site table");
        memory.cast()
    }

    /// 抓当前调用栈，跳过记账分配器自己的两层，返回抓到的层数。
    ///
    /// Captures the current stack, skipping the allocator's own two frames.
    pub fn capture_stack(frames: &mut [usize]) -> usize {
        let captured = unsafe {
            RtlCaptureStackBackTrace(
                2,
                frames.len() as u32,
                frames.as_mut_ptr().cast(),
                ptr::null_mut(),
            )
        };
        usize::from(captured)
    }

    pub fn process_memory() -> ProcessMemory {
        let mut counters = ProcessMemoryCountersEx::default();
        let size = size_of::<ProcessMemoryCountersEx>() as u32;
        counters.cb = size;
        // 当前进程的伪句柄无需释放。
        // The current process pseudo-handle does not need closing.
        if unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &raw mut counters, size) } == 0 {
            return ProcessMemory::default();
        }
        ProcessMemory {
            working_set: counters.working_set_size,
            peak_working_set: counters.peak_working_set_size,
            private_commit: counters.private_usage,
        }
    }

    pub fn walk_heaps() -> HeapTotals {
        let mut handles = [ptr::null_mut(); 64];
        let count = unsafe { GetProcessHeaps(handles.len() as u32, handles.as_mut_ptr()) } as usize;
        let mut totals = HeapTotals {
            heaps: count,
            ..HeapTotals::default()
        };
        for heap in &handles[..count.min(handles.len())] {
            walk_heap(*heap, &mut totals);
        }
        totals
    }

    /// 让每个堆合并相邻的空闲块、把整页空闲的内存退还给系统。
    ///
    /// Asks every heap to coalesce free blocks and decommit fully free pages.
    pub fn compact_heaps() {
        let mut handles = [ptr::null_mut(); 64];
        let count = unsafe { GetProcessHeaps(handles.len() as u32, handles.as_mut_ptr()) } as usize;
        for heap in &handles[..count.min(handles.len())] {
            unsafe { HeapCompact(*heap, 0) };
        }
    }

    fn walk_heap(heap: Handle, totals: &mut HeapTotals) {
        if unsafe { HeapLock(heap) } == 0 {
            return;
        }
        let mut entry: ProcessHeapEntry = unsafe { std::mem::zeroed() };
        while unsafe { HeapWalk(heap, &raw mut entry) } != 0 {
            let bytes = entry.data_bytes as usize;
            if entry.flags & PROCESS_HEAP_ENTRY_BUSY != 0 {
                totals.busy_bytes += bytes;
                totals.busy_blocks += 1;
                totals.overhead_bytes += usize::from(entry.overhead_bytes);
            } else if entry.flags & PROCESS_HEAP_UNCOMMITTED_RANGE != 0 {
                totals.uncommitted_bytes += bytes;
            } else if entry.flags & PROCESS_HEAP_REGION == 0 {
                totals.free_bytes += bytes;
                totals.free_blocks += 1;
            }
        }
        unsafe { HeapUnlock(heap) };
    }

    /// 一段已提交内存里有多少字节在工作集里。
    ///
    /// Bytes of a committed range that are in the working set.
    fn resident_bytes(start: usize, bytes: usize) -> usize {
        const BATCH: usize = 1024;
        let mut buffer = [WorkingSetExInformation {
            virtual_address: ptr::null_mut(),
            attributes: 0,
        }; BATCH];
        let pages = bytes / PAGE;
        let mut resident = 0;
        let mut done = 0;
        while done < pages {
            let batch = (pages - done).min(BATCH);
            for (index, item) in buffer[..batch].iter_mut().enumerate() {
                item.virtual_address = (start + (done + index) * PAGE) as *mut c_void;
                item.attributes = 0;
            }
            let size = (batch * size_of::<WorkingSetExInformation>()) as u32;
            if unsafe { K32QueryWorkingSetEx(GetCurrentProcess(), buffer.as_mut_ptr(), size) } != 0
            {
                resident += buffer[..batch]
                    .iter()
                    .filter(|item| item.attributes & 1 != 0)
                    .count()
                    * PAGE;
            }
            done += batch;
        }
        resident
    }

    struct Region {
        allocation: usize,
        start: usize,
        bytes: usize,
        kind: u32,
        guard: bool,
        committed: bool,
    }

    fn regions() -> Vec<Region> {
        let mut regions = Vec::new();
        let mut address = 0usize;
        loop {
            let mut info: MemoryBasicInformation = unsafe { std::mem::zeroed() };
            let size = size_of::<MemoryBasicInformation>();
            if unsafe { VirtualQuery(address as *const c_void, &raw mut info, size) } == 0 {
                break;
            }
            regions.push(Region {
                allocation: info.allocation_base as usize,
                start: info.base_address as usize,
                bytes: info.region_size,
                kind: info.kind,
                guard: info.protect & PAGE_GUARD != 0,
                committed: info.state == MEM_COMMIT,
            });
            let Some(next) = (info.base_address as usize).checked_add(info.region_size) else {
                break;
            };
            address = next;
        }
        regions
    }

    pub fn address_space(profiler: (usize, usize)) -> AddressSpace {
        let regions = regions();
        let mut stack_allocations: HashMap<usize, ()> = HashMap::new();
        for region in regions.iter().filter(|region| region.guard) {
            stack_allocations.insert(region.allocation, ());
        }
        let mut space = AddressSpace {
            stack_count: stack_allocations.len(),
            ..AddressSpace::default()
        };
        for region in regions.iter().filter(|region| region.committed) {
            let resident = resident_bytes(region.start, region.bytes);
            let (total, in_set) = if region.kind == MEM_IMAGE {
                (&mut space.image, &mut space.image_resident)
            } else if region.kind == MEM_MAPPED {
                (&mut space.mapped, &mut space.mapped_resident)
            } else if region.kind != MEM_PRIVATE {
                continue;
            } else if region.start >= profiler.0 && region.start < profiler.1 {
                (&mut space.profiler, &mut space.profiler_resident)
            } else if stack_allocations.contains_key(&region.allocation) {
                (&mut space.stacks, &mut space.stacks_resident)
            } else {
                (&mut space.private, &mut space.private_resident)
            };
            *total += region.bytes;
            *in_set += resident;
        }
        space
    }

    fn symbol_name(buffer: &[u64]) -> String {
        let bytes: &[u8] =
            unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast(), buffer.len() * 8) };
        let length = u32::from_ne_bytes(
            bytes[SYMBOL_NAME_LEN_OFFSET..SYMBOL_NAME_LEN_OFFSET + 4]
                .try_into()
                .expect("four bytes"),
        ) as usize;
        let name = &bytes[SYMBOL_NAME_OFFSET..SYMBOL_NAME_OFFSET + length.min(SYMBOL_MAX_NAME)];
        String::from_utf8_lossy(name).into_owned()
    }

    fn symbol_buffer() -> Vec<u64> {
        let mut buffer = vec![0u64; (SYMBOL_INFO_SIZE + SYMBOL_MAX_NAME) / 8 + 2];
        let bytes: &mut [u8] =
            unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast(), buffer.len() * 8) };
        bytes[..4].copy_from_slice(&(SYMBOL_INFO_SIZE as u32).to_ne_bytes());
        bytes[SYMBOL_MAX_NAME_LEN_OFFSET..SYMBOL_MAX_NAME_LEN_OFFSET + 4]
            .copy_from_slice(&(SYMBOL_MAX_NAME as u32).to_ne_bytes());
        buffer
    }

    fn init_symbols(process: Handle) {
        static INIT: std::sync::Once = std::sync::Once::new();
        // PDB 和可执行文件放在一起，但名字里的连字符变成了下划线，默认的查找路径
        // （当前目录）找不到，所以把可执行文件所在目录交给 dbghelp。
        // The PDB sits beside the executable under a slightly different name;
        // the default search path (the current directory) misses it, so hand
        // dbghelp the executable's directory.
        let mut search_path = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.to_string_lossy().into_owned()))
            .unwrap_or_default()
            .into_bytes();
        search_path.push(0);
        INIT.call_once(|| unsafe {
            SymSetOptions(SYMOPT_UNDNAME | SYMOPT_DEFERRED_LOADS);
            SymInitialize(process, search_path.as_ptr(), 1);
        });
    }

    /// 一个返回地址对应的函数名，内联进来的函数排在前面（从内到外）。
    ///
    /// Function names for one return address; inlined callees come first.
    pub fn symbolize(address: usize) -> Vec<String> {
        let process = unsafe { GetCurrentProcess() };
        init_symbols(process);
        // 返回地址指向调用指令的下一条，退一个字节才落在调用所在的那一行里。
        // A return address points past the call; step back one byte to stay
        // inside the calling line.
        let address = address.saturating_sub(1) as u64;
        let mut names = Vec::new();
        let mut buffer = symbol_buffer();
        let inlined = unsafe { SymAddrIncludeInlineTrace(process, address) };
        let (mut context, mut frame_index) = (0u32, 0u32);
        if inlined > 0
            && unsafe {
                SymQueryInlineTrace(
                    process,
                    address,
                    0,
                    address,
                    address,
                    &raw mut context,
                    &raw mut frame_index,
                )
            } != 0
        {
            for depth in 0..inlined {
                let mut displacement = 0u64;
                let found = unsafe {
                    SymFromInlineContext(
                        process,
                        address,
                        context + depth,
                        &raw mut displacement,
                        buffer.as_mut_ptr(),
                    )
                };
                if found != 0 {
                    names.push(symbol_name(&buffer));
                }
            }
        }
        let mut displacement = 0u64;
        if unsafe { SymFromAddr(process, address, &raw mut displacement, buffer.as_mut_ptr()) } != 0
        {
            names.push(symbol_name(&buffer));
        }
        names
    }
}

pub use imp::{
    address_space, capture_stack, compact_heaps, process_memory, reserve_zeroed, symbolize,
    walk_heaps,
};
