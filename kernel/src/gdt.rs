use core::arch::asm;
use core::mem::size_of;

const KERNEL_CODE_SELECTOR: u16 = 0x08;
const KERNEL_DATA_SELECTOR: u16 = 0x10;
const TSS_SELECTOR: u16 = 0x18;

const USER_DATA_SELECTOR: u16 = 0x2B;
const USER_CODE_SELECTOR: u16 = 0x33;

const IST_STACK_SIZE: usize = 16 * 1024;
const PRIVILEGE_STACK_SIZE: usize = 16 * 1024;

#[repr(C, packed(4))]
#[allow(dead_code)]
struct TaskStateSegment {
    _reserved0: u32,
    rsp: [u64; 3],
    _reserved1: u64,
    ist: [u64; 7],
    _reserved2: u64,
    _reserved3: u16,
    iomap_base: u16,
}

#[repr(align(16))]
struct TssStorage(TaskStateSegment);

#[repr(align(16))]
struct InterruptStack([u8; IST_STACK_SIZE]);

#[repr(align(16))]
struct PrivilegeStack([u8; PRIVILEGE_STACK_SIZE]);

#[repr(C, packed)]
struct GdtDescriptorPointer {
    limit: u16,
    base: u64,
}

static mut GDT: [u64; 7] = [
    0,                      // Entry 0: Null
    0x00AF_9A00_0000_FFFF, // Entry 1: Kernel code
    0x00CF_9200_0000_FFFF, // Entry 2: Kernel data
    0,                      // Entry 3: TSS low
    0,                      // Entry 4: TSS high
    0x00CF_F200_0000_FFFF, // Entry 5: User data
    0x00AF_FA00_0000_FFFF, // Entry 6: User code
];

static mut TSS: TssStorage = TssStorage(TaskStateSegment {
    _reserved0: 0,
    rsp: [0; 3],
    _reserved1: 0,
    ist: [0; 7],
    _reserved2: 0,
    _reserved3: 0,
    iomap_base: size_of::<TaskStateSegment>() as u16,
});

static mut DOUBLE_FAULT_STACK: InterruptStack =
    InterruptStack([0; IST_STACK_SIZE]);

static mut KERNEL_PRIVILEGE_STACK: PrivilegeStack =
    PrivilegeStack([0; PRIVILEGE_STACK_SIZE]);

fn make_tss_descriptor(base: u64, limit: u32) -> (u64, u64) {
    let limit = limit as u64;

    let low =
        (limit & 0xFFFF)
        | ((base & 0x00FF_FFFF) << 16)
        | (0x9 << 40) // Available 64-bit TSS
        | (1 << 47)   // Present
        | (((limit >> 16) & 0xF) << 48)
        | (((base >> 24) & 0xFF) << 56);

    let high = base >> 32;

    (low, high)
}

pub fn init_gdt() {
    unsafe {
        // Ring3からRing0へ戻るときに使うカーネルスタック
        let kernel_stack_base =
            core::ptr::addr_of_mut!(KERNEL_PRIVILEGE_STACK.0)
                .cast::<u8>();

        let kernel_stack_top =
            kernel_stack_base as u64
                + PRIVILEGE_STACK_SIZE as u64;

        // ダブルフォルト用IST1スタック
        let double_fault_stack_base =
            core::ptr::addr_of_mut!(DOUBLE_FAULT_STACK.0)
                .cast::<u8>();

        let double_fault_stack_top =
            double_fault_stack_base as u64
                + IST_STACK_SIZE as u64;

        let tss_ptr = core::ptr::addr_of_mut!(TSS.0);

        // TSS.RSP0 = Ring3からRing0へ遷移するときのスタック
        let rsp_ptr =
            core::ptr::addr_of_mut!((*tss_ptr).rsp)
                .cast::<u64>();

        rsp_ptr.write_unaligned(kernel_stack_top);

        // TSS.IST1 = ダブルフォルト用スタック
        let ist_ptr =
            core::ptr::addr_of_mut!((*tss_ptr).ist)
                .cast::<u64>();

        ist_ptr.write_unaligned(double_fault_stack_top);

        // I/O bitmapはTSSの範囲外に置く
        let iomap_ptr =
            core::ptr::addr_of_mut!((*tss_ptr).iomap_base);

        iomap_ptr.write_unaligned(
            size_of::<TaskStateSegment>() as u16,
        );

        // TSS記述子をGDTのEntry 3-4に設定する
        let (tss_low, tss_high) = make_tss_descriptor(
            tss_ptr as u64,
            (size_of::<TaskStateSegment>() - 1) as u32,
        );

        let gdt_ptr =
            core::ptr::addr_of_mut!(GDT).cast::<u64>();

        gdt_ptr.add(3).write(tss_low);
        gdt_ptr.add(4).write(tss_high);

        let gdtr = GdtDescriptorPointer {
            limit: (size_of::<[u64; 7]>() - 1) as u16,
            base: gdt_ptr as u64,
        };

        asm!(
        "cli",
        "lgdt [{gdtr}]",

        // Ring0のコードセグメントへ再ロード
        "push {code_selector}",
        "lea rax, [rip + 2f]",
        "push rax",
        "retfq",
        "2:",

        // Ring0のデータセグメントを再ロード
        "mov ax, {data_selector}",
        "mov ds, ax",
        "mov es, ax",
        "mov ss, ax",

        // TSSをTask Registerへロード
        "mov ax, {tss_selector}",
        "ltr ax",

        out("rax") _,
        gdtr = in(reg) &gdtr,
        code_selector = const KERNEL_CODE_SELECTOR,
        data_selector = const KERNEL_DATA_SELECTOR,
        tss_selector = const TSS_SELECTOR,
        );
    }
}