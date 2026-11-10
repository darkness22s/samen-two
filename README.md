# Samen

**Samen** is an independent x86-64 operating-system kernel written in Rust, with a framebuffer desktop and interactive app windows. It is **not** Linux and does not need a host OS at runtime. The current target is QEMU/SeaBIOS on x86-64.

## Features

- Freestanding Rust kernel, `#![no_std]`, x86-64 ELF and Limine boot protocol
- **Native high-resolution desktop:** the original 2048×1152 desert wallpaper is JPEG-decoded once and aspect-correctly scaled to the actual framebuffer; the original SAMEN wordmark is a separate antialiased alpha layer. The old 692×498 flattened screenshot is no longer used at runtime.
- **Actual desktop UI, not a screenshot:** the translucent dock, Chrome/Files/Settings icons, selection feedback and application windows are rendered by Rust code as independent layers.
- **Flicker-resistant compositor:** the scene and wallpaper live in normal RAM; complete scene updates are presented only after composition, while ordinary PS/2 mouse movement restores a small cursor rectangle instead of repainting the screen.
- **High-resolution output:** Limine requests 1920×1080×32 where the display/video firmware supports it, with correct aspect ratio and framebuffer-mode fallback.
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

The original Figma-exported composition is preserved as `assets/source-desktop-figma.png` **for comparison only**, alongside the unmodified original high-resolution `assets/source-wallpaper.jpg`. The SAMEN logo is extracted as an antialiased alpha mask in `assets/logo-alpha-240x95.bin`. At runtime the kernel does **not** draw `assets/samen-desktop.qoi`; it decodes the original wallpaper and paints the dock and icons separately. The assets are embedded into the kernel at build time; the Figma file is never modified.

The initial desktop is visually matched to the supplied design, but independently rendered with higher-resolution assets and proper aspect-correct scaling. Dock hitboxes are mapped through a uniform desktop-space transform; clicking an icon opens or restores its window. The Browser, Files and Settings now have kernel-native interactive windows. Their interiors are provisional because their exact Figma window designs were not supplied. The original desktop artwork remains untouched. The Browser is a local-pages UI rather than a functioning internet browser. The graphical interface currently supports QEMU's legacy PS/2 mouse, not USB HID devices.

## Storage safety

SamenFS1 is a small experimental **raw-disk** filesystem, not FAT or ext4. The ATA driver accesses the **primary-master IDE disk** (LBA0 is the superblock) and assumes a dedicated virtual disk. **Do not attach a physical disk or an image containing data you need.** There is no journaling, partition support, concurrent access, or power-loss safety. `format` erases the filesystem directory.

## Scope

This is an early single-core kernel with a real graphical desktop, **not** yet a general-purpose desktop OS. It does not yet have multitasking, process isolation, networking, USB/SATA/NVMe support, or a userspace. The source, test scripts, and build system are included so functionality can be verified and extended.

## Renderer diagnostics

In the serial recovery shell, run `gui render stats` to inspect the number of full scene redraws and cursor-only updates. Moving the mouse without holding a window title bar must increase only the cursor counter. Run `make test-gui` to verify the physical PS/2 mouse path, native dock pixels, filesystem window interactions, and cursor redraw behavior under QEMU.
