OUTPUT_FORMAT(elf32-i386)
ENTRY(_start)

SECTIONS
{
  . = 0x100000;

  .multiboot ALIGN(4K) :
  {
    KEEP(*(.multiboot))
  }

  .text ALIGN(4K) :
  {
    *(.text .text.*)
  }

  .rodata ALIGN(4K) :
  {
    *(.rodata .rodata.*)
  }

  .data ALIGN(4K) :
  {
    *(.data .data.*)
  }

  .bss ALIGN(4K) :
  {
    *(COMMON)
    *(.bss .bss.*)
  }

  /DISCARD/ :
  {
    *(.comment)
    *(.note*)
  }
}
