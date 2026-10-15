
#![no_std]
#![no_main]

use core::ffi::c_void;
use core::panic::PanicInfo;

use core::mem::{size_of};
use core::ptr::{
    copy_nonoverlapping,
    null_mut,
    read_unaligned,
    write_bytes,
    write_unaligned,
};


#[used]
#[unsafe(link_section = ".sbat")]
static SBAT_SECTION: [u8; include_bytes!("../../sbat.csv").len()] =
    *include_bytes!("../../sbat.csv");

const KERNEL_STACK_PAGES: usize = 16;


const EFI_ALLOCATE_ANY_PAGES: u32 = 0;
const R_X86_64_RELATIVE: u32 = 8;

type EfiStatus = usize;
type EfiHandle = *mut c_void;

const BUFFER_SIZE: usize = 256;

core::arch::global_asm!(
    r#"
    .global tekito_enter_kernel_on_stack
tekito_enter_kernel_on_stack:
    // Win64 ABI:
    // RCX = kernel entry address
    // RDX = BootInfo pointer
    // R8  = new stack top

    mov rax, rcx
    mov rcx, rdx
    mov rsp, r8

    // スタックを16バイト境界に揃える
    and rsp, -16

    // Win64 ABIのshadow spaceを確保
    sub rsp, 32

    // フレームチェーンの終端
    xor rbp, rbp

    // 文字列操作の方向フラグをクリア
    cld

    // 新しいスタックからカーネルを呼び出す
    call rax

.Ltekito_kernel_returned:
    // カーネルは戻らない契約だが、
    // 万一戻ってきた場合にもUEFIへ復帰しない
    cli
    hlt
    jmp .Ltekito_kernel_returned
"#
);

// UEFIの共通テーブルヘッダー
#[repr(C)]
#[allow(dead_code)]
struct EfiTableHeader {
    _signature: u64,
    _revision: u32,
    _header_size: u32,
    _crc32: u32,
    _reserved: u32,
}

// UEFI Simple Text Output Protocol
#[repr(C)]
#[allow(dead_code)]
struct SimpleTextOutputProtocol {
    _reset: *mut c_void,

    output_string: unsafe extern "efiapi" fn(
        this: *mut SimpleTextOutputProtocol,
        string: *const u16,
    ) -> EfiStatus,

    _test_string: *mut c_void,
    _query_mode: *mut c_void,
    _set_mode: *mut c_void,
    _set_attribute: *mut c_void,
    _clear_screen: *mut c_void,
    _set_cursor_position: *mut c_void,
    _enable_cursor: *mut c_void,
    _mode: *mut c_void,
}

// UEFI System Table
#[repr(C)]
#[allow(dead_code)]
struct SystemTable {
    _header: EfiTableHeader,
    _firmware_vendor: *const u16,
    _firmware_revision: u32,

    _console_in_handle: EfiHandle,
    _con_in: *mut c_void,

    _console_out_handle: EfiHandle,
    con_out: *mut SimpleTextOutputProtocol,

    _standard_error_handle: EfiHandle,
    _std_err: *mut SimpleTextOutputProtocol,

    _runtime_services: *mut c_void,
    _boot_services: *mut c_void,

    _number_of_table_entries: usize,
    _configuration_table: *mut c_void,
}

const EFI_BUFFER_TOO_SMALL: usize = (1usize << 63) | 5;
const EFI_INVALID_PARAMETER: usize = (1usize << 63) | 2;

const MEMORY_MAP_INITIAL_CAPACITY: usize = 64 * 1024;
const MEMORY_MAP_EXTRA_CAPACITY: usize = 16 * 1024;
const MEMORY_MAP_MAX_CAPACITY: usize = 4 * 1024 * 1024;

const EFI_SUCCESS: usize = 0;
const EFI_LOADER_DATA: u32 = 2;

const MAX_KERNEL_FILE_SIZE: usize = 4 * 1024 * 1024;
const MAX_KERNEL_IMAGE_SIZE: u64 = 64 * 1024 * 1024;

const KERNEL_RETURN_MAGIC: u64 = 80;

#[repr(C)]
#[derive(Clone, Copy)]
struct EfiGuid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

