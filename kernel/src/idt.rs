
use core::arch::asm;
use core::mem::size_of;
use core::sync::atomic::{AtomicUsize, Ordering};

static RING3_TEST_SYSCALL_COUNT: AtomicUsize =
    AtomicUsize::new(0);

const IDT_ENTRIES: usize = 256;
const CPU_EXCEPTION_COUNT: usize = 32;
const KERNEL_CODE_SELECTOR: u16 = 0x08;
const INTERRUPT_GATE_FLAGS: u8 = 0x8E;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    flags: u8,
    offset_middle: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    const MISSING: Self = Self {
        offset_low: 0,
        selector: 0,
        ist: 0,
        flags: 0,
        offset_middle: 0,
        offset_high: 0,
        reserved: 0,
    };

    fn interrupt_gate(handler: u64, ist_index: u8) -> Self {
        Self {
            offset_low: handler as u16,
            selector: KERNEL_CODE_SELECTOR,
            ist: ist_index & 0x07,
            flags: INTERRUPT_GATE_FLAGS,
            offset_middle: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            reserved: 0,
        }
    }


    fn user_interrupt_gate(handler: u64) -> Self {
        Self {
            offset_low: handler as u16,
            selector: KERNEL_CODE_SELECTOR,
            ist: 0,
            flags: 0xEE, // Present + DPL3 + 64-bit interrupt gate
            offset_middle: (handler >> 16) as u16,
            offset_high: (handler >> 32) as u32,
            reserved: 0,
        }
    }
}

#[repr(C, packed)]
struct Idtr {
    limit: u16,
    base: u64,
}

static mut IDT: [IdtEntry; IDT_ENTRIES] =
    [IdtEntry::MISSING; IDT_ENTRIES];

// 例外発生時にデバッガーから確認できる情報
#[unsafe(no_mangle)]
pub static mut LAST_EXCEPTION_VECTOR: u64 = u64::MAX;

#[unsafe(no_mangle)]
pub static mut LAST_EXCEPTION_ERROR_CODE: u64 = 0;

unsafe extern "C" {
    static isr_stub_table: [i32; CPU_EXCEPTION_COUNT];
    static ring3_test_int80_stub: u8;
}

