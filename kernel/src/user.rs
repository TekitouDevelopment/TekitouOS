use core::arch::asm;
use core::ptr;

const PAGE_SIZE: u64 = 4096;

const USER_DATA_SELECTOR: u64 = 0x2B;
const USER_CODE_SELECTOR: u64 = 0x33;

// push rax; pop rax; int 0x80; jmp $
// ユーザースタックへのアクセスと割り込み呼び出しを試す。
const USER_TEST_CODE: [u8; 18] = [
    // mov eax, 1
    0xB8, 0x01, 0x00, 0x00, 0x00,

    // int 0x80
    0xCD, 0x80,

    // mov edi, eax
    // 1回目の戻り値を第1引数にする
    0x89, 0xC7,

    // mov eax, 2
    0xB8, 0x02, 0x00, 0x00, 0x00,

    // int 0x80
    0xCD, 0x80,

    // jmp $
    0xEB, 0xFE,
];

pub fn run_test() -> ! {
    let base = match crate::paging::user_base() {
        Some(address) => address,
        None => halt(),
    };

    let code_physical = match crate::memory::allocate_page() {
        Some(address) => address,
        None => halt(),
    };

    let stack_physical = match crate::memory::allocate_page() {
        Some(address) => address,
        None => halt(),
    };

    let code_virtual = base;

    let stack_virtual = match base.checked_add(3 * PAGE_SIZE) {
        Some(address) => address,
        None => halt(),
    };

    let stack_top = match base.checked_add(4 * PAGE_SIZE) {
        Some(address) => address,
        None => halt(),
    };

    // 物理ページへユーザーコードを書き込む。
    unsafe {
        ptr::copy_nonoverlapping(
            USER_TEST_CODE.as_ptr(),
            code_physical as usize as *mut u8,
            USER_TEST_CODE.len(),
        );
    }

    // コードページは実行可能・書き込み不可。
    if !crate::paging::map_user_page(
        code_virtual,
        code_physical,
        false,
        true,
    ) {
        halt();
    }

    // スタックページは書き込み可能・実行不可。
    if !crate::paging::map_user_page(
        stack_virtual,
        stack_physical,
        true,
        false,
    ) {
        halt();
    }

    unsafe {
        enter_user_mode(code_virtual, stack_top);
    }
}

unsafe fn enter_user_mode(
    user_rip: u64,
    user_rsp: u64,
) -> ! {
    unsafe {
        asm!(
        "cli",

        // iretq用のスタックフレームを作る。
        "push {user_ss}",
        "push {user_rsp}",
        "pushfq",

        // Ring3では割り込みを有効化せず、IF=0を維持する。
        "and qword ptr [rsp], -513",

        "push {user_cs}",
        "push {user_rip}",

        // データセグメントをユーザー用に切り替える。
        "mov ax, {user_ss}",
        "mov ds, ax",
        "mov es, ax",

        "iretq",

        user_ss = const USER_DATA_SELECTOR,
        user_rsp = in(reg) user_rsp,
        user_cs = const USER_CODE_SELECTOR,
        user_rip = in(reg) user_rip,
        options(noreturn),
        );
    }
}

fn halt() -> ! {
    loop {
        core::hint::spin_loop();
    }
}