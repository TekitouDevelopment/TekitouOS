use core::arch::asm;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};

const PAGE_SIZE: u64 = 4096;
const PML4_SPAN: u64 = 1 << 39;

const ADDRESS_MASK: u64 = 0x000F_FFFF_FFFF_F000;

const PRESENT: u64 = 1 << 0;
const WRITABLE: u64 = 1 << 1;
const USER_ACCESSIBLE: u64 = 1 << 2;
const HUGE_PAGE: u64 = 1 << 7;
const NO_EXECUTE: u64 = 1 << 63;

const EFER_MSR: u32 = 0xC000_0080;
const EFER_NXE: u32 = 1 << 11;

static mut PML4_PHYSICAL: u64 = 0;
static mut USER_BASE: u64 = 0;
static mut INITIALIZED: bool = false;

/// 現在の4段ページテーブルを利用し、
/// ユーザー用の空きPML4領域を探す。
pub fn init() -> bool {
    if !enable_nxe() {
        return false;
    }

    let cr4: u64;
    let cr3: u64;

    unsafe {
        asm!(
        "mov {}, cr4",
        out(reg) cr4,
        options(nomem, nostack, preserves_flags),
        );

        asm!(
        "mov {}, cr3",
        out(reg) cr3,
        options(nomem, nostack, preserves_flags),
        );
    }

    // 5段ページテーブルには未対応。
    if cr4 & (1 << 12) != 0 {
        return false;
    }

    let old_root = cr3 & ADDRESS_MASK;

    if old_root == 0 {
        return false;
    }

    // 新しいPML4用の物理ページを確保する。
    let new_root = match crate::memory::allocate_page() {
        Some(address) => address,
        None => return false,
    };

    if new_root == old_root
        || new_root % PAGE_SIZE != 0
        || new_root & !ADDRESS_MASK != 0
    {
        return false;
    }

    let old_pml4 = old_root as usize as *const u64;
    let new_pml4 = new_root as usize as *mut u64;

    // 現在のPML4を新しいページにコピーする。
    for index in 0..512 {
        let entry = unsafe {
            read_volatile(old_pml4.add(index))
        };

        unsafe {
            write_volatile(new_pml4.add(index), entry);
        }
    }

    // 既存のマッピングと重複しないユーザー領域を探す。
    let mut selected_base = None;

    for index in 1..256 {
        let entry = unsafe {
            read_volatile(new_pml4.add(index))
        };

        if entry & PRESENT == 0 {
            selected_base = Some((index as u64) << 39);
            break;
        }
    }

    let user_base = match selected_base {
        Some(base) => base,
        None => return false,
    };

    // 以後はコピーしたPML4を使う。
    unsafe {
        asm!(
        "mov cr3, {}",
        in(reg) new_root,
        options(nostack, preserves_flags),
        );

        write_volatile(
            addr_of_mut!(PML4_PHYSICAL),
            new_root,
        );

        write_volatile(
            addr_of_mut!(USER_BASE),
            user_base,
        );

        write_volatile(
            addr_of_mut!(INITIALIZED),
            true,
        );
    }

    true
}

/// ユーザー空間として使用する仮想アドレスの先頭。
pub fn user_base() -> Option<u64> {
    if !unsafe { read_volatile(addr_of!(INITIALIZED)) } {
        return None;
    }

    Some(unsafe { read_volatile(addr_of!(USER_BASE)) })
}