core::arch::global_asm!(
    r#"
    .section .text,"ax",@progbits
    .balign 16

    // エラーコードをCPUが積まない例外
    .macro ISR_NOERR num
    .global isr_stub_\num
    isr_stub_\num:
        push 0
        push \num
        jmp isr_common
    .endm

    // エラーコードをCPUが積む例外
    .macro ISR_ERR num
    .global isr_stub_\num
    isr_stub_\num:
        push \num
        jmp isr_common
    .endm

    // CPU例外 0〜31
    ISR_NOERR 0
    ISR_NOERR 1
    ISR_NOERR 2
    ISR_NOERR 3
    ISR_NOERR 4
    ISR_NOERR 5
    ISR_NOERR 6
    ISR_NOERR 7
    ISR_ERR   8
    ISR_NOERR 9
    ISR_ERR   10
    ISR_ERR   11
    ISR_ERR   12
    ISR_ERR   13
    ISR_ERR   14
    ISR_NOERR 15
    ISR_NOERR 16
    ISR_ERR   17
    ISR_NOERR 18
    ISR_NOERR 19
    ISR_NOERR 20
    ISR_ERR   21
    ISR_NOERR 22
    ISR_NOERR 23
    ISR_NOERR 24
    ISR_NOERR 25
    ISR_NOERR 26
    ISR_NOERR 27
    ISR_NOERR 28
    ISR_ERR   29
    ISR_ERR   30
    ISR_NOERR 31

    // --------------------------------------------------
    // CPU例外の共通ハンドラー
    // --------------------------------------------------
    isr_common:
        cli
        cld

        // Win64 ABI:
        // 第1引数 = ベクター番号
        // 第2引数 = エラーコード
        mov rcx, [rsp]
        mov rdx, [rsp + 8]

        // 32バイトのshadow spaceを確保
        and rsp, -16
        sub rsp, 32

        call exception_handler

    .Lexception_halt:
        cli
        hlt
        jmp .Lexception_halt

        .balign 16
    .global ring3_test_int80_stub
ring3_test_int80_stub:
    cli
    cld

    // 汎用レジスタを保存
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    // 保存したレジスタからシステムコール番号と引数を取得
    // Win64 ABI: RCX, RDX, R8, R9 が第1〜第4引数
    mov rcx, [rsp + 112]  // syscall number (元のRAX)
    mov rdx, [rsp + 72]   // arg1 (元のRDI)
    mov r8,  [rsp + 80]   // arg2 (元のRSI)
    mov r9,  [rsp + 88]   // arg3 (元のRDX)

    // Win64 ABIのshadow space
    sub rsp, 32
    call ring3_test_int80_handler

    // ハンドラーの戻り値を保存済みRAXに格納
    mov [rsp + 144], rax
    add rsp, 32

    // レジスタを復元
    // RAXだけはシステムコールの戻り値になる
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax

    // ユーザー側の実行状態を復元
    iretq

    // --------------------------------------------------
    // CPU例外ハンドラーのアドレステーブル
    // --------------------------------------------------
    .balign 4
    .global isr_stub_table
isr_stub_table:
    .long isr_stub_0 - isr_stub_table
    .long isr_stub_1 - isr_stub_table
    .long isr_stub_2 - isr_stub_table
    .long isr_stub_3 - isr_stub_table
    .long isr_stub_4 - isr_stub_table
    .long isr_stub_5 - isr_stub_table
    .long isr_stub_6 - isr_stub_table
    .long isr_stub_7 - isr_stub_table
    .long isr_stub_8 - isr_stub_table
    .long isr_stub_9 - isr_stub_table
    .long isr_stub_10 - isr_stub_table
    .long isr_stub_11 - isr_stub_table
    .long isr_stub_12 - isr_stub_table
    .long isr_stub_13 - isr_stub_table
    .long isr_stub_14 - isr_stub_table
    .long isr_stub_15 - isr_stub_table
    .long isr_stub_16 - isr_stub_table
    .long isr_stub_17 - isr_stub_table
    .long isr_stub_18 - isr_stub_table
    .long isr_stub_19 - isr_stub_table
    .long isr_stub_20 - isr_stub_table
    .long isr_stub_21 - isr_stub_table
    .long isr_stub_22 - isr_stub_table
    .long isr_stub_23 - isr_stub_table
    .long isr_stub_24 - isr_stub_table
    .long isr_stub_25 - isr_stub_table
    .long isr_stub_26 - isr_stub_table
    .long isr_stub_27 - isr_stub_table
    .long isr_stub_28 - isr_stub_table
    .long isr_stub_29 - isr_stub_table
    .long isr_stub_30 - isr_stub_table
    .long isr_stub_31 - isr_stub_table
"#
);

#[derive(Clone, Copy)]
struct FramebufferInfo {
    base: u64,
    size: u64,
    width: u32,
    height: u32,
    pitch: u32,
    pixel_format: u32,
    pixel_information: [u32; 4],
}

static mut FRAMEBUFFER: FramebufferInfo = FramebufferInfo {
    base: 0,
    size: 0,
    width: 0,
    height: 0,
    pitch: 0,
    pixel_format: 0,
    pixel_information: [0; 4],
};
pub fn init_idt(info: &crate::BootInfo) {
    unsafe {
        core::ptr::addr_of_mut!(FRAMEBUFFER).write(
            FramebufferInfo {
                base: info.framebuffer_base,
                size: info.framebuffer_size,
                width: info.framebuffer_width,
                height: info.framebuffer_height,
                pitch: info.pixels_per_scan_line,
                pixel_format: info.pixel_format,
                pixel_information: info.pixel_information,
            },
        );
    }

    let idt_ptr =  unsafe { core::ptr::addr_of_mut!(IDT)}
        .cast::<IdtEntry>();

    let stub_table = unsafe { core::ptr::addr_of!(isr_stub_table)}
        .cast::<i32>();

    let table_base = stub_table as usize as isize;

    for vector in 0..CPU_EXCEPTION_COUNT {
        let relative = unsafe {
            core::ptr::read_volatile(stub_table.add(vector))
        };

        // テーブルの基準アドレス + 相対オフセット
        let handler = (table_base + relative as isize) as u64;

        // #DF（ダブルフォルト、ベクター8）だけIST1を使う
        let ist_index = if vector == 8 {
            1
        } else {
            0
        };

        //
        // //テスト
        // // テスト中は #UD と #DF の両方で IST1 を使う
        // let ist_index = if vector == 6 || vector == 8 {
        //     1
        // } else {
        //     0
        // };

        unsafe {
            idt_ptr
                .add(vector)
                .write(IdtEntry::interrupt_gate(handler, ist_index));
        }
    }

    let ring3_handler = unsafe {
        core::ptr::addr_of!(ring3_test_int80_stub) as u64
    };

    unsafe {
        idt_ptr
            .add(0x80)
            .write(IdtEntry::user_interrupt_gate(ring3_handler));
    }

    let idtr = Idtr {
        limit: (size_of::<[IdtEntry; IDT_ENTRIES]>() - 1) as u16,
        base: idt_ptr as u64,
    };

    // 新しいIDTをCPUへ登録する。
    // 割り込みは再有効化しない。
    unsafe {
        asm!(
        "lidt [{ptr}]",
        ptr = in(reg) &idtr,
        options(readonly, nostack, preserves_flags)
        );
    }
}

