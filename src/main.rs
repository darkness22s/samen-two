#![no_std]
#![no_main]

// A freestanding 64-bit kernel. No Linux, userspace, heap, or external runtime.
mod arch;
mod ata;
mod gui;
mod dock;
mod ui;
mod terminal;
mod keyboard;
mod ps2;
mod heap;
mod windows;


extern crate alloc;
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
    kprintln!("  Samen OS 0.4 | x86-64 | Rust bare metal");
    kprintln!("  Figma-matched graphical desktop + serial recovery shell.");
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
    // GUI uses the Limine framebuffer, but serial commands and SamenFS1 remain alive.
    if let Some(mut fb) = unsafe { gui::acquire() } {
        kprintln!("GUI init: framebuffer acquired");
        if let Some(mut desktop) = gui::Desktop::initialize(&mut fb) {
            let (width,height) = fb.dimensions();
            kprintln!("GUI: ready framebuffer={}x{} bpp=32", width, height);
            kprintln!("GUI: native 2048x1152 wallpaper, double buffered, 4x4 MSAA dock");
            let mouse = gui::mouse_init();
            kprintln!("GUI: PS/2 mouse {}", if mouse.is_some() { "ready" } else { "unavailable" });
            kprintln!("GUI: Browser/Files/Terminal dock hit areas enabled");
            return graphics_shell(&mut fb, &mut desktop, mouse, &mut disk, &mut fs);
        }
        kprintln!("GUI: image decode failed; using serial recovery shell.");
    } else {
        kprintln!("GUI: no compatible 32-bit RGB framebuffer; using serial recovery shell.");
    }
    shell(&mut disk, &mut fs)
}

fn graphics_shell(
    fb: &mut gui::Framebuffer,
    desktop: &mut gui::Desktop,
    mut mouse: Option<ps2::Mouse>,
    disk: &mut Option<AtaDisk>,
    fs: &mut Option<Fs>,
) -> ! {
    let mut buffer = [0u8; 512];
    let mut len = 0usize;
    let mut keyboard=keyboard::Decoder::new();
    kprint!("samen> ");
    loop {
        if let Some(event) = mouse.as_mut().and_then(|m| m.poll()) {
            match event {
                ps2::Event::Motion(event)=>{
                    let action=desktop.on_mouse(fb,event.dx,event.dy,
                        event.left_pressed,event.left_down,fs.as_ref());
                    apply_gui_action(action,fb,desktop,disk,fs);
                }
                ps2::Event::Scan(scan)=>{
                    if let Some(key)=keyboard.feed(scan) {
                        if desktop.windows.front()==Some(gui::App::Terminal) {
                            if desktop.windows.terminal.key(key,disk,fs) {
                                desktop.windows.sync_files(fs.as_ref());
                                desktop.paint_all(fb);
                            }
                        }
                    }
                }
            }
        }
        let Some(byte) = arch::try_read() else {
            core::hint::spin_loop();
            continue;
        };
        match byte {
            b'\r' | b'\n' => {
                kprintln!();
                match core::str::from_utf8(&buffer[..len]) {
                    Ok(text) => {
                        let command = text.trim();
                        match command {
                            "gui" | "gui redraw" => { desktop.paint_all(fb); kprintln!("GUI: redrawn"); },
                            "gui status" => kprintln!("GUI: selected {} pointer={},{}", desktop.selected_name(),desktop.x,desktop.y),
                            "gui render stats" => kprintln!("GUI: redraws={} cursor_updates={}",desktop.scene_redraws,desktop.cursor_updates),
                            "gui windows" => {
                                for w in desktop.windows.windows.iter() {
                                    kprintln!("GUI: {} open={} minimized={} maximized={} rect={},{},{},{}",w.app.label(),w.open,w.minimized,w.maximized,w.rect.x,w.rect.y,w.rect.w,w.rect.h);
                                }
                            },
                            "gui browser" | "gui chrome" => desktop.choose(fb, gui::App::Browser),
                            "gui browser status" => kprintln!("GUI: browser help={}",desktop.windows.browser_help),
                            "gui files" => {desktop.windows.sync_files(fs.as_ref());desktop.choose(fb, gui::App::Files);},
                            "gui terminal" | "gui settings" => desktop.choose(fb, gui::App::Terminal),
                            "gui terminal status" => kprintln!("GUI: terminal executions={} input='{}' last='{}'",
                                desktop.windows.terminal.executions,
                                desktop.windows.terminal.command_line(),
                                desktop.windows.terminal.last_command()),
                            _ if command.starts_with("gui terminal command ") => {
                                let line=&command["gui terminal command ".len()..];
                                desktop.windows.terminal.run(line,disk,fs);
                                desktop.windows.sync_files(fs.as_ref());
                                desktop.paint_all(fb);
                                kprintln!("GUI: terminal executed '{}'",line);
                            },
                            _ => {dispatch(command,disk,fs); desktop.refresh_files(fb,fs.as_ref());},
                        }
                    }
                    Err(_) => kprintln!("Input must be valid UTF-8."),
                }
                len=0;
                kprint!("samen> ");
            }
            8 | 127 => {
                if len>0 {len-=1;arch::write_raw(b"\x08 \x08");}
            }
            3 => {len=0;kprintln!("^C");kprint!("samen> ");}
            32..=126 => {
                if len<buffer.len()-1 {
                    buffer[len]=byte;len+=1;arch::write_raw(&[byte]);
                }
            }
            _ => {}
        }
    }
}

