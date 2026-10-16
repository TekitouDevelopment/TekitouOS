use core::arch::asm;
use core::ptr;

const PAGE_SIZE: u64 = 4096;

const USER_DATA_SELECTOR: u64 = 0x2B;
const USER_CODE_SELECTOR: u64 = 0x33;

#[unsafe(no_mangle)]
static mut USER_EXIT_KERNEL_RSP: u64 = 0;

#[unsafe(no_mangle)]
static mut USER_EXIT_RETURN_RIP: u64 = 0;

#[unsafe(no_mangle)]
static mut USER_EXIT_STATUS: u64 = 0;

unsafe extern "C" {
    fn enter_user_mode(user_rip: u64, user_rsp: u64) -> u64;
}

core::arch::global_asm!(
    r#"
    .section .text,"ax",@progbits
    .balign 16

    .global enter_user_mode
enter_user_mode:
    cli
    cld

    // Rustの呼び出し規約で保存が必要なレジスタを退避
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15

    // 引数を保持
    mov r12, rdi
    mov r13, rsi

    // 終了時に復元するカーネルスタック
    mov qword ptr [rip + USER_EXIT_KERNEL_RSP], rsp

    // 終了時の復帰先
    lea rax, [rip + .Luser_exit_return]
    mov qword ptr [rip + USER_EXIT_RETURN_RIP], rax

    // Ring3へのiretqフレーム
    push 0x2B
    push r13
    pushfq
    and qword ptr [rsp], -513
    push 0x33
    push r12

    mov ax, 0x2B
    mov ds, ax
    mov es, ax

    iretq

.Luser_exit_return:
    // SYS_EXITでここへ戻ってきたら、保存したレジスタを復元
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx

    // SYS_EXITに指定された終了コードを返す
    mov rax, qword ptr [rip + USER_EXIT_STATUS]
    ret
"#
);

pub fn run_app(code: &[u8]) -> u64 {
    if code.is_empty() || code.len() > PAGE_SIZE as usize {
        halt();
    }

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
    // 物理ページをゼロ初期化してからユーザーコードを書き込む。
    unsafe {
        ptr::write_bytes(
            code_physical as usize as *mut u8,
            0,
            PAGE_SIZE as usize,
        );

        ptr::write_bytes(
            stack_physical as usize as *mut u8,
            0,
            PAGE_SIZE as usize,
        );

        ptr::copy_nonoverlapping(
            code.as_ptr(),
            code_physical as usize as *mut u8,
            code.len(),
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


    // SYS_EXITからカーネルへ復帰する。
    let exit_status = unsafe {
        enter_user_mode(code_virtual, stack_top)
    };

    // スタックを先にアンマップする。
    let stack_frame =
        match crate::paging::unmap_user_page(stack_virtual) {
            Some(address) => address,
            None => halt(),
        };

    if !crate::memory::free_page(stack_frame) {
        halt();
    }

    // 次にコードをアンマップする。
    // 最後のマッピングなら、空になったページテーブルも回収される。
    let code_frame =
        match crate::paging::unmap_user_page(code_virtual) {
            Some(address) => address,
            None => halt(),
        };

    if !crate::memory::free_page(code_frame) {
        halt();
    }

    exit_status
}

fn halt() -> ! {
    loop {
        core::hint::spin_loop();
    }
}