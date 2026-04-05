/* SPDX-License-Identifier: MIT OR Apache-2.0 */
/* Raspberry Pi 64-bit: GPU loads kernel8.img at 0x80000. */
ENTRY(_start)

__stack_top = 0x8000000;

SECTIONS
{
  . = 0x80000;

  .text.boot : {
    KEEP(*(.text.boot))
  }

  .text : {
    *(.text .text.*)
  }

  .rodata : {
    *(.rodata .rodata.*)
  }

  .data : {
    *(.data .data.*)
  }

  .bss : ALIGN(8) {
    __bss_start = .;
    *(.bss .bss.*)
    *(COMMON)
    __bss_end = .;
    . = ALIGN(8);
  }

  /DISCARD/ : {
    *(.comment*)
    *(.eh_frame*)
    *(.note*)
  }
}
