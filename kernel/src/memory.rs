
use core::mem::size_of;
use core::ptr::{addr_of, addr_of_mut, read_unaligned, write_volatile};

const PAGE_SIZE: u64 = 4096;
const EFI_CONVENTIONAL_MEMORY: u32 = 7;

#[repr(C)]
#[derive(Clone, Copy)]
struct EfiMemoryDescriptor {
    memory_type: u32,
    _padding: u32,
    physical_start: u64,
    virtual_start: u64,
    number_of_pages: u64,
    attribute: u64,
}

const MAX_TRACKED_ALLOCATIONS: usize = 1024;

#[derive(Clone, Copy)]
struct AllocatorState {
    map_base: u64,
    map_size: usize,
    descriptor_size: usize,
    initialized: bool,
}

const EMPTY_STATE: AllocatorState = AllocatorState {
    map_base: 0,
    map_size: 0,
    descriptor_size: 0,
    initialized: false,
};

#[derive(Clone, Copy)]
struct AllocationRecord {
    start: u64,
    page_count: u64,
}

const EMPTY_ALLOCATION: AllocationRecord = AllocationRecord {
    start: 0,
    page_count: 0,
};

static mut STATE: AllocatorState = EMPTY_STATE;

static mut ALLOCATIONS: [AllocationRecord; MAX_TRACKED_ALLOCATIONS] =
    [EMPTY_ALLOCATION; MAX_TRACKED_ALLOCATIONS];

static mut ALLOCATION_COUNT: usize = 0;


/// UEFIのメモリマップを初期化する。
/// 成功したらtrueを返す。
pub fn init(info: &crate::BootInfo) -> bool {
    if info.memory_map == 0
        || info.memory_map_size == 0
        || info.memory_map_descriptor_size == 0
        || info.memory_map_descriptor_version != 1
        || info.memory_map_size > usize::MAX as u64
        || info.memory_map_descriptor_size > usize::MAX as u64
    {
        return false;
    }

    let map_size = info.memory_map_size as usize;
    let descriptor_size = info.memory_map_descriptor_size as usize;

    if descriptor_size < size_of::<EfiMemoryDescriptor>()
        || map_size % descriptor_size != 0
    {
        return false;
    }

    let descriptor_count = map_size / descriptor_size;

    if descriptor_count == 0 {
        return false;
    }

    unsafe {
        write_volatile(
            addr_of_mut!(ALLOCATION_COUNT),
            0,
        );

        write_volatile(
            addr_of_mut!(STATE),
            AllocatorState {
                map_base: info.memory_map,
                map_size,
                descriptor_size,
                initialized: true,
            },
        );
    }

    true
}

/// 使用可能な物理ページを確保し、その物理アドレスを返す。
pub fn allocate_page() -> Option<u64> {
    allocate_pages(1)
}

pub fn allocate_pages(page_count: usize) -> Option<u64> {
    if page_count == 0 {
        return None;
    }

    let requested_pages = page_count as u64;

    if requested_pages as usize != page_count {
        return None;
    }

    let allocation_size =
        requested_pages.checked_mul(PAGE_SIZE)?;

    let state = unsafe { read_state() };

    if !state.initialized {
        return None;
    }

    let count = unsafe {
        addr_of!(ALLOCATION_COUNT).read_volatile()
    };

    if count >= MAX_TRACKED_ALLOCATIONS {
        return None;
    }

    let descriptor_count =
        state.map_size / state.descriptor_size;

    let allocations =
        unsafe {addr_of!(ALLOCATIONS).cast::<AllocationRecord>()};

    for descriptor_index in 0..descriptor_count {
        let offset = descriptor_index
            .checked_mul(state.descriptor_size)?;

        let descriptor_address =
            state.map_base.checked_add(offset as u64)?;

        let descriptor = unsafe {
            read_unaligned(
                descriptor_address as usize
                    as *const EfiMemoryDescriptor,
            )
        };

        if descriptor.memory_type != EFI_CONVENTIONAL_MEMORY
            || descriptor.number_of_pages < requested_pages
            || descriptor.physical_start % PAGE_SIZE != 0
        {
            continue;
        }

        let region_start = descriptor.physical_start;

        let region_size = descriptor.number_of_pages
            .checked_mul(PAGE_SIZE)?;

        let region_end =
            region_start.checked_add(region_size)?;

        let mut candidate = if region_start == 0 {
            PAGE_SIZE
        } else {
            region_start
        };

        for index in 0..count {
            let record = unsafe {
                allocations.add(index).read()
            };

            let record_size =
                record.page_count.checked_mul(PAGE_SIZE)?;

            let record_end =
                record.start.checked_add(record_size)?;

            if record_end <= candidate {
                continue;
            }

            if candidate.checked_add(allocation_size)?
                <= record.start
            {
                break;
            }

            candidate = record_end;

            if candidate.checked_add(allocation_size)?
                > region_end
            {
                break;
            }
        }

        if candidate.checked_add(allocation_size)?
            > region_end
        {
            continue;
        }

        insert_allocation(
            AllocationRecord {
                start: candidate,
                page_count: requested_pages,
            },
            count,
        );

        return Some(candidate);
    }

    None
}

pub fn free_page(address: u64) -> bool {
    free_pages(address, 1)
}

pub fn free_pages(address: u64, page_count: usize) -> bool {
    if address == 0
        || address % PAGE_SIZE != 0
        || page_count == 0
    {
        return false;
    }

    let pages = page_count as u64;

    if pages as usize != page_count {
        return false;
    }

    let size = match pages.checked_mul(PAGE_SIZE) {
        Some(size) => size,
        None => return false,
    };

    if address.checked_add(size).is_none() {
        return false;
    }

    if !unsafe { read_state() }.initialized {
        return false;
    }

    let count = unsafe {
        addr_of!(ALLOCATION_COUNT).read_volatile()
    };

    let allocations =
        unsafe {addr_of_mut!(ALLOCATIONS).cast::<AllocationRecord>()};

    for index in 0..count {
        let record = unsafe {
            allocations.add(index).read()
        };

        if record.start == address
            && record.page_count == pages
        {
            for move_index in index..count - 1 {
                let next = unsafe {
                    allocations.add(move_index + 1).read()
                };

                unsafe {
                    allocations.add(move_index).write(next);
                }
            }

            unsafe {
                allocations.add(count - 1)
                    .write(EMPTY_ALLOCATION);

                write_volatile(
                    addr_of_mut!(ALLOCATION_COUNT),
                    count - 1,
                );
            }

            return true;
        }
    }

    false
}

fn insert_allocation(
    record: AllocationRecord,
    count: usize,
) {
    let allocations =
        unsafe {addr_of_mut!(ALLOCATIONS).cast::<AllocationRecord>()};

    let mut index = 0;

    while index < count {
        let current = unsafe {
            allocations.add(index).read()
        };

        if current.start >= record.start {
            break;
        }

        index += 1;
    }

    for move_index in (index..count).rev() {
        let current = unsafe {
            allocations.add(move_index).read()
        };

        unsafe {
            allocations.add(move_index + 1).write(current);
        }
    }

    unsafe {
        allocations.add(index).write(record);

        write_volatile(
            addr_of_mut!(ALLOCATION_COUNT),
            count + 1,
        );
    }
}

unsafe fn read_state() -> AllocatorState {
    unsafe { addr_of!(STATE).read_volatile() }
}