fn fill_screen(colortype: u8) {
    let fb = unsafe {
        core::ptr::read_volatile(
            core::ptr::addr_of!(FRAMEBUFFER)
        )
    };

    let width = fb.width as usize;
    let height = fb.height as usize;
    let pitch = fb.pitch as usize;

    if fb.base == 0
        || fb.size == 0
        || width == 0
        || height == 0
        || pitch < width
    {
        return;
    }

    // 32-bit GOPピクセル形式
    let pixel: u32 = match (fb.pixel_format, colortype) {
        // PixelRedGreenBlueReserved8BitPerColor
        (0, 0) => 0x00FF_0000, // Blue
        (0, 1) => 0x0080_0000, // Dark blue

        // PixelBlueGreenRedReserved8BitPerColor
        (1, 0) => 0x0000_00FF, // Blue
        (1, 1) => 0x0000_0080, // Dark blue

        (0, 2) | (1, 2) => 0x0000_8000, // Dark green

        // PixelBitMask
        (2, 0) => fb.pixel_information[2],
        (2, 1) => {
            let mask = fb.pixel_information[2];

            if mask == 0 {
                return;
            }

            let shift = mask.trailing_zeros();
            let max_value = mask >> shift;
            (max_value / 2 + 1) << shift
        }
        (2, 2) => {
            let mask = fb.pixel_information[1];

            if mask == 0 {
                return;
            }

            let shift = mask.trailing_zeros();
            let max_value = mask >> shift;
            (max_value / 2) << shift
        }

        _ => return,
    };

    let required = match (pitch as u64)
        .checked_mul(height as u64)
        .and_then(|pixels| pixels.checked_mul(4))
    {
        Some(size) if size <= fb.size => size,
        _ => return,
    };

    if fb.base.checked_add(required).is_none() {
        return;
    }

    let framebuffer = fb.base as usize as *mut u32;

    for y in 0..height {
        for x in 0..width {
            unsafe {
                framebuffer
                    .add(y * pitch + x)
                    .write_volatile(pixel);
            }
        }
    }
}

// 例外発生後は復帰せず、停止する。
// ベクター番号とエラーコードはメモリに記録される。
#[unsafe(no_mangle)]
extern "win64" fn exception_handler(
    vector: u64,
    error_code: u64,
) -> ! {
    unsafe {
        core::ptr::addr_of_mut!(LAST_EXCEPTION_VECTOR)
            .write_volatile(vector);

        core::ptr::addr_of_mut!(LAST_EXCEPTION_ERROR_CODE)
            .write_volatile(error_code);
    }

    if vector == 8 {
        fill_screen(1); //dark_blue
    } else {
        fill_screen(0); //blue
    }


    // 復帰せず停止
    loop {
        unsafe {
            core::arch::asm!("cli", "hlt", options(nomem, nostack));
        }
    }
}

const SYSCALL_RETURN_TEST: u64 = 0x5445_4B49;

#[unsafe(no_mangle)]
extern "win64" fn ring3_test_int80_handler(
    number: u64,
    arg1: u64,
    _arg2: u64,
    _arg3: u64,
) -> u64 {
    match number {
        // syscall 1: テスト用の値を返す
        1 => SYSCALL_RETURN_TEST,

        // syscall 2: 渡された引数が正しければ成功
        2 => {
            if arg1 == SYSCALL_RETURN_TEST {
                fill_screen(2);
                0
            } else {
                u64::MAX
            }
        }

        // 未対応のシステムコール
        _ => u64::MAX,
    }
}