/// 4 KiBの物理ページをユーザー空間にマッピングする。
///
/// executable = true なら実行可能、
/// falseならNXを設定して実行不可にする。
pub fn map_user_page(
    virtual_address: u64,
    physical_address: u64,
    writable: bool,
    executable: bool,
) -> bool {
    if !unsafe { read_volatile(addr_of!(INITIALIZED)) } {
        return false;
    }

    if virtual_address == 0
        || virtual_address % PAGE_SIZE != 0
        || physical_address == 0
        || physical_address % PAGE_SIZE != 0
        || physical_address & !ADDRESS_MASK != 0
    {
        return false;
    }

    let base = unsafe {
        read_volatile(addr_of!(USER_BASE))
    };

    let end = match base.checked_add(PML4_SPAN) {
        Some(value) => value,
        None => return false,
    };

    // 初期化時に選択したユーザー領域の中だけを操作する。
    if virtual_address < base || virtual_address >= end {
        return false;
    }

    let root = unsafe {
        read_volatile(addr_of!(PML4_PHYSICAL))
    };

    let indices = [
        ((virtual_address >> 39) & 0x1FF) as usize,
        ((virtual_address >> 30) & 0x1FF) as usize,
        ((virtual_address >> 21) & 0x1FF) as usize,
        ((virtual_address >> 12) & 0x1FF) as usize,
    ];

    let pml4 = root as usize as *mut u64;

    let pdpt = match get_or_create_table(
        unsafe { pml4.add(indices[0]) },
    ) {
        Some(table) => table,
        None => return false,
    };

    let pd = match get_or_create_table(
        unsafe { pdpt.add(indices[1]) },
    ) {
        Some(table) => table,
        None => return false,
    };

    let pt = match get_or_create_table(
        unsafe { pd.add(indices[2]) },
    ) {
        Some(table) => table,
        None => return false,
    };

    let pte = unsafe {
        pt.add(indices[3])
    };

    let old_entry = unsafe {
        read_volatile(pte)
    };

    // 既存マッピングを誤って上書きしない。
    if old_entry & PRESENT != 0 {
        return false;
    }

    let mut flags = PRESENT | USER_ACCESSIBLE;

    if writable {
        flags |= WRITABLE;
    }

    if !executable {
        flags |= NO_EXECUTE;
    }

    unsafe {
        write_volatile(
            pte,
            physical_address | flags,
        );

        // 新しいマッピングをTLBに反映させる。
        asm!(
        "invlpg [{}]",
        in(reg) virtual_address,
        options(nostack, preserves_flags),
        );
    }

    true
}

/// 存在するページテーブルをたどり、
/// なければ物理ページを確保してテーブルを作る。
fn get_or_create_table(entry_ptr: *mut u64) -> Option<*mut u64> {
    let entry = unsafe {
        read_volatile(entry_ptr)
    };

    if entry & PRESENT != 0 {
        // 大きなページの分割は次の段階で扱う。
        if entry & HUGE_PAGE != 0 {
            return None;
        }

        let updated = entry | WRITABLE | USER_ACCESSIBLE;

        unsafe {
            write_volatile(entry_ptr, updated);
        }

        let physical = updated & ADDRESS_MASK;

        if physical == 0 {
            return None;
        }

        return Some(physical as usize as *mut u64);
    }

    let physical = crate::memory::allocate_page()?;

    if physical == 0
        || physical & !ADDRESS_MASK != 0
    {
        return None;
    }

    // 新しいページテーブルをゼロ初期化する。
    let table = physical as usize as *mut u64;

    for index in 0..512 {
        unsafe {
            write_volatile(table.add(index), 0);
        }
    }

    unsafe {
        write_volatile(
            entry_ptr,
            physical | PRESENT | WRITABLE | USER_ACCESSIBLE,
        );
    }

    Some(table)
}

/// CPUがNXをサポートしていることを確認し、EFER.NXEを有効化する。
fn enable_nxe() -> bool {
    let max_extended = unsafe {
        core::arch::x86_64::__cpuid(0x8000_0000)
    }.eax;

    if max_extended < 0x8000_0001 {
        return false;
    }

    let features = unsafe {
        core::arch::x86_64::__cpuid(0x8000_0001)
    };

    // Extended Feature Flags: EDX bit 20 = NX
    if features.edx & (1 << 20) == 0 {
        return false;
    }

    let mut low: u32;
    let high: u32;

    unsafe {
        asm!(
        "rdmsr",
        in("ecx") EFER_MSR,
        out("eax") low,
        out("edx") high,
        options(nomem, nostack),
        );

        low |= EFER_NXE;

        asm!(
        "wrmsr",
        in("ecx") EFER_MSR,
        in("eax") low,
        in("edx") high,
        options(nomem, nostack),
        );
    }

    true
}