const LOADED_IMAGE_GUID: EfiGuid = EfiGuid {
    data1: 0x5B1B31A1,
    data2: 0x9562,
    data3: 0x11D2,
    data4: [0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

const SIMPLE_FILE_SYSTEM_GUID: EfiGuid = EfiGuid {
    data1: 0x964E5B22,
    data2: 0x6459,
    data3: 0x11D2,
    data4: [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

type HandleProtocolFn = unsafe extern "efiapi" fn(
    handle: EfiHandle,
    protocol: *const EfiGuid,
    interface: *mut *mut c_void,
) -> EfiStatus;

type AllocatePagesFn = unsafe extern "efiapi" fn(
    allocation_type: u32,
    memory_type: u32,
    pages: usize,
    memory: *mut u64,
) -> EfiStatus;

type AllocatePoolFn = unsafe extern "efiapi" fn(
    memory_type: u32,
    size: usize,
    buffer: *mut *mut c_void,
) -> EfiStatus;

type GetMemoryMapFn = unsafe extern "efiapi" fn(
    memory_map_size: *mut usize,
    memory_map: *mut c_void,
    map_key: *mut usize,
    descriptor_size: *mut usize,
    descriptor_version: *mut u32,
) -> EfiStatus;

type ExitBootServicesFn = unsafe extern "efiapi" fn(
    image_handle: EfiHandle,
    map_key: usize,
) -> EfiStatus;

type FreePoolFn =
unsafe extern "efiapi" fn(buffer: *mut c_void) -> EfiStatus;

#[repr(C)]
struct BootServices {
    _header: EfiTableHeader,

    _raise_tpl: *mut c_void,
    _restore_tpl: *mut c_void,

    allocate_pages: AllocatePagesFn,
    _free_pages: *mut c_void,
    get_memory_map: GetMemoryMapFn,

    allocate_pool: AllocatePoolFn,
    free_pool: FreePoolFn,

    _create_event: *mut c_void,
    _set_timer: *mut c_void,
    _wait_for_event: *mut c_void,
    _signal_event: *mut c_void,
    _close_event: *mut c_void,
    _check_event: *mut c_void,

    _install_protocol_interface: *mut c_void,
    _reinstall_protocol_interface: *mut c_void,
    _uninstall_protocol_interface: *mut c_void,

    handle_protocol: HandleProtocolFn,

    // HandleProtocol の後から ExitBootServices の前まで
    _before_exit_boot_services: [*mut c_void; 9],
    exit_boot_services: ExitBootServicesFn,

    // GetNextMonotonicCount から LocateHandleBuffer まで
    _before_locate_protocol: [*mut c_void; 10],
    locate_protocol: LocateProtocolFn,
}

#[repr(C)]
struct LoadedImageProtocol {
    revision: u32,
    parent_handle: EfiHandle,
    system_table: *mut SystemTable,
    device_handle: EfiHandle,
    _file_path: *mut c_void,
    _reserved: *mut c_void,
    _load_options_size: u32,
    _load_options: *mut c_void,
    _image_base: *mut c_void,
    _image_size: u64,
    _image_code_type: u32,
    _image_data_type: u32,
    _unload: *mut c_void,
}

type OpenVolumeFn = unsafe extern "efiapi" fn(
    this: *mut SimpleFileSystemProtocol,
    root: *mut *mut EfiFileProtocol,
) -> EfiStatus;

#[repr(C)]
struct SimpleFileSystemProtocol {
    revision: u64,
    open_volume: OpenVolumeFn,
}

type FileOpenFn = unsafe extern "efiapi" fn(
    this: *mut EfiFileProtocol,
    new_handle: *mut *mut EfiFileProtocol,
    file_name: *const u16,
    open_mode: u64,
    attributes: u64,
) -> EfiStatus;

type FileCloseFn =
unsafe extern "efiapi" fn(this: *mut EfiFileProtocol) -> EfiStatus;

type FileReadFn = unsafe extern "efiapi" fn(
    this: *mut EfiFileProtocol,
    buffer_size: *mut usize,
    buffer: *mut c_void,
) -> EfiStatus;

#[repr(C)]
struct EfiFileProtocol {
    revision: u64,

    open: FileOpenFn,
    close: FileCloseFn,
    _delete: *mut c_void,
    read: FileReadFn,

    _write: *mut c_void,
    _get_position: *mut c_void,
    _set_position: *mut c_void,
    _get_info: *mut c_void,
    _set_info: *mut c_void,
    _flush: *mut c_void,
}

// ELF64 header
#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Header {
    ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

// ELF64 program header
#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64ProgramHeader {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Dynamic {
    d_tag: i64,
    d_val: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Elf64Rela {
    r_offset: u64,
    r_info: u64,
    r_addend: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BootInfo {
    pub magic: u64,
    pub version: u32,
    pub struct_size: u32,

    pub kernel_load_base: u64,
    pub kernel_size: u64,

    pub framebuffer_base: u64,
    pub framebuffer_size: u64,
    pub framebuffer_width: u32,
    pub framebuffer_height: u32,
    pub pixels_per_scan_line: u32,
    pub pixel_format: u32,
    pub pixel_information: [u32; 4],

    pub memory_map: u64,
    pub memory_map_size: u64,
    pub memory_map_descriptor_size: u64,
    pub memory_map_descriptor_version: u32,
}


unsafe extern "win64" {
    fn tekito_enter_kernel_on_stack(
        entry_address: usize,
        boot_info: *const BootInfo,
        stack_top: u64,
    ) -> !;
}

const BOOT_INFO_MAGIC: u64 = 0x5445_4B49_544F_4F53;

const GRAPHICS_OUTPUT_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data1: 0x9042A9DE,
    data2: 0x23DC,
    data3: 0x4A38,
    data4: [0x96, 0xFB, 0x7A, 0xDE, 0xD0, 0x80, 0x51, 0x6A],
};

#[repr(C)]
struct GraphicsOutputProtocol {
    _query_mode: *mut c_void,
    _set_mode: *mut c_void,
    _blt: *mut c_void,
    mode: *mut GraphicsOutputProtocolMode,
}

#[repr(C)]
struct GraphicsOutputProtocolMode {
    max_mode: u32,
    mode: u32,
    info: *mut GraphicsOutputModeInformation,
    size_of_info: usize,
    framebuffer_base: u64,
    framebuffer_size: usize,
}

#[repr(C)]
struct GraphicsOutputModeInformation {
    version: u32,
    horizontal_resolution: u32,
    vertical_resolution: u32,
    pixel_format: u32,
    pixel_information: [u32; 4],
    pixels_per_scan_line: u32,
}

type LocateProtocolFn = unsafe extern "efiapi" fn(
    protocol: *const EfiGuid,
    registration: *mut c_void,
    interface: *mut *mut c_void,
) -> EfiStatus;















#[unsafe(no_mangle)]
pub extern "efiapi" fn efi_main(
    image_handle: EfiHandle,
    system_table: *mut SystemTable,
) -> EfiStatus {
    if system_table.is_null() {
        loop {
            core::hint::spin_loop();
        }
    }

    println(system_table, "TekitouOS Bootloader");
    println(system_table, "Opening kernel.elf...");

    let boot_services = unsafe {
        (*system_table)._boot_services.cast::<BootServices>()
    };

    if boot_services.is_null() {
        halt(system_table, "ERROR: Boot Services unavailable");
    }

    let mut gop_raw: *mut c_void = null_mut();

    let status = unsafe {
        ((*boot_services).locate_protocol)(
            &GRAPHICS_OUTPUT_PROTOCOL_GUID,
            null_mut(),
            &mut gop_raw,
        )
    };

    if status != EFI_SUCCESS || gop_raw.is_null() {
        halt(system_table, "ERROR: GOP unavailable");
    }

    let gop = gop_raw.cast::<GraphicsOutputProtocol>();

    let mode = unsafe { (*gop).mode };

    if mode.is_null() {
        halt(system_table, "ERROR: GOP mode unavailable");
    }

    let info = unsafe { (*mode).info };

    if info.is_null() {
        halt(system_table, "ERROR: GOP mode info unavailable");
    }

    // 必要な値をローカル変数へ退避
    let (
        framebuffer_base,
        framebuffer_size,
        width,
        height,
        pixels_per_scan_line,
        pixel_format,
        pixel_information,
    ) = unsafe {
        (
            (*mode).framebuffer_base,
            (*mode).framebuffer_size,
            (*info).horizontal_resolution,
            (*info).vertical_resolution,
            (*info).pixels_per_scan_line,
            (*info).pixel_format,
            (*info).pixel_information,
        )
    };

    if width == 0
        || height == 0
        || pixels_per_scan_line < width
        || framebuffer_base == 0
        || framebuffer_size == 0
    {
        halt(system_table, "ERROR: Invalid GOP framebuffer");
    }

    // PixelBltOnly（3）は直接フレームバッファを使う形式ではない
    if pixel_format == 3 {
        halt(system_table, "ERROR: GOP is BLT-only");
    }

    println(system_table, "GOP acquired.");

    // 1. 現在のEFIイメージ情報を取得
    let mut loaded_image_raw: *mut c_void = null_mut();

    let status = unsafe {
        ((*boot_services).handle_protocol)(
            image_handle,
            &LOADED_IMAGE_GUID,
            &mut loaded_image_raw,
        )
    };

    if status != EFI_SUCCESS || loaded_image_raw.is_null() {
        halt(system_table, "ERROR: Loaded Image Protocol");
    }

    let loaded_image =
        loaded_image_raw.cast::<LoadedImageProtocol>();

    let device_handle = unsafe {
        (*loaded_image).device_handle
    };

    if device_handle.is_null() {
        halt(system_table, "ERROR: No boot device");
    }

    // 2. 起動元デバイスのファイルシステムを取得
    let mut fs_raw: *mut c_void = null_mut();

    let status = unsafe {
        ((*boot_services).handle_protocol)(
            device_handle,
            &SIMPLE_FILE_SYSTEM_GUID,
            &mut fs_raw,
        )
    };

    if status != EFI_SUCCESS || fs_raw.is_null() {
        halt(system_table, "ERROR: Simple File System");
    }

    let fs = fs_raw.cast::<SimpleFileSystemProtocol>();

    // 3. ESPのルートディレクトリを開く
    let mut root: *mut EfiFileProtocol = null_mut();

    let status = unsafe {
        ((*fs).open_volume)(fs, &mut root)
    };

    if status != EFI_SUCCESS || root.is_null() {
        halt(system_table, "ERROR: Open ESP");
    }

    // UEFIのファイル名はNUL終端UTF-16
    const KERNEL_PATH: [u16; 11] = [
        b'k' as u16,
        b'e' as u16,
        b'r' as u16,
        b'n' as u16,
        b'e' as u16,
        b'l' as u16,
        b'.' as u16,
        b'e' as u16,
        b'l' as u16,
        b'f' as u16,
        0,
    ];

    let mut kernel_file: *mut EfiFileProtocol = null_mut();

    let status = unsafe {
        ((*root).open)(
            root,
            &mut kernel_file,
            KERNEL_PATH.as_ptr(),
            1, // EFI_FILE_MODE_READ
            0,
        )
    };

    if status != EFI_SUCCESS || kernel_file.is_null() {
        halt(system_table, "ERROR: Cannot open kernel.elf");
    }

    println(system_table, "kernel.elf opened.");

    // 4. ELFファイルを読み込むためのバッファを確保
    let mut kernel_buffer: *mut c_void = null_mut();

    let status = unsafe {
        ((*boot_services).allocate_pool)(
            EFI_LOADER_DATA,
            MAX_KERNEL_FILE_SIZE,
            &mut kernel_buffer,
        )
    };

    if status != EFI_SUCCESS || kernel_buffer.is_null() {
        halt(system_table, "ERROR: AllocatePool failed");
    }

    let mut file_size = MAX_KERNEL_FILE_SIZE;

    let read_status = unsafe {
        ((*kernel_file).read)(
            kernel_file,
            &mut file_size,
            kernel_buffer,
        )
    };

    unsafe {
        ((*kernel_file).close)(kernel_file);
        ((*root).close)(root);
    }

    if read_status != EFI_SUCCESS || file_size == 0 {
        halt(system_table, "ERROR: Reading kernel.elf");
    }

    if file_size >= MAX_KERNEL_FILE_SIZE {
        halt(system_table, "ERROR: kernel.elf too large");
    }

    println(system_table, "kernel.elf read successfully.");

    let kernel_data = kernel_buffer.cast::<u8>();

    // 5. ELFヘッダーを検証
    if file_size < size_of::<Elf64Header>() {
        halt(system_table, "ERROR: ELF header too small");
    }

    let header: Elf64Header = unsafe {
        read_unaligned(kernel_data.cast::<Elf64Header>())
    };

    if &header.ident[0..4] != b"\x7FELF"
        || header.ident[4] != 2 // ELFCLASS64
        || header.ident[5] != 1 // little-endian
        || header.ident[6] != 1 // ELF version
    {
        halt(system_table, "ERROR: Invalid ELF identification");
    }

    if header.e_type != 3 {
        halt(system_table, "ERROR: Expected ET_DYN PIE kernel");
    }

    println(system_table, "ET_DYN PIE kernel detected.");


    if header.e_phentsize as usize
        != size_of::<Elf64ProgramHeader>()
        || header.e_phnum == 0
        || header.e_phnum > 32
    {
        halt(system_table, "ERROR: Invalid ELF program headers");
    }

    let phoff = header.e_phoff as usize;

    let table_size = size_of::<Elf64ProgramHeader>()
        * header.e_phnum as usize;

    let table_end = match phoff.checked_add(table_size) {
        Some(value) => value,
        None => halt(system_table, "ERROR: ELF header overflow"),
    };

    if table_end > file_size {
        halt(system_table, "ERROR: Program headers out of bounds");
    }
    let mut load_segment: Option<Elf64ProgramHeader> = None;
    let mut dynamic_segment: Option<Elf64ProgramHeader> = None;

    for index in 0..header.e_phnum as usize {
        let offset = phoff + index * size_of::<Elf64ProgramHeader>();

        let ph: Elf64ProgramHeader = unsafe {
            read_unaligned(
                kernel_data.add(offset).cast::<Elf64ProgramHeader>(),
            )
        };

        match ph.p_type {
            1 => {
                // PT_LOAD
                if load_segment.is_some() {
                    halt(system_table, "ERROR: Multiple PT_LOAD segments");
                }

                load_segment = Some(ph);
            }
            2 => {
                // PT_DYNAMIC
                if dynamic_segment.is_some() {
                    halt(system_table, "ERROR: Multiple PT_DYNAMIC segments");
                }

                dynamic_segment = Some(ph);
            }
            _ => {}
        }
    }

    let segment = match load_segment {
        Some(value) => value,
        None => halt(system_table, "ERROR: No PT_LOAD segment"),
    };

    let dynamic = match dynamic_segment {
        Some(value) => value,
        None => halt(system_table, "ERROR: No PT_DYNAMIC segment"),
    };


    const PAGE_SIZE: u64 = 4096;

    // 単一 PT_LOAD の基本検証
    if segment.p_memsz == 0
        || segment.p_memsz > MAX_KERNEL_IMAGE_SIZE
        || segment.p_filesz > segment.p_memsz
        || segment.p_flags & 1 == 0
    {
        halt(system_table, "ERROR: Invalid PT_LOAD");
    }

    if segment.p_align > PAGE_SIZE
        || (segment.p_align > 1
        && (!segment.p_align.is_power_of_two()
        || segment.p_vaddr % segment.p_align
        != segment.p_offset % segment.p_align))
        || segment.p_vaddr % PAGE_SIZE
        != segment.p_offset % PAGE_SIZE
    {
        halt(system_table, "ERROR: Unsupported PT_LOAD alignment");
    }

    let segment_end = match segment.p_vaddr.checked_add(segment.p_memsz) {
        Some(value) => value,
        None => halt(system_table, "ERROR: PT_LOAD address overflow"),
    };

    let segment_file_end =
        match segment.p_vaddr.checked_add(segment.p_filesz) {
            Some(value) => value,
            None => halt(system_table, "ERROR: PT_LOAD file range overflow"),
        };

    let file_end = match segment.p_offset.checked_add(segment.p_filesz) {
        Some(value) => value,
        None => halt(system_table, "ERROR: PT_LOAD offset overflow"),
    };

    if file_end > file_size as u64
        || header.e_entry < segment.p_vaddr
        || header.e_entry >= segment_end
    {
        halt(system_table, "ERROR: Invalid PT_LOAD range");
    }

    // PT_DYNAMIC はファイル内に存在し、
    // PT_LOAD のファイル対応範囲に収まる必要がある。
    let dynamic_end = match dynamic.p_vaddr.checked_add(dynamic.p_memsz) {
        Some(value) => value,
        None => halt(system_table, "ERROR: PT_DYNAMIC overflow"),
    };

    let dynamic_file_end =
        match dynamic.p_offset.checked_add(dynamic.p_filesz) {
            Some(value) => value,
            None => halt(system_table, "ERROR: PT_DYNAMIC file overflow"),
        };

    if dynamic.p_memsz == 0
        || dynamic.p_filesz != dynamic.p_memsz
        || dynamic.p_memsz
        % size_of::<Elf64Dynamic>() as u64 != 0
        || dynamic_file_end > file_size as u64
        || dynamic.p_vaddr < segment.p_vaddr
        || dynamic_end > segment_file_end
        || dynamic.p_offset < segment.p_offset
        || dynamic_file_end > file_end
        || dynamic.p_vaddr - segment.p_vaddr
        != dynamic.p_offset - segment.p_offset
    {
        halt(system_table, "ERROR: Invalid PT_DYNAMIC range");
    }

    println(system_table, "ELF layout validated.");

    // リンク時の仮想アドレス範囲をページ単位に丸める
    let image_start = segment.p_vaddr & !(PAGE_SIZE - 1);

    let image_end = match segment_end.checked_add(PAGE_SIZE - 1) {
        Some(value) => value & !(PAGE_SIZE - 1),
        None => halt(system_table, "ERROR: Image size overflow"),
    };

    let image_span = image_end - image_start;

    if image_span == 0
        || image_span > MAX_KERNEL_IMAGE_SIZE + PAGE_SIZE
    {
        halt(system_table, "ERROR: Invalid image span");
    }

    // 固定アドレスではなく、UEFIに任意のページを確保してもらう
    let page_count = (image_span / PAGE_SIZE) as usize;
    let mut allocated_base: u64 = 0;

    let status = unsafe {
        ((*boot_services).allocate_pages)(
            EFI_ALLOCATE_ANY_PAGES,
            EFI_LOADER_DATA,
            page_count,
            &mut allocated_base,
        )
    };

    if status != EFI_SUCCESS || allocated_base == 0 {
        halt(system_table, "ERROR: AllocatePages failed");
    }

    let allocated_end = match allocated_base.checked_add(image_span) {
        Some(value) => value,
        None => halt(system_table, "ERROR: Allocated range overflow"),
    };

    let allocation_ptr = allocated_base as *mut u8;

    // PT_LOAD を配置し、ファイルにない BSS 部分をゼロ初期化する
    let destination_offset = (segment.p_vaddr - image_start) as usize;

    unsafe {
        copy_nonoverlapping(
            kernel_data.add(segment.p_offset as usize),
            allocation_ptr.add(destination_offset),
            segment.p_filesz as usize,
        );

        write_bytes(
            allocation_ptr.add(
                destination_offset + segment.p_filesz as usize
            ),
            0,
            (segment.p_memsz - segment.p_filesz) as usize,
        );

        ((*boot_services).free_pool)(kernel_buffer);
    }

    println(system_table, "Kernel image loaded.");

    // 実際のロード位置とリンク時の位置との差
    let load_bias =
        allocated_base as i128 - image_start as i128;

    // ロード済みイメージ内の PT_DYNAMIC を参照
    let dynamic_offset = (dynamic.p_vaddr - image_start) as usize;

    let dynamic_ptr = unsafe {
        allocation_ptr
            .add(dynamic_offset)
            .cast::<Elf64Dynamic>()
    };

    let dynamic_count =
        (dynamic.p_memsz / size_of::<Elf64Dynamic>() as u64) as usize;

    let mut rela_addr: Option<u64> = None;
    let mut rela_size: Option<u64> = None;
    let mut rela_ent: Option<u64> = None;
    let mut saw_null = false;

    // 動的テーブルから RELA 情報を得る
    for index in 0..dynamic_count {
        let item = unsafe {
            read_unaligned(dynamic_ptr.add(index))
        };

        if item.d_tag == 0 {
            saw_null = true;
            break;
        }

        match item.d_tag {
            7 => {
                if rela_addr.replace(item.d_val).is_some() {
                    halt(system_table, "ERROR: Duplicate DT_RELA");
                }
            }
            8 => {
                if rela_size.replace(item.d_val).is_some() {
                    halt(system_table, "ERROR: Duplicate DT_RELASZ");
                }
            }
            9 => {
                if rela_ent.replace(item.d_val).is_some() {
                    halt(system_table, "ERROR: Duplicate DT_RELAENT");
                }
            }

            // 共有ライブラリやPLT、REL/RELR方式、
            // 初期化関数などはこのローダーでは未対応。
            1 | 2 | 3 | 12 | 13 | 17 | 18 | 19 | 20
            | 22 | 23 | 25 | 26 | 27 | 28 | 32 | 33
            | 35 | 36 | 37 => {
                halt(system_table, "ERROR: Unsupported dynamic feature");
            }

            // DT_FLAGS、DT_FLAGS_1、DT_DEBUG、シンボル表、
            // ハッシュ表など、今回使わない情報は読み飛ばす。
            _ => {}
        }
    }

    if !saw_null {
        halt(system_table, "ERROR: Missing DT_NULL");
    }

    match (rela_addr, rela_size, rela_ent) {
        // 再配置情報が存在しないカーネル
        (None, None, None) => {
            println(system_table, "No PIE relocations required.");
        }

        // DT_RELASZ が 0 なら、再配置対象は存在しない
        (None, Some(0), None) => {
            println(system_table, "No PIE relocations required.");
        }

        (None, Some(0), Some(ent))
        if ent == size_of::<Elf64Rela>() as u64 =>
            {
                println(system_table, "No PIE relocations required.");
            }

        // 通常の再配置情報がそろっている場合
        (Some(rela_addr), Some(rela_size), Some(rela_ent)) => {
            if rela_ent != size_of::<Elf64Rela>() as u64
                || rela_size % rela_ent != 0
                || rela_size > MAX_KERNEL_IMAGE_SIZE
            {
                halt(system_table, "ERROR: Invalid RELA table");
            }

            if rela_size == 0 {
                println(system_table, "No PIE relocations required.");
            } else {
                let rela_end = match rela_addr.checked_add(rela_size) {
                    Some(value) => value,
                    None => halt(
                        system_table,
                        "ERROR: RELA range overflow",
                    ),
                };

                if rela_addr < segment.p_vaddr
                    || rela_end > segment_file_end
                {
                    halt(
                        system_table,
                        "ERROR: RELA table outside PT_LOAD",
                    );
                }

                let rela_ptr = unsafe {
                    allocation_ptr
                        .add((rela_addr - image_start) as usize)
                        .cast::<Elf64Rela>()
                };

                let relocation_count =
                    (rela_size / rela_ent) as usize;

                println(system_table, "Applying PIE relocations...");

                for index in 0..relocation_count {
                    let rela = unsafe {
                        read_unaligned(rela_ptr.add(index))
                    };

                    let relocation_type =
                        (rela.r_info & 0xFFFF_FFFF) as u32;
                    let symbol_index = rela.r_info >> 32;

                    if relocation_type != R_X86_64_RELATIVE
                        || symbol_index != 0
                    {
                        halt(
                            system_table,
                            "ERROR: Unsupported relocation type",
                        );
                    }

                    let target_end = match rela.r_offset.checked_add(8) {
                        Some(value) => value,
                        None => halt(
                            system_table,
                            "ERROR: Relocation overflow",
                        ),
                    };

                    if rela.r_offset < segment.p_vaddr
                        || target_end > segment_end
                    {
                        halt(
                            system_table,
                            "ERROR: Relocation target outside image",
                        );
                    }

                    let relocated_value =
                        load_bias + rela.r_addend as i128;

                    if relocated_value < 0
                        || relocated_value > u64::MAX as i128
                    {
                        halt(
                            system_table,
                            "ERROR: Relocation value overflow",
                        );
                    }

                    let target = unsafe {
                        allocation_ptr
                            .add((rela.r_offset - image_start) as usize)
                            .cast::<u64>()
                    };

                    unsafe {
                        write_unaligned(
                            target,
                            relocated_value as u64,
                        );
                    }
                }

                println(system_table, "PIE relocations applied.");
            }
        }

        // 情報が一部だけ欠けている場合は不整合として拒否
        _ => {
            halt(
                system_table,
                "ERROR: Incomplete RELA metadata",
            );
        }
    }

    println(system_table, "PIE relocations applied.");

    // ELF のエントリーポイントも実際のロード位置に合わせる。
    let runtime_entry = load_bias + header.e_entry as i128;

    let runtime_segment_start =
        load_bias + segment.p_vaddr as i128;
    let runtime_segment_end =
        load_bias + segment_end as i128;

    if runtime_entry < runtime_segment_start
        || runtime_entry >= runtime_segment_end
        || runtime_entry < allocated_base as i128
        || runtime_entry >= allocated_end as i128
    {
        halt(system_table, "ERROR: Invalid relocated entry point");
    }

    let mut boot_info = BootInfo {
        magic: BOOT_INFO_MAGIC,
        version: 2,
        struct_size: size_of::<BootInfo>() as u32,

        kernel_load_base: allocated_base,
        kernel_size: image_span,

        framebuffer_base,
        framebuffer_size: framebuffer_size as u64,
        framebuffer_width: width,
        framebuffer_height: height,
        pixels_per_scan_line,
        pixel_format,
        pixel_information,

        memory_map: 0,
        memory_map_size: 0,
        memory_map_descriptor_size: 0,
        memory_map_descriptor_version: 0,
    };

    // BootInfoをブートローダーのスタックから独立させる
    if size_of::<BootInfo>() > PAGE_SIZE as usize {
        halt(system_table, "ERROR: BootInfo too large");
    }

    let mut boot_info_base: u64 = 0;

    let status = unsafe {
        ((*boot_services).allocate_pages)(
            EFI_ALLOCATE_ANY_PAGES,
            EFI_LOADER_DATA,
            1,
            &mut boot_info_base,
        )
    };

    if status != EFI_SUCCESS || boot_info_base == 0 {
        halt(system_table, "ERROR: BootInfo allocation failed");
    }

    let boot_info_ptr = boot_info_base as usize as *mut BootInfo;

    unsafe {
        boot_info_ptr.write(boot_info);
    }


    println(system_table, "Calling PIE kernel entry...");


    println(system_table, "Preparing ExitBootServices...");


    // カーネル用スタックを確保する（16ページ = 64 KiB）
    let mut kernel_stack_base: u64 = 0;

    let status = unsafe {
        ((*boot_services).allocate_pages)(
            EFI_ALLOCATE_ANY_PAGES,
            EFI_LOADER_DATA,
            KERNEL_STACK_PAGES,
            &mut kernel_stack_base,
        )
    };

    if status != EFI_SUCCESS || kernel_stack_base == 0 {
        halt(system_table, "ERROR: Kernel stack allocation failed");
    }

    let kernel_stack_size =
        (KERNEL_STACK_PAGES as u64) * PAGE_SIZE;

    let kernel_stack_top = match
    kernel_stack_base.checked_add(kernel_stack_size)
    {
        Some(top) => top,
        None => halt(system_table, "ERROR: Kernel stack overflow"),
    };


    // メモリマップ用バッファを事前確保する。
    let mut memory_map_buffer: *mut c_void = null_mut();
    let mut memory_map_capacity = MEMORY_MAP_INITIAL_CAPACITY;

    let status = unsafe {
        ((*boot_services).allocate_pool)(
            EFI_LOADER_DATA,
            memory_map_capacity,
            &mut memory_map_buffer,
        )
    };

    if status != EFI_SUCCESS || memory_map_buffer.is_null() {
        halt(system_table, "ERROR: Memory map buffer allocation failed");
    }

    let mut exit_attempted = false;

    loop {
        let mut memory_map_size = memory_map_capacity;
        let mut map_key = 0usize;
        let mut descriptor_size = 0usize;
        let mut descriptor_version = 0u32;

        // 最新のメモリマップと MapKey を取得する。
        let map_status = unsafe {
            ((*boot_services).get_memory_map)(
                &mut memory_map_size,
                memory_map_buffer,
                &mut map_key,
                &mut descriptor_size,
                &mut descriptor_version,
            )
        };

        if map_status == EFI_BUFFER_TOO_SMALL {
            let new_capacity = match memory_map_size
                .checked_add(MEMORY_MAP_EXTRA_CAPACITY)
            {
                Some(value) if value <= MEMORY_MAP_MAX_CAPACITY => value,
                _ => {
                    if exit_attempted {
                        loop {
                            core::hint::spin_loop();
                        }
                    }

                    halt(system_table, "ERROR: Memory map too large");
                }
            };

            // 初回の ExitBootServices が失敗した後も、
            // メモリ確保・解放は再試行に必要な用途で行える。
            let mut new_buffer: *mut c_void = null_mut();

            let alloc_status = unsafe {
                ((*boot_services).allocate_pool)(
                    EFI_LOADER_DATA,
                    new_capacity,
                    &mut new_buffer,
                )
            };

            if alloc_status != EFI_SUCCESS || new_buffer.is_null() {
                if exit_attempted {
                    loop {
                        core::hint::spin_loop();
                    }
                }

                halt(system_table, "ERROR: Memory map buffer growth failed");
            }

            let free_status = unsafe {
                ((*boot_services).free_pool)(memory_map_buffer)
            };

            if free_status != EFI_SUCCESS {
                if exit_attempted {
                    loop {
                        core::hint::spin_loop();
                    }
                }

                halt(system_table, "ERROR: FreePool failed");
            }

            memory_map_buffer = new_buffer;
            memory_map_capacity = new_capacity;

            // バッファ確保でメモリマップが変わり得るため、再取得する。
            continue;
        }

        if map_status != EFI_SUCCESS {
            if exit_attempted {
                loop {
                    core::hint::spin_loop();
                }
            }

            halt(system_table, "ERROR: GetMemoryMap failed");
        }

        if descriptor_size == 0
            || memory_map_size > memory_map_capacity
            || memory_map_size % descriptor_size != 0
        {
            if exit_attempted {
                loop {
                    core::hint::spin_loop();
                }
            }

            halt(system_table, "ERROR: Invalid memory map");
        }

        // BootInfo に取得したメモリマップ情報を記録する。
        boot_info.memory_map = memory_map_buffer as usize as u64;
        boot_info.memory_map_size = memory_map_size as u64;
        boot_info.memory_map_descriptor_size = descriptor_size as u64;
        boot_info.memory_map_descriptor_version = descriptor_version;

        // 専用ページ上のBootInfoも最新の内容に更新する
        unsafe {
            boot_info_ptr.write(boot_info);
        }

        // GetMemoryMap の MapKey を使って UEFI を終了する。
        exit_attempted = true;

        let exit_status = unsafe {
            ((*boot_services).exit_boot_services)(
                image_handle,
                map_key,
            )
        };

        if exit_status == EFI_SUCCESS {
            break;
        }

        if exit_status != EFI_INVALID_PARAMETER {
            // 終了試行後はコンソール出力などをしない。
            loop {
                core::hint::spin_loop();
            }
        }

        // MapKey が無効になった場合はメモリマップからやり直す。
    }

    unsafe {
        tekito_enter_kernel_on_stack(
            runtime_entry as usize,
            boot_info_ptr as *const BootInfo,
            kernel_stack_top,
        );
    }
}


// TekitouOSのprintln関数
fn println(system_table: *mut SystemTable, text: &str) {
    if system_table.is_null() {
        return;
    }

    // UEFIから渡されたSystem Tableを参照
    let console = unsafe { (*system_table).con_out };

    if console.is_null() {
        return;
    }

    let mut buffer = [0u16; BUFFER_SIZE];
    let mut len = 0usize;

    // UTF-8の文字列をUTF-16に変換
    // バッファがいっぱいになったら出力して続きを送る
    for ch in text.chars() {
        let mut encoded = [0u16; 2];
        let units = ch.encode_utf16(&mut encoded);

        if len + units.len() > BUFFER_SIZE - 1 {
            // SAFETY: consoleは有効なプロトコル。
            unsafe {
                flush_console(console, &mut buffer, len);
            }
            len = 0;
        }

        buffer[len..len + units.len()].copy_from_slice(units);
        len += units.len();
    }

    // CRLFを追加する領域がなければ、先に出力する
    if len + 2 > BUFFER_SIZE - 1 {
        // SAFETY: consoleは有効なプロトコル。
        unsafe {
            flush_console(console, &mut buffer, len);
        }
        len = 0;
    }

    // UEFIコンソール用の改行（CR + LF）
    buffer[len] = b'\r' as u16;
    buffer[len + 1] = b'\n' as u16;
    len += 2;

    // SAFETY: consoleは有効なプロトコル。
    unsafe {
        flush_console(console, &mut buffer, len);
    }
}


fn halt(system_table: *mut SystemTable, message: &str) -> ! {
    println(system_table, message);

    loop {
        core::hint::spin_loop();
    }
}

fn u64_to_str<'a>(mut value: u64, buffer: &'a mut [u8; 20]) -> &'a str {
    let mut pos = buffer.len();

    loop {
        pos -= 1;
        buffer[pos] = b'0' + (value % 10) as u8;
        value /= 10;

        if value == 0 {
            break;
        }
    }

    // バッファに書いた文字はすべてASCIIの数字なのでUTF-8として有効。
    unsafe {
        core::str::from_utf8_unchecked(&buffer[pos..])
    }
}


// NUL終端UTF-16文字列をUEFIコンソールへ送る
// console は有効なUEFIの出力プロトコルへのポインタであること。
unsafe fn flush_console(
    console: *mut SimpleTextOutputProtocol,
    buffer: &mut [u16; BUFFER_SIZE],
    len: usize,
) {
    buffer[len] = 0;

    // console はUEFIから取得した有効なプロトコル。
    unsafe {
        ((*console).output_string)(console, buffer.as_ptr());
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