fn apply_gui_action(
    action:windows::Action,fb:&mut gui::Framebuffer,desktop:&mut gui::Desktop,
    disk:&mut Option<AtaDisk>,fs:&mut Option<Fs>,
){
    match action {
        windows::Action::None=>return,
        windows::Action::CreateSample=>{
            match (disk.as_mut(),fs.as_mut()) {
                (Some(d),Some(f))=>{
                    let name="Welcome.txt";
                    match f.write_file(d,name,b"Welcome to Samen OS. This file is stored on SamenFS1.") {
                        Ok(())=>kprintln!("GUI: created Welcome.txt"),
                        Err(err)=>kprintln!("GUI: create failed: {:?}",err),
                    }
                }
                _=>kprintln!("GUI: no mounted SamenFS1 disk"),
            }
        }
        windows::Action::DeleteSelected=>{
            let name=desktop.windows.file_name();
            match (name,disk.as_mut(),fs.as_mut()) {
                (Some(name),Some(d),Some(f))=>{
                    match f.delete_file(d,name){
                        Ok(())=>kprintln!("GUI: deleted {}",name),
                        Err(err)=>kprintln!("GUI: delete failed: {:?}",err),
                    }
                    desktop.windows.selected_file=None;
                    desktop.windows.clear_preview();
                }
                _=>kprintln!("GUI: select a file and mount SamenFS1 first"),
            }
        }
        windows::Action::PreviewSelected=>{
            let mut contents=[0u8;256];
            let mut count=0usize;
            if let (Some(name),Some(d),Some(f))=
                (desktop.windows.file_name(),disk.as_mut(),fs.as_ref()) {
                let result=f.read_file(d,name,|part|{
                    let take=part.len().min(contents.len()-count);
                    contents[count..count+take].copy_from_slice(&part[..take]);
                    count+=take;
                });
                if result.is_ok(){desktop.windows.set_preview(&contents[..count]);}
                else {desktop.windows.clear_preview();}
            }
        }
    }
    desktop.windows.sync_files(fs.as_ref());
    desktop.paint_all(fb);
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
            kprintln!("gui [status|windows|render stats|redraw|browser|files|terminal]  Graphical desktop");
            kprintln!("gui terminal status / gui terminal command TEXT  Native terminal");
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
            kprintln!("Samen 0.2: independent x86-64 Rust kernel using Limine.");
            kprintln!("Single core, serial I/O, ATA PIO, SamenFS1 root directory.");
            kprintln!("GUI desktop: HD Browser, Files and native Terminal windows.");
            kprintln!("No userspace, processes, or non-IDE storage yet.");
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
