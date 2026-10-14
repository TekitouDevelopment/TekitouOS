#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "win64" fn _start() -> u64 {
    let a1 = 50;
    let a2 = 30;
    let a3 = a1 + a2;

    a3
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}