
#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

// syscall番号
const SYS_SET_SCREEN_COLOR: u64 = 3;

const COLOR_BLUE: u64 = 0;
const COLOR_DARK_BLUE: u64 = 1;
const COLOR_DARK_GREEN: u64 = 2;
const COLOR_RED: u64 = 3;
const COLOR_GREEN: u64 = 4;
const COLOR_YELLOW: u64 = 5;
const COLOR_PURPLE: u64 = 6;
const COLOR_WHITE: u64 = 7;
const COLOR_BLACK: u64 = 8;

// Ring3のエントリーポイント。
// iretqで直接入るため、最初のスタックを整えてからRust関数を呼ぶ。
core::arch::global_asm!(
    r#"
    .section .text._start,"ax",@progbits
    .global _start
_start:
    and rsp, -16
    call rust_main
1:
    pause
    jmp 1b
"#
);

#[unsafe(no_mangle)]
extern "C" fn rust_main(){
    // 暗緑色を指定
    let result = syscall1(
        SYS_SET_SCREEN_COLOR,
        COLOR_PURPLE,
    );

    if result == 0 {
        let _ = syscall1(
            SYS_SET_SCREEN_COLOR,
            COLOR_YELLOW,
        );
    } else {
        let _ = syscall1(
            SYS_SET_SCREEN_COLOR,
            COLOR_RED,
        );
    }
    //
    // // ひとまずアプリを終了させず、Ring3に留まる
    // let _ = result;
    // loop {
    //     core::hint::spin_loop();
    // }
}

#[inline(always)]
fn syscall1(number: u64, arg1: u64) -> u64 {
    let result: u64;

    unsafe {
        asm!(
        "syscall",
        inlateout("rax") number => result,
        in("rdi") arg1,
        lateout("rcx") _,
        lateout("r11") _,
        );
    }

    result
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
