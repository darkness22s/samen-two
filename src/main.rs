#![no_std]
#![no_main]

// A freestanding 64-bit kernel. No Linux, userspace, heap, or external runtime.
mod arch;
mod ata;

use core::panic::PanicInfo;
use ata::AtaDisk;
use samen_kernel::fs::{BlockDevice, Fs, FsError};

// Limine protocol base revision 4. Delimiters must be retained by the linker.
#[used]
#[unsafe(link_section = ".limine_req_start")]
static REQUEST_START: [u64; 4] = [
    0xf6b8f4b39de7d1ae, 0xfab91a6940fcb9cf,
    0x785c6ed015d3e316, 0x181e920a7852b9d9,
];

#[used]
#[unsafe(link_section = ".limine_reqs")]
static BASE_REVISION: [u64; 3] = [
    0xf9562b2d5c95a6c8, 0x6a7b384944536bdc, 4,
];

#[used]
#[unsafe(link_section = ".limine_req_end")]
static REQUEST_END: [u64; 2] = [0xadc0e0531bb10d03, 0x9572709f31764c62];

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    arch::serial_init();
    kprintln!("");
    kprintln!("  Samen OS 0.1 | x86-64 | Rust bare metal");
    kprintln!("  Serial terminal (COM1, 115200 8N1), no GUI.");
    kprintln!("  Type 'help' for commands.");
    let mut disk = AtaDisk::probe().ok();
    let mut fs = None;
    match disk.as_mut() {
        Some(d) => {
            kprintln!("  ATA disk: {} sectors ({} MiB)", d.sector_count(), d.sector_count() / 2048);
            match Fs::mount(d) {
                Ok(mounted) => {
                    kprintln!("  SamenFS1 mounted; files survive reboots.");
                    fs = Some(mounted);
                }
                Err(FsError::NotFormatted) => kprintln!("  Disk unformatted; type 'format YES' to initialize it."),
                Err(err) => kprintln!("  Filesystem mount error: {:?}", err),
            }
        }
        None => kprintln!("  No compatible primary-master IDE disk found."),
    }
    kprintln!("");
    shell(&mut disk, &mut fs)
}

fn shell(disk: &mut Option<AtaDisk>, fs: &mut Option<Fs>) -> ! {
    let mut buffer = [0u8; 512];
    loop {
        kprint!("samen> ");
        let n = read_line(&mut buffer);
        if let Ok(line) = core::str::from_utf8(&buffer[..n]) {
            dispatch(line.trim(), disk, fs);
        } else {
            kprintln!("Input must be valid UTF-8.");
        }
    }
}

fn read_line(buffer: &mut [u8; 512]) -> usize {
    let mut len = 0;
    loop {
        let Some(byte) = arch::try_read() else {
            core::hint::spin_loop();
            continue;
        };
        match byte {
            b'\r' | b'\n' => {
                kprintln!();
                return len;
            }
            8 | 127 => {
                if len > 0 {
                    len -= 1;
                    arch::write_raw(b"\x08 \x08");
                }
            }
            3 => {
                kprintln!("^C");
                return 0;
            }
            32..=126 => {
                if len < buffer.len() - 1 {
                    buffer[len] = byte;
                    len += 1;
                    arch::write_raw(&[byte]);
                }
            }
            _ => {}
        }
    }
}

fn require_storage<'a>(
    disk: &'a mut Option<AtaDisk>,
    fs: &'a mut Option<Fs>,
) -> Option<(&'a mut AtaDisk, &'a mut Fs)> {
    match (disk.as_mut(), fs.as_mut()) {
        (Some(d), Some(f)) => Some((d, f)),
        _ => {
            kprintln!("No mounted SamenFS1 filesystem. Check 'disk' or use 'format YES'.");
            None
        }
    }
}

