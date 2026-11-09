//! Minimal x86-64 platform I/O and a polling 16550 COM1 console.
use core::arch::asm;
use core::fmt::{self, Write};

const COM1: u16 = 0x3f8;

#[inline(always)]
pub unsafe fn outb(port: u16, value: u8) {
    // SAFETY: caller owns the port and runs at ring 0.
    unsafe { asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags)); }
}

#[inline(always)]
pub unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: caller owns the port and runs at ring 0.
    unsafe { asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags)); }
    value
}

#[inline(always)]
pub unsafe fn outw(port: u16, value: u16) {
    // SAFETY: caller owns the port and runs at ring 0.
    unsafe { asm!("out dx, ax", in("dx") port, in("ax") value, options(nomem, nostack, preserves_flags)); }
}

#[inline(always)]
pub unsafe fn inw(port: u16) -> u16 {
    let value: u16;
    // SAFETY: caller owns the port and runs at ring 0.
    unsafe { asm!("in ax, dx", in("dx") port, out("ax") value, options(nomem, nostack, preserves_flags)); }
    value
}

pub fn serial_init() {
    // SAFETY: Samen exclusively owns COM1 in this single-core kernel.
    unsafe {
        outb(COM1 + 1, 0x00); // interrupt disabled; we poll
        outb(COM1 + 3, 0x80); // DLAB
        outb(COM1, 0x01);     // divisor 1 => 115200 baud
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x03); // 8N1
        outb(COM1 + 2, 0xc7); // FIFO
        outb(COM1 + 4, 0x0b); // DTR, RTS, OUT2
    }
}

fn serial_byte(b: u8) {
    if b == b'\n' {
        serial_byte(b'\r');
    }
    // Bounded wait protects against a missing serial controller.
    for _ in 0..1_000_000 {
        if unsafe { inb(COM1 + 5) } & 0x20 != 0 {
            unsafe { outb(COM1, b); }
            return;
        }
        core::hint::spin_loop();
    }
}

pub fn write_raw(bytes: &[u8]) {
    for &byte in bytes {
        serial_byte(byte);
    }
}

pub fn try_read() -> Option<u8> {
    if unsafe { inb(COM1 + 5) } & 1 != 0 {
        Some(unsafe { inb(COM1) })
    } else {
        None
    }
}

struct Serial;
impl Write for Serial {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        write_raw(s.as_bytes());
        Ok(())
    }
}

pub fn print(args: fmt::Arguments<'_>) {
    let _ = Serial.write_fmt(args);
}

#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => { $crate::arch::print(format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! kprintln {
    () => { $crate::kprint!("\n") };
    ($($arg:tt)*) => { $crate::kprint!("{}\n", format_args!($($arg)*)) };
}

pub fn poweroff() -> ! {
    // QEMU isa-debug-exit for automated tests, and ACPI legacy QEMU fallback.
    unsafe {
        outb(0xf4, 0x10);
        outw(0x604, 0x2000);
        outw(0xb004, 0x2000);
    }
    halt()
}

pub fn reboot() -> ! {
    unsafe { outb(0x64, 0xfe); }
    halt()
}

pub fn halt() -> ! {
    loop {
        unsafe { asm!("cli; hlt", options(nomem, nostack)); }
    }
}
