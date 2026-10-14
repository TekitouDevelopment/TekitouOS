
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::hint::spin_loop;
use core::mem::{align_of, size_of};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

const UNINITIALIZED: u8 = 0;
const INITIALIZING: u8 = 1;
const READY: u8 = 2;

const HEADER_MAGIC: u64 = 0x4845_4150_4652_4545;

#[repr(C)]
struct FreeBlock {
    size: usize,
    next: *mut FreeBlock,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AllocationHeader {
    block_start: *mut u8,
    block_size: usize,
    magic: u64,
}

pub struct FreeListAllocator {
    locked: AtomicBool,
    state: AtomicU8,
    free_head: UnsafeCell<*mut FreeBlock>,
}

// free_headへのアクセスはlockedで保護する。
unsafe impl Sync for FreeListAllocator {}

struct LockGuard<'a>(&'a AtomicBool);

impl Drop for LockGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl FreeListAllocator {
    pub const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
            state: AtomicU8::new(UNINITIALIZED),
            free_head: UnsafeCell::new(ptr::null_mut()),
        }
    }

    fn acquire_lock(&self) -> LockGuard<'_> {
        while self
            .locked
            .compare_exchange_weak(
                false,
                true,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_err()
        {
            spin_loop();
        }

        LockGuard(&self.locked)
    }

    pub fn init(&self, start: usize, size: usize) -> bool {
        if start == 0
            || size < size_of::<FreeBlock>()
            || start % align_of::<FreeBlock>() != 0
            || start.checked_add(size).is_none()
        {
            return false;
        }

        if self
            .state
            .compare_exchange(
                UNINITIALIZED,
                INITIALIZING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return false;
        }

        unsafe {
            let first = start as *mut FreeBlock;

            ptr::write(
                first,
                FreeBlock {
                    size,
                    next: ptr::null_mut(),
                },
            );

            *self.free_head.get() = first;
        }

        self.state.store(READY, Ordering::Release);
        true
    }
}

unsafe impl GlobalAlloc for FreeListAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() == 0 {
            return layout.align() as *mut u8;
        }

        if self.state.load(Ordering::Acquire) != READY {
            return ptr::null_mut();
        }

        let _guard = self.acquire_lock();
        let head = self.free_head.get();

        let mut previous: *mut FreeBlock = ptr::null_mut();
        let mut current = unsafe { *head };

        let alignment = layout.align().max(align_of::<AllocationHeader>());

        while !current.is_null() {
            let block_start = current as usize;

            let (block_size, next) = unsafe {
                ((*current).size, (*current).next)
            };

            let block_end = match block_start.checked_add(block_size) {
                Some(value) => value,
                None => return ptr::null_mut(),
            };

            let minimum_user = match block_start
                .checked_add(size_of::<AllocationHeader>())
            {
                Some(value) => value,
                None => return ptr::null_mut(),
            };

            let user_address = match align_up(minimum_user, alignment) {
                Some(value) => value,
                None => return ptr::null_mut(),
            };

            let user_end = match user_address.checked_add(layout.size()) {
                Some(value) => value,
                None => return ptr::null_mut(),
            };

            if user_end <= block_end {
                let split_candidate =
                    align_up(user_end, align_of::<FreeBlock>());

                let split_address = match split_candidate {
                    Some(value)
                    if value <= block_end
                        && block_end - value >= size_of::<FreeBlock>() =>
                        {
                            Some(value)
                        }
                    _ => None,
                };

                let allocated_size;

                if let Some(split) = split_address {
                    // 空き領域の先頭部分を使い、
                    // 残りを新しい空きブロックにする。
                    let remainder = split as *mut FreeBlock;

                    unsafe {
                        ptr::write(
                            remainder,
                            FreeBlock {
                                size: block_end - split,
                                next,
                            },
                        );

                        if previous.is_null() {
                            *head = remainder;
                        } else {
                            (*previous).next = remainder;
                        }
                    }

                    allocated_size = split - block_start;
                } else {
                    // 端数が小さい場合はブロック全体を使う。
                    unsafe {
                        if previous.is_null() {
                            *head = next;
                        } else {
                            (*previous).next = next;
                        }
                    }

                    allocated_size = block_size;
                }

                let header_address =
                    user_address - size_of::<AllocationHeader>();

                let header = header_address as *mut AllocationHeader;

                unsafe {
                    ptr::write(
                        header,
                        AllocationHeader {
                            block_start: block_start as *mut u8,
                            block_size: allocated_size,
                            magic: HEADER_MAGIC,
                        },
                    );
                }

                return user_address as *mut u8;
            }

            previous = current;
            current = next;
        }

        ptr::null_mut()
    }

    unsafe fn dealloc(&self, user_ptr: *mut u8, _layout: Layout) {
        if user_ptr.is_null()
            || self.state.load(Ordering::Acquire) != READY
        {
            return;
        }

        let _guard = self.acquire_lock();

        let header_ptr = unsafe {
            user_ptr.sub(size_of::<AllocationHeader>())
                as *mut AllocationHeader
        };

        let header = unsafe { ptr::read(header_ptr) };

        // 不正なヘッダーや二重解放を検出するための簡易チェック。
        if header.magic != HEADER_MAGIC
            || header.block_start.is_null()
            || header.block_size < size_of::<FreeBlock>()
        {
            return;
        }

        let start = header.block_start as usize;

        if start.checked_add(header.block_size).is_none() {
            return;
        }

        // 二重解放の検出用にマーカーを消す。
        let mut invalidated = header;
        invalidated.magic = 0;

        unsafe {
            ptr::write(header_ptr, invalidated);
        }

        let node = start as *mut FreeBlock;

        unsafe {
            ptr::write(
                node,
                FreeBlock {
                    size: header.block_size,
                    next: ptr::null_mut(),
                },
            );
        }

        let head = self.free_head.get();
        let mut previous: *mut FreeBlock = ptr::null_mut();
        let mut current = unsafe { *head };

        // アドレス順に挿入する。
        unsafe {
            while !current.is_null() && (current as usize) < start {
                previous = current;
                current = (*current).next;
            }

            (*node).next = current;

            if previous.is_null() {
                *head = node;
            } else {
                (*previous).next = node;
            }

            // 後ろの空きブロックと連結する。
            if !current.is_null() {
                let node_end =
                    (node as usize).checked_add((*node).size);

                if node_end == Some(current as usize) {
                    if let Some(merged_size) =
                        (*node).size.checked_add((*current).size)
                    {
                        (*node).size = merged_size;
                        (*node).next = (*current).next;
                    }
                }
            }

            // 前の空きブロックとも連結する。
            if !previous.is_null() {
                let previous_end =
                    (previous as usize).checked_add((*previous).size);

                if previous_end == Some(start) {
                    if let Some(merged_size) =
                        (*previous).size.checked_add((*node).size)
                    {
                        (*previous).size = merged_size;
                        (*previous).next = (*node).next;
                    }
                }
            }
        }
    }
}

fn align_up(value: usize, alignment: usize) -> Option<usize> {
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
}

#[global_allocator]
static GLOBAL_ALLOCATOR: FreeListAllocator = FreeListAllocator::new();

pub fn init(start: usize, size: usize) -> bool {
    GLOBAL_ALLOCATOR.init(start, size)
}
