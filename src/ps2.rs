//! Polling PS/2 mouse driver for i8042 / QEMU. Handles signed 3-byte packets.
//! No interrupts, PS/2 keyboard handling, USB or wheel extension yet.
use crate::arch::{inb, outb};

#[derive(Default)]
pub struct Mouse {
    packet: [u8; 3],
    offset: usize,
    pub buttons: u8,
}

pub struct Motion {
    pub dx: i32,
    pub dy: i32,
    pub left_pressed: bool,
    pub left_down: bool,
}

fn wait_input() -> bool {
    for _ in 0..100_000 {
        if unsafe { inb(0x64) } & 2 == 0 { return true; }
        core::hint::spin_loop();
    }
    false
}
fn wait_output() -> bool {
    for _ in 0..100_000 {
        if unsafe { inb(0x64) } & 1 != 0 { return true; }
        core::hint::spin_loop();
    }
    false
}
fn controller(cmd: u8) -> bool {
    if !wait_input() { return false; }
    unsafe { outb(0x64, cmd); }
    true
}
fn data(byte: u8) -> bool {
    if !wait_input() { return false; }
    unsafe { outb(0x60, byte); }
    true
}
fn send_mouse(byte: u8) -> bool {
    if !controller(0xd4) || !data(byte) || !wait_output() { return false; }
    unsafe { inb(0x60) == 0xfa }
}

impl Mouse {
    pub fn init() -> Option<Self> {
        // Drain stale bytes before (re)initializing the controller.
        for _ in 0..32 {
            if unsafe { inb(0x64) } & 1 == 0 { break; }
            unsafe { inb(0x60); }
        }
        if !controller(0xa8) { return None; } // enable auxiliary port
        if !controller(0x20) || !wait_output() { return None; }
        let configuration = unsafe { inb(0x60) };
        if !controller(0x60) || !data((configuration & !0x23) | 0x40) {
            return None; // disable IRQ1/IRQ12, enable translation only as configured
        }
        if !send_mouse(0xf6) || !send_mouse(0xf4) { return None; }
        Some(Self::default())
    }

    pub fn poll(&mut self) -> Option<Motion> {
        let status = unsafe { inb(0x64) };
        if status & 1 == 0 { return None; }
        let byte = unsafe { inb(0x60) };
        if status & 0x20 == 0 { return None; } // keyboard packet
        if self.offset == 0 && byte & 0x08 == 0 { return None; }
        self.packet[self.offset] = byte;
        self.offset += 1;
        if self.offset != 3 { return None; }
        self.offset = 0;
        let [flags, x, y] = self.packet;
        if flags & 0xc0 != 0 { return None; }
        let left = flags & 1 != 0;
        let pressed = left && self.buttons & 1 == 0;
        self.buttons = flags & 7;
        Some(Motion {
            dx: (x as i8) as i32,
            dy: -((y as i8) as i32),
            left_pressed: pressed,
            left_down: left,
        })
    }
}
