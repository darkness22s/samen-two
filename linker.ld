/* Higher-half freestanding ELF, loaded by the Limine x86-64 protocol. */
OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)

PHDRS
{
    text PT_LOAD FLAGS(5);
    rodata PT_LOAD FLAGS(4);
    data PT_LOAD FLAGS(6);
}

SECTIONS
{
    . = 0xffffffff80000000;

    .text : ALIGN(4096)
    {
        *(.text .text.*)
    } :text

    .rodata : ALIGN(4096)
    {
        *(.rodata .rodata.*)
    } :rodata

    .limine_requests : ALIGN(4096)
    {
        KEEP(*(.limine_req_start))
        KEEP(*(.limine_req_start.*))
        KEEP(*(.limine_reqs))
        KEEP(*(.limine_reqs.*))
        KEEP(*(.limine_req_end))
        KEEP(*(.limine_req_end.*))
    } :data

    .data : ALIGN(4096)
    {
        *(.data .data.*)
    } :data

    .bss (NOLOAD) : ALIGN(4096)
    {
        *(COMMON)
        *(.bss .bss.*)
    } :data

    /DISCARD/ : { *(.eh_frame .eh_frame.*) *(.comment) *(.note .note.*) }
}
