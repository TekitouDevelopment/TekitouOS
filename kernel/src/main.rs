#![no_std]
#![no_main]

use core::mem::size_of;
use core::panic::PanicInfo;

const BOOT_INFO_MAGIC: u64 = 0x5445_4B49_544F_4F53;
const BOOT_INFO_VERSION: u32 = 2;

#[repr(C)]
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

    // ExitBootServices 後もカーネルが利用するメモリマップ情報
    pub memory_map: u64,
    pub memory_map_size: u64,
    pub memory_map_descriptor_size: u64,
    pub memory_map_descriptor_version: u32,
}

#[unsafe(no_mangle)]
pub extern "win64" fn _start(
    boot_info: *const BootInfo,
) -> ! {
    // BootInfo ポインターの検証
    if boot_info.is_null() {
        halt();
    }

    // null でないことを確認してから参照する
    let info = unsafe { &*boot_info };

    // BootInfo の形式とバージョンを検証
    if info.magic != BOOT_INFO_MAGIC
        || info.version != BOOT_INFO_VERSION
        || info.struct_size as usize != size_of::<BootInfo>()
    {
        halt();
    }

    // フレームバッファ情報を検証
    if info.framebuffer_base == 0
        || info.framebuffer_size == 0
        || info.framebuffer_width == 0
        || info.framebuffer_height == 0
        || info.pixels_per_scan_line < info.framebuffer_width
        || info.pixel_format == 3
    {
        halt();
    }

    // メモリマップ情報を検証
    if info.memory_map == 0
        || info.memory_map_size == 0
        || info.memory_map_descriptor_size == 0
        || info.memory_map_size % info.memory_map_descriptor_size != 0
    {
        halt();
    }

    // 画面を白くする
    if !fill_screen_white(info) {
        halt();
    }

    loop {
        core::hint::spin_loop();
    }
}

fn fill_screen_white(info: &BootInfo) -> bool {
    let width = info.framebuffer_width as usize;
    let height = info.framebuffer_height as usize;
    let pitch = info.pixels_per_scan_line as usize;

    if info.framebuffer_base == 0
        || info.framebuffer_size == 0
        || width == 0
        || height == 0
        || pitch < width
    {
        return false;
    }

    // 現在対応するピクセル形式の色を指定する。
    let pixel: u32 = match info.pixel_format {
        // RGB/BGR どちらも緑は同じ値になる
        0 | 1 => 0xFFFFFF,

        // PixelBitMask: GreenMask を使う
        2 => {
            let white_mask = info.pixel_information[1];

            if white_mask == 0 {
                return false;
            }

            white_mask
        }

        // PixelBltOnly などは非対応
        _ => return false,
    };

    // フレームバッファの必要サイズを検証
    let required_bytes = match (pitch as u64)
        .checked_mul(height as u64)
        .and_then(|pixels| pixels.checked_mul(4))
    {
        Some(size) => size,
        None => return false,
    };

    if required_bytes > info.framebuffer_size
        || info.framebuffer_base.checked_add(required_bytes).is_none()
    {
        return false;
    }

    let framebuffer =
        info.framebuffer_base as usize as *mut u32;

    // 画面全体を白色にする
    for y in 0..height {
        let row = y * pitch;

        for x in 0..width {
            unsafe {
                framebuffer
                    .add(row + x)
                    .write_volatile(pixel);
            }
        }
    }

    true
}


fn halt() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    halt();
}