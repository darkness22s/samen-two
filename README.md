# Samen

**Samen** is an independent x86-64 operating-system kernel written in Rust, with a framebuffer desktop and interactive app windows. It is **not** Linux and does not need a host OS at runtime. The current target is QEMU/SeaBIOS on x86-64.

## Features

- Freestanding Rust kernel, `#![no_std]`, x86-64 ELF and Limine boot protocol
- **Original desktop design:** Limine 32-bit framebuffer, pixel-perfect QOI decode of the supplied Figma screenshot, visible software pointer, PS/2 mouse, clickable Chrome/Files/Settings dock icons
- **Interactive windows:** click dock icons to open Files, Settings or Browser; drag title bars; use close, minimize, maximize and restore controls
- **Files:** lists files directly from the mounted SamenFS1 disk; mouse-select to preview, create a real `Welcome.txt`, delete selected files
- **Settings:** live glass/translucency, pointer-speed, and blue/purple accent toggles
- **Browser:** interactive local Home/Help screens. HTTP, HTML rendering and networking are **not implemented**, so this is not a web browser yet
- Polled COM1 serial recovery shell still works while the GUI is on screen
- Primary-master ATA PIO disk driver with error/time-out handling
- **SamenFS1**: persistent, sector-backed, single-directory filesystem with format, create/replace, read, list and delete
- Fully self-contained kernel code (no third-party Rust crates)
- Host-side filesystem unit tests and QEMU smoke/persistence integration tests

## Build and run

On Ubuntu/Debian, install `make curl tar xorriso qemu-system-x86 python3`. Install stable Rust via rustup and add `rustup target add x86_64-unknown-none`. Run:

```sh
make iso
make run
```

`make run` creates `build/samen-data.img` once (16 MiB) and **never overwrites it** unless explicitly requested. Samen opens its graphical desktop in the QEMU VGA window **and** prints a recovery serial prompt in the terminal. Move the mouse and click the Chrome, Files or Settings icons in the translucent dock; the three kernel-rendered windows will open, with working controls. You can also use `gui status`, `gui windows`, `gui browser`, `gui files`, `gui settings`, `gui settings status`, `gui browser status`, and `gui redraw` in the terminal. Try:

```text
help
ls
write greeting Hello from Samen
cat greeting
ls
rm greeting
```

Use `make test` for host filesystem tests; `make test-qemu` for boot and disk persistence; `make test-gui` for framebuffer, PS/2 mouse, window interactions and file creation in QEMU.

A prebuilt, tested `Samen.iso` is uploaded to the [Samen development release](https://github.com/darkness22s/samen-two/releases/tag/samen-dev) on successful main-branch CI runs. The ISO is a boot image; the writable data disk is generated separately with `make disk`.

## Desktop artwork and scope

The original Figma-exported composition is preserved byte-for-byte as `assets/source-desktop-figma.png`, and the separately supplied wallpaper is preserved as `assets/source-wallpaper.jpg`. `assets/samen-desktop.qoi` is a **lossless** RGBA-independent RGB encoding of the composition rendered directly by the kernel; its decoded pixels match the uploaded PNG (no font, blur, wallpaper or icon recreation). The assets are bundled into the kernel at build time; there is no Figma MCP dependency.

The initial desktop uses the exact exported frame and scales to the Limine framebuffer resolution. The three dock hitboxes use source-frame coordinates; clicking one opens or restores its window. The Browser, Files and Settings now have kernel-native interactive windows. Their interiors are provisional because their exact Figma window designs were not supplied. The original desktop artwork remains untouched. The Browser is a local-pages UI rather than a functioning internet browser. The graphical interface currently supports QEMU's legacy PS/2 mouse, not USB HID devices.

## Storage safety

SamenFS1 is a small experimental **raw-disk** filesystem, not FAT or ext4. The ATA driver accesses the **primary-master IDE disk** (LBA0 is the superblock) and assumes a dedicated virtual disk. **Do not attach a physical disk or an image containing data you need.** There is no journaling, partition support, concurrent access, or power-loss safety. `format` erases the filesystem directory.

## Scope

This is an early single-core kernel with a real graphical desktop, **not** yet a general-purpose desktop OS. It does not yet have multitasking, process isolation, networking, USB/SATA/NVMe support, or a userspace. The source, test scripts, and build system are included so functionality can be verified and extended.
