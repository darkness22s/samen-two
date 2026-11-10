SHELL := /bin/sh
.DEFAULT_GOAL := iso

CARGO ?= cargo
QEMU ?= qemu-system-x86_64
PYTHON ?= python3
BUILD := build
ISO := $(BUILD)/Samen.iso
DISK := $(BUILD)/samen-data.img
LIMINE := $(BUILD)/limine-binary
KERNEL := target/x86_64-unknown-none/release/samen-kernel
HOST_TARGET := $(shell rustc -vV 2>/dev/null | sed -n 's/^host: //p')

.PHONY: all kernel iso disk run test test-qemu clean
all: iso

kernel:
	$(CARGO) build --release --target x86_64-unknown-none

$(LIMINE)/limine:
	mkdir -p $(BUILD)
	curl -fL --retry 3 -o $(BUILD)/limine-binary.tar.gz \
	  https://github.com/Limine-Bootloader/Limine/releases/latest/download/limine-binary.tar.gz
	tar -xzf $(BUILD)/limine-binary.tar.gz -C $(BUILD)
	$(MAKE) -C $(LIMINE)

iso: kernel $(LIMINE)/limine
	rm -rf $(BUILD)/iso_root
	mkdir -p $(BUILD)/iso_root/boot/limine $(BUILD)/iso_root/EFI/BOOT
	cp $(KERNEL) $(BUILD)/iso_root/boot/samen-kernel
	cp limine.conf $(BUILD)/iso_root/boot/limine/limine.conf
	cp $(LIMINE)/limine-bios.sys $(LIMINE)/limine-bios-cd.bin \
	  $(LIMINE)/limine-uefi-cd.bin $(BUILD)/iso_root/boot/limine/
	cp $(LIMINE)/BOOTX64.EFI $(BUILD)/iso_root/EFI/BOOT/
	xorriso -as mkisofs -R -r -J \
	  -b boot/limine/limine-bios-cd.bin -no-emul-boot -boot-load-size 4 -boot-info-table \
	  -hfsplus -apm-block-size 2048 \
	  --efi-boot boot/limine/limine-uefi-cd.bin \
	  -efi-boot-part --efi-boot-image --protective-msdos-label \
	  $(BUILD)/iso_root -o $(ISO)
	$(LIMINE)/limine bios-install $(ISO)
	rm -rf $(BUILD)/iso_root
	@echo "Built $(ISO)"

# No dependencies by design: never silently overwrite a user's persistent disk.
$(DISK):
	$(PYTHON) tools/mkfs.py --output $(DISK) --size-mib 16

disk: $(DISK)

run: iso disk
	$(QEMU) -machine pc -cpu qemu64 -m 256M -smp 1 \
	  -cdrom $(ISO) -boot d \
	  -drive file=$(DISK),format=raw,if=ide,index=0 \
	  -vga std -monitor none -serial stdio -no-reboot

test:
	$(CARGO) test --lib --target $(HOST_TARGET)

test-qemu: iso
	$(PYTHON) tools/test_qemu.py --iso $(ISO)

# Clean artifacts, but NEVER delete the persistent data disk.
clean:
	rm -rf $(BUILD)/iso_root $(ISO) target
	@echo "Retained $(DISK) to protect data."
