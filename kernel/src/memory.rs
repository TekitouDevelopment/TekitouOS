
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

#[derive(Clone, Copy)]
struct AllocatorState {
    map_base: u64,
    map_size: usize,
    descriptor_size: usize,
    descriptor_index: usize,
    page_offset: u64,
    initialized: bool,
}

const EMPTY_STATE: AllocatorState = AllocatorState {
    map_base: 0,
    map_size: 0,
    descriptor_size: 0,
    descriptor_index: 0,
    page_offset: 0,
    initialized: false,
};

static mut STATE: AllocatorState = EMPTY_STATE;

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

    let state = AllocatorState {
        map_base: info.memory_map,
        map_size,
        descriptor_size,
        descriptor_index: 0,
        page_offset: 0,
        initialized: true,
    };

    unsafe {
        write_volatile(addr_of_mut!(STATE), state);
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

    // x86_64向け。usizeからu64への変換が
    // 元の値を保持できることを確認する。
    if requested_pages as usize != page_count {
        return None;
    }

    let allocation_size = requested_pages.checked_mul(PAGE_SIZE)?;

    let mut state = unsafe { read_state() };

    if !state.initialized {
        return None;
    }

    let descriptor_count =
        state.map_size / state.descriptor_size;

    while state.descriptor_index < descriptor_count {
        let offset = state
            .descriptor_index
            .checked_mul(state.descriptor_size)?;

        let descriptor_address =
            state.map_base.checked_add(offset as u64)?;

        let descriptor_ptr =
            descriptor_address as usize
                as *const EfiMemoryDescriptor;

        let descriptor = unsafe {
            read_unaligned(descriptor_ptr)
        };

        if descriptor.memory_type != EFI_CONVENTIONAL_MEMORY
            || state.page_offset >= descriptor.number_of_pages
        {
            next_descriptor(&mut state);
            continue;
        }

        let remaining =
            descriptor.number_of_pages - state.page_offset;

        // この領域の残りでは連続確保できない。
        if remaining < requested_pages {
            next_descriptor(&mut state);
            continue;
        }

        let page_offset_bytes = match state
            .page_offset
            .checked_mul(PAGE_SIZE)
        {
            Some(value) => value,
            None => {
                next_descriptor(&mut state);
                continue;
            }
        };

        let physical_address = match descriptor
            .physical_start
            .checked_add(page_offset_bytes)
        {
            Some(value) => value,
            None => {
                next_descriptor(&mut state);
                continue;
            }
        };

        if physical_address == 0
            || physical_address % PAGE_SIZE != 0
        {
            state.page_offset += 1;

            if state.page_offset >= descriptor.number_of_pages {
                next_descriptor(&mut state);
            }

            continue;
        }

        if physical_address
            .checked_add(allocation_size)
            .is_none()
        {
            next_descriptor(&mut state);
            continue;
        }

        // 連続した領域を確保した位置までカーソルを進める。
        state.page_offset += requested_pages;

        if state.page_offset >= descriptor.number_of_pages {
            next_descriptor(&mut state);
        }

        save_state(state);
        return Some(physical_address);
    }

    // 確保できなかった場合、保存済みカーソルは変更しない。
    None
}

fn next_descriptor(state: &mut AllocatorState) {
    state.descriptor_index += 1;
    state.page_offset = 0;
}

unsafe fn read_state() -> AllocatorState {
    unsafe { addr_of!(STATE).read_volatile() }
}

fn save_state(state: AllocatorState) {
    unsafe {
        write_volatile(addr_of_mut!(STATE), state);
    }
}
