
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};

const PAGE_SIZE: usize = 4096;

pub struct BumpAllocator {
    next: AtomicUsize,
    end: AtomicUsize,
}

impl BumpAllocator {
    pub const fn new() -> Self {
        Self {
            next: AtomicUsize::new(0),
            end: AtomicUsize::new(0),
        }
    }

    pub fn init(&self, start: usize, size: usize) -> bool {
        if start == 0
            || size == 0
            || start & (PAGE_SIZE - 1) != 0
        {
            return false;
        }

        let end = match start.checked_add(size) {
            Some(value) => value,
            None => return false,
        };

        // 初期化は一度だけ許可する。
        if self
            .end
            .compare_exchange(
                0,
                end,
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_err()
        {
            return false;
        }

        // nextを最後に公開する。
        self.next.store(start, Ordering::Release);
        true
    }
}

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();

        // ゼロサイズの場合は、アラインメントを満たす
        // 非nullのダングリングポインタを返す。
        if size == 0 {
            return align as *mut u8;
        }

        let mut current = self.next.load(Ordering::Acquire);

        if current == 0 {
            return ptr::null_mut();
        }

        let end = self.end.load(Ordering::Relaxed);

        if end == 0 {
            return ptr::null_mut();
        }

        loop {
            // Layoutのalignは2のべき乗。
            let aligned = match current.checked_add(align - 1) {
                Some(value) => value & !(align - 1),
                None => return ptr::null_mut(),
            };

            let allocation_end = match aligned.checked_add(size) {
                Some(value) => value,
                None => return ptr::null_mut(),
            };

            if allocation_end > end {
                return ptr::null_mut();
            }

            // CASにより、割り当て領域の重複を防ぐ。
            match self.next.compare_exchange_weak(
                current,
                allocation_end,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return aligned as *mut u8,
                Err(actual) => current = actual,
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // 今回は何もしない。
        // Bump Allocatorなので、解放した領域は再利用しない。
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: BumpAllocator = BumpAllocator::new();

pub fn init(start: usize, size: usize) -> bool {
    GLOBAL_ALLOCATOR.init(start, size)
}
