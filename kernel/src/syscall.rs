use core::arch::asm;
use core::ptr::{addr_of_mut, write_volatile};

const SYSCALL_STACK_SIZE: usize = 16 * 1024;

const IA32_EFER: u32 = 0xC000_0080;
const IA32_STAR: u32 = 0xC000_0081;
const IA32_LSTAR: u32 = 0xC000_0082;
const IA32_FMASK: u32 = 0xC000_0084;

const EFER_SCE: u64 = 1;

#[repr(align(16))]
struct SyscallStack([u8; SYSCALL_STACK_SIZE]);

static mut SYSCALL_STACK: SyscallStack =
    SyscallStack([0; SYSCALL_STACK_SIZE]);

#[unsafe(no_mangle)]
static mut SYSCALL_USER_RSP: u64 = 0;

#[unsafe(no_mangle)]
static mut SYSCALL_KERNEL_RSP: u64 = 0;

unsafe extern "C" {
    fn syscall_entry();
}

pub fn init() -> bool {
    // SYSCALL/SYSRET対応を確認する。
    let max_extended =
        unsafe { core::arch::x86_64::__cpuid(0x8000_0000) }.eax;

    if max_extended < 0x8000_0001 {
        return false;
    }

    let features =
        unsafe { core::arch::x86_64::__cpuid(0x8000_0001) };

    // Extended Feature Flags EDX bit 11
    if features.edx & (1 << 11) == 0 {
        return false;
    }

    let stack_base =
        unsafe {core::ptr::addr_of_mut!(SYSCALL_STACK.0)}
            .cast::<u8>();

    let stack_top =
        stack_base as u64 + SYSCALL_STACK_SIZE as u64;

    unsafe {
        write_volatile(
            addr_of_mut!(SYSCALL_KERNEL_RSP),
            stack_top,
        );

        // Kernel CS = 0x08
        // SYSRETQのUser CS = 0x23 + 16 = 0x33
        // SYSRETQのUser SS = 0x23 + 8  = 0x2B
        let star = (0x23u64 << 48) | (0x08u64 << 32);

        write_msr(IA32_STAR, star);
        write_msr(
            IA32_LSTAR,
            syscall_entry as *const () as u64,
        );

        // Ring0でTF/IF/DF/NT/ACをクリアする。
        write_msr(
            IA32_FMASK,
            (1 << 8) | (1 << 9) | (1 << 10)
                | (1 << 14) | (1 << 18),
        );

        // MSR設定の最後にSYSCALLを有効化する。
        let efer = read_msr(IA32_EFER);
        write_msr(IA32_EFER, efer | EFER_SCE);
    }

    true
}

unsafe fn read_msr(index: u32) -> u64 {
    let low: u32;
    let high: u32;

    unsafe {
        asm!(
        "rdmsr",
        in("ecx") index,
        out("eax") low,
        out("edx") high,
        options(nostack),
        );
    }

    ((high as u64) << 32) | low as u64
}

unsafe fn write_msr(index: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;

    unsafe {
        asm!(
        "wrmsr",
        in("ecx") index,
        in("eax") low,
        in("edx") high,
        options(nostack),
        );
    }
}

core::arch::global_asm!(
    r#"
    .section .text,"ax",@progbits
    .balign 16

    .global syscall_entry
syscall_entry:
    // SYSCALLではRSPが自動切り替えされない。
    cli
    cld

    mov qword ptr [rip + SYSCALL_USER_RSP], rsp
    mov rsp, qword ptr [rip + SYSCALL_KERNEL_RSP]

    // 保存:
    // RCX = ユーザーの復帰先
    // R11 = ユーザーのRFLAGS
    // RAX = システムコール番号
    push rcx
    push r11
    push rax
    push rbx
    push rbp
    push rsi
    push rdi
    push rdx
    push r8
    push r9
    push r10
    push r12
    push r13
    push r14
    push r15

    // Rust側はWin64 ABI。
    // number -> RCX
    // arg1   -> RDX
    // arg2   -> R8
    // arg3   -> R9
    mov rcx, [rsp + 96]
    mov rdx, [rsp + 64]
    mov r8,  [rsp + 72]
    mov r9,  [rsp + 56]

    // 32バイトshadow space + 8バイトのアラインメント調整
    sub rsp, 40
    call syscall_dispatch
    add rsp, 40

    // RAXにはディスパッチャーの戻り値を残す。
    // それ以外の保存済みレジスタを復元する。
    pop r15
    pop r14
    pop r13
    pop r12
    pop r10
    pop r9
    pop r8
    pop rdx
    pop rdi
    pop rsi
    pop rbp
    pop rbx

    // 保存していた元のRAX（システムコール番号）は捨てる。
    add rsp, 8

    // SYSRETQ用の復帰先とRFLAGSを復元する。
    pop r11
    pop rcx

    // ユーザースタックへ戻る。
    mov rsp, qword ptr [rip + SYSCALL_USER_RSP]
    sysretq
"#
);