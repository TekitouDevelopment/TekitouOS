ENTRY(_start)

PHDRS
{
    image   PT_LOAD    FLAGS(7);
    dynamic PT_DYNAMIC FLAGS(6);
}

SECTIONS
{
    . = 0x100000;

    .text : ALIGN(4096)
    {
        *(.text .text.*)
    } :image

    .rodata : ALIGN(4096)
    {
        *(.rodata .rodata.*)
    } :image

    .data : ALIGN(4096)
    {
        *(.data .data.*)
        *(.got .got.*)
    } :image

        .dynamic : ALIGN(8)
        {
            *(.dynamic)
        } :image :dynamic

        .rela.dyn : ALIGN(8)
        {
            *(.rela.dyn)
        } :image

    .bss (NOLOAD) : ALIGN(4096)
    {
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        *(COMMON)
    } :image

    /DISCARD/ :
    {
        *(.eh_frame .eh_frame.*)
        *(.comment)
        *(.note .note.*)
    }
}