fn dispatch(line: &str, disk: &mut Option<AtaDisk>, fs: &mut Option<Fs>) {
    if line.is_empty() {
        return;
    }
    let (command, args) = line.split_once(' ').unwrap_or((line, ""));
    match command {
        "help" => {
            kprintln!("about             Kernel and filesystem details");
            kprintln!("help              Display this help");
            kprintln!("disk              Show disk status");
            kprintln!("ls                List files");
            kprintln!("stat <file>       Show file information");
            kprintln!("cat <file>        Print a file");
            kprintln!("write <file> TEXT Create or replace a text file");
            kprintln!("rm <file>         Delete a file");
            kprintln!("format YES        ERASE filesystem directory on disk");
            kprintln!("reboot            Restart the virtual machine");
            kprintln!("exit              Power off (also QEMU test exit)");
        }
        "about" => {
            kprintln!("Samen 0.1: independent x86-64 Rust kernel using Limine.");
            kprintln!("Single core, serial I/O, ATA PIO, SamenFS1 root directory.");
            kprintln!("No userspace, processes, GUI, or non-IDE storage yet.");
        }
        "disk" => match disk.as_ref() {
            Some(d) => kprintln!("IDE primary master: {} sectors; mounted: {}", d.sector_count(), fs.is_some()),
            None => kprintln!("No ATA primary-master device detected."),
        },
        "ls" => {
            if let Some((_, mounted)) = require_storage(disk, fs) {
                let mut count = 0;
                for file in mounted.files() {
                    kprintln!("{:>8}  {}", file.size, file.name());
                    count += 1;
                }
                if count == 0 {
                    kprintln!("No files.");
                }
            }
        }
        "stat" => {
            if let Some((_, mounted)) = require_storage(disk, fs) {
                match mounted.info(args.trim()) {
                    Ok(f) => kprintln!("{}: {} bytes, start LBA {}, {} sectors",
                        f.name(), f.size, f.first_sector, f.sectors),
                    Err(err) => kprintln!("stat: {:?}", err),
                }
            }
        }
        "cat" => {
            if let Some((d, mounted)) = require_storage(disk, fs) {
                match mounted.read_file(d, args.trim(), arch::write_raw) {
                    Ok(()) => kprintln!(),
                    Err(err) => kprintln!("cat: {:?}", err),
                }
            }
        }
        "write" => {
            let args = args.trim_start();
            let (name, contents) = args.split_once(' ').unwrap_or((args, ""));
            if let Some((d, mounted)) = require_storage(disk, fs) {
                match mounted.write_file(d, name, contents.as_bytes()) {
                    Ok(()) => kprintln!("Saved '{}' ({} bytes).", name, contents.len()),
                    Err(err) => kprintln!("write: {:?}", err),
                }
            }
        }
        "rm" => {
            if let Some((d, mounted)) = require_storage(disk, fs) {
                match mounted.delete_file(d, args.trim()) {
                    Ok(()) => kprintln!("Removed '{}'.", args.trim()),
                    Err(err) => kprintln!("rm: {:?}", err),
                }
            }
        }
        "format" => {
            if args.trim() != "YES" {
                kprintln!("Refusing destructive format. Use exactly: format YES");
                return;
            }
            match disk.as_mut() {
                Some(d) => {
                    match Fs::format(d) {
                        Ok(()) => {
                            *fs = Fs::mount(d).ok();
                            kprintln!("SamenFS1 formatted: {} available data sectors.", d.sector_count() - samen_kernel::fs::DATA_START);
                        }
                        Err(err) => {
                            *fs = None;
                            kprintln!("format: {:?}", err);
                        }
                    }
                }
                None => kprintln!("No disk to format."),
            }
        }
        "reboot" => arch::reboot(),
        "exit" | "shutdown" => arch::poweroff(),
        _ => kprintln!("Unknown command '{}'. Type 'help'.", command),
    }
}

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    kprintln!("SAMEN KERNEL PANIC: {}", info);
    arch::halt()
}
