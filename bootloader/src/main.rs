
#![no_std]
#![no_main]

use core::ffi::c_void;
use core::panic::PanicInfo;

type EfiStatus = usize;
type EfiHandle = *mut c_void;

const BUFFER_SIZE: usize = 256;

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

use core::mem::{size_of, transmute};
use core::ptr::{
    copy_nonoverlapping,
    null_mut,
    read_unaligned,
    write_bytes,
};

const EFI_SUCCESS: usize = 0;
const EFI_LOADER_DATA: u32 = 2;
const EFI_ALLOCATE_ADDRESS: u32 = 2;

const MAX_KERNEL_FILE_SIZE: usize = 4 * 1024 * 1024;
const MAX_KERNEL_IMAGE_SIZE: u64 = 64 * 1024 * 1024;
const KERNEL_LOAD_ADDRESS: u64 = 0x100000;

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

type FreePoolFn =
unsafe extern "efiapi" fn(buffer: *mut c_void) -> EfiStatus;

#[repr(C)]
struct BootServices {
    _header: EfiTableHeader,

    _raise_tpl: *mut c_void,
    _restore_tpl: *mut c_void,

    allocate_pages: AllocatePagesFn,
    _free_pages: *mut c_void,
    _get_memory_map: *mut c_void,

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

    if header.e_type != 2 && header.e_type != 3 {
        halt(system_table, "ERROR: Unsupported ELF type");
    }

    if header.e_type == 3 {
        println(
            system_table,
            "ET_DYN detected; fixed-address test mode.",
        );
    }


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

    // 今回はPT_LOADがちょうど1個あるELFだけを受け入れる。
    let mut load_segment: Option<Elf64ProgramHeader> = None;

    for index in 0..header.e_phnum as usize {
        let offset = phoff
            + index * size_of::<Elf64ProgramHeader>();

        let ph: Elf64ProgramHeader = unsafe {
            read_unaligned(
                kernel_data
                    .add(offset)
                    .cast::<Elf64ProgramHeader>(),
            )
        };

        if ph.p_type == 1 { // PT_LOAD
            if load_segment.is_some() {
                halt(system_table, "ERROR: Multiple PT_LOAD segments");
            }

            load_segment = Some(ph);
        }
    }

    let segment = match load_segment {
        Some(value) => value,
        None => halt(system_table, "ERROR: No PT_LOAD segment"),
    };

    // 6. 現在のkernel.ldが想定する配置を検証
    if segment.p_vaddr != KERNEL_LOAD_ADDRESS
        || segment.p_paddr != KERNEL_LOAD_ADDRESS
    {
        halt(system_table, "ERROR: Unexpected kernel load address");
    }

    if segment.p_memsz == 0
        || segment.p_memsz > MAX_KERNEL_IMAGE_SIZE
        || segment.p_filesz > segment.p_memsz
    {
        halt(system_table, "ERROR: Invalid segment size");
    }

    let file_end = match segment.p_offset.checked_add(segment.p_filesz) {
        Some(value) => value,
        None => halt(system_table, "ERROR: Segment offset overflow"),
    };

    if file_end > file_size as u64 {
        halt(system_table, "ERROR: Segment exceeds ELF file");
    }

    let segment_end = match segment.p_paddr.checked_add(segment.p_memsz) {
        Some(value) => value,
        None => halt(system_table, "ERROR: Segment address overflow"),
    };

    if header.e_entry < segment.p_paddr
        || header.e_entry >= segment_end
    {
        halt(system_table, "ERROR: Entry point outside segment");
    }

    println(system_table, "ELF header validated.");

    // 7. カーネルのリンク先で物理メモリを確保
    const PAGE_SIZE: u64 = 4096;

    let page_count =
        ((segment.p_memsz + PAGE_SIZE - 1) / PAGE_SIZE) as usize;

    let mut load_address = segment.p_paddr;

    let status = unsafe {
        ((*boot_services).allocate_pages)(
            EFI_ALLOCATE_ADDRESS,
            EFI_LOADER_DATA,
            page_count,
            &mut load_address,
        )
    };

    if status != EFI_SUCCESS || load_address != segment.p_paddr {
        halt(system_table, "ERROR: AllocatePages at 1 MiB failed");
    }

    // 8. ELFのファイル内容を配置し、BSSをゼロにする
    let destination = load_address as *mut u8;

    unsafe {
        copy_nonoverlapping(
            kernel_data.add(segment.p_offset as usize),
            destination,
            segment.p_filesz as usize,
        );

        write_bytes(
            destination.add(segment.p_filesz as usize),
            0,
            (segment.p_memsz - segment.p_filesz) as usize,
        );

        ((*boot_services).free_pool)(kernel_buffer);
    }

    println(system_table, "Kernel loaded into memory.");
    println(system_table, "Calling kernel entry point...");

    // 9. エントリーポイントを呼び出す
    //
    // 今回のkernel::_startはテスト用のwin64 ABI。
    let entry: unsafe extern "win64" fn() -> u64 = unsafe {
        transmute::<usize, unsafe extern "win64" fn() -> u64>(
            header.e_entry as usize,
        )
    };

    let result = unsafe { entry() };

    // テスト用kernelは戻り値を返すため、ここへ戻ってくる。
    if result == KERNEL_RETURN_MAGIC {
        println(system_table, "SUCCESS: Kernel returned expected value!");
    } else {
        println(system_table, "ERROR: Unexpected kernel return value");
    }

    loop {
        core::hint::spin_loop();
    }
}

fn halt(system_table: *mut SystemTable, message: &str) -> ! {
    println(system_table, message);

    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
