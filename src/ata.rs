//! Legacy 28-bit LBA ATA PIO driver. Supports QEMU's primary-master IDE disk.
//! Hardware assumptions are explicit: no AHCI/SATA, DMA, IRQs, or partition map.
use crate::arch::{inb, inw, outb, outw};
use samen_kernel::fs::{BlockDevice, FsError, BLOCK_SIZE};

const IO: u16 = 0x1f0;
const CTRL: u16 = 0x3f6;
const DATA: u16 = IO;
const FEATURES: u16 = IO + 1;
const COUNT: u16 = IO + 2;
const LBA0: u16 = IO + 3;
const LBA1: u16 = IO + 4;
const LBA2: u16 = IO + 5;
const DRIVE: u16 = IO + 6;
const COMMAND: u16 = IO + 7;
const STATUS: u16 = IO + 7;

pub struct AtaDisk {
    sectors: u32,
}

fn io_delay() {
    for _ in 0..4 {
        unsafe { inb(CTRL); }
    }
}

fn wait_data() -> Result<(), FsError> {
    for _ in 0..1_000_000 {
        let s = unsafe { inb(STATUS) };
        if s & 0x80 == 0 {
            if s & 0x21 != 0 || s == 0 {
                return Err(FsError::Io);
            }
            if s & 0x08 != 0 {
                return Ok(());
            }
        }
        core::hint::spin_loop();
    }
    Err(FsError::Io)
}

fn wait_ready() -> Result<(), FsError> {
    for _ in 0..1_000_000 {
        let s = unsafe { inb(STATUS) };
        if s & 0x80 == 0 {
            return if s & 0x21 != 0 || s == 0 { Err(FsError::Io) } else { Ok(()) };
        }
        core::hint::spin_loop();
    }
    Err(FsError::Io)
}

impl AtaDisk {
    pub fn probe() -> Result<Self, FsError> {
        unsafe {
            outb(CTRL, 0x02); // disable drive IRQs, use polling
            outb(DRIVE, 0xa0); // primary master
            io_delay();
            outb(COUNT, 0);
            outb(LBA0, 0);
            outb(LBA1, 0);
            outb(LBA2, 0);
            outb(COMMAND, 0xec); // IDENTIFY DEVICE
        }
        if unsafe { inb(STATUS) } == 0 {
            return Err(FsError::Io);
        }
        wait_ready()?;
        // Reject ATAPI devices (e.g. CD-ROMs).
        if unsafe { inb(LBA1) != 0 || inb(LBA2) != 0 } {
            return Err(FsError::Io);
        }
        wait_data()?;
        let mut words = [0u16; 256];
        for word in &mut words {
            *word = unsafe { inw(DATA) };
        }
        if words[49] & (1 << 9) == 0 {
            return Err(FsError::Io); // LBA mode not supported
        }
        let sectors = (u32::from(words[61]) << 16) | u32::from(words[60]);
        if sectors == 0 {
            return Err(FsError::Io);
        }
        Ok(Self { sectors: sectors.min(1 << 28) })
    }

    fn command(&mut self, lba: u32, command: u8) -> Result<(), FsError> {
        if lba >= self.sectors || lba >= (1 << 28) {
            return Err(FsError::Io);
        }
        wait_ready()?;
        unsafe {
            outb(DRIVE, 0xe0 | ((lba >> 24) as u8 & 0x0f));
            io_delay();
            outb(FEATURES, 0);
            outb(COUNT, 1);
            outb(LBA0, lba as u8);
            outb(LBA1, (lba >> 8) as u8);
            outb(LBA2, (lba >> 16) as u8);
            outb(COMMAND, command);
        }
        io_delay();
        wait_data()
    }
}

impl BlockDevice for AtaDisk {
    fn sector_count(&self) -> u32 {
        self.sectors
    }

    fn read_sector(&mut self, lba: u32, out: &mut [u8; BLOCK_SIZE]) -> Result<(), FsError> {
        self.command(lba, 0x20)?;
        for idx in 0..256 {
            let word = unsafe { inw(DATA) }.to_le_bytes();
            out[idx * 2] = word[0];
            out[idx * 2 + 1] = word[1];
        }
        io_delay();
        Ok(())
    }

    fn write_sector(&mut self, lba: u32, input: &[u8; BLOCK_SIZE]) -> Result<(), FsError> {
        self.command(lba, 0x30)?;
        for idx in 0..256 {
            let word = u16::from_le_bytes([input[idx * 2], input[idx * 2 + 1]]);
            unsafe { outw(DATA, word); }
        }
        // Flush after every sector; slower than batching but avoids losing
        // directory/superblock updates when shutting down the emulator.
        unsafe { outb(COMMAND, 0xe7); }
        io_delay();
        wait_ready()
    }
}
