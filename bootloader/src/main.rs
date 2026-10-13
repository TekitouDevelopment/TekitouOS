#![no_std]
#![no_main]

use core::ffi::c_void;
use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "efiapi" fn efi_main(
    _image_handle: *mut c_void,
    _system_table: *mut c_void,
) -> usize {
    0 // EFI_SUCCESS
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {}
}
