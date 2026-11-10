#!/usr/bin/env python3
"""Boot Samen on QEMU and prove a file survives a shutdown and second boot."""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import select
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent


class Guest:
    def __init__(self, iso: Path, disk: Path, monitor_socket: Path | None = None):
        self.proc = subprocess.Popen(
            [
                "qemu-system-x86_64",
                "-machine", "pc",
                "-cpu", "qemu64",
                "-m", "256M",
                "-smp", "1",
                "-cdrom", str(iso),
                "-boot", "d",
                "-drive", f"file={disk},format=raw,if=ide,index=0",
                "-device", "isa-debug-exit,iobase=0xf4,iosize=0x04",
                "-display", "none",
                "-vga", "std",
                "-monitor", (f"unix:{monitor_socket},server,nowait" if monitor_socket else "none"),
                "-serial", "stdio",
                "-no-reboot",
            ],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            bufsize=0,
        )
        self.log = bytearray()
        self.wait_for("samen> ", timeout=45)

    def wait_for(self, text: str, timeout: float = 15, start: int | None = None) -> str:
        if start is None:
            start = len(self.log)
        deadline = time.monotonic() + timeout
        needle = text.encode()
        while time.monotonic() < deadline:
            if needle in self.log[start:]:
                return self.log[start:].decode("utf-8", errors="replace")
            if self.proc.poll() is not None:
                break
            ready, _, _ = select.select([self.proc.stdout], [], [], 0.1)
            if ready:
                part = os.read(self.proc.stdout.fileno(), 4096)
                if not part:
                    break
                self.log.extend(part)
        output = self.log.decode("utf-8", errors="replace")
        raise AssertionError(f"QEMU did not emit {text!r}; exit={self.proc.poll()}\n{output[-5000:]}")

    def command(self, line: str, expected: str) -> None:
        begin = len(self.log)
        assert self.proc.stdin is not None
        self.proc.stdin.write((line + "\r").encode())
        self.proc.stdin.flush()
        result = self.wait_for("samen> ", start=begin)
        if expected not in result:
            raise AssertionError(
                f"{line!r}: expected {expected!r}\nGuest output:\n{result}"
            )
        print(f"PASS {line!r} -> {expected!r}")

    def exit(self) -> None:
        if self.proc.poll() is None:
            assert self.proc.stdin is not None
            self.proc.stdin.write(b"exit\r")
            self.proc.stdin.flush()
        try:
            code = self.proc.wait(timeout=10)
            if code not in (0, 33):  # isa-debug-exit writes 0x10 => exit 33
                raise AssertionError(f"QEMU exited with {code}: {self.log[-3000:]!r}")
        finally:
            if self.proc.poll() is None:
                self.proc.kill()
                self.proc.wait()

    def close(self) -> None:
        if self.proc.poll() is None:
            self.proc.kill()
            self.proc.wait()


def smoke_test(iso: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="samen-qemu-") as tmp:
        disk = Path(tmp) / "data.img"
        subprocess.run(
            [sys.executable, str(ROOT / "tools" / "mkfs.py"),
             "--output", str(disk), "--size-mib", "16"],
            check=True,
        )

        guest = Guest(iso, disk)
        try:
            guest.command("ls", "No files.")
            guest.command("gui status", "GUI: selected none")
            guest.command("gui files", "GUI: selected Files")
            guest.command("write persisted Saved across reboots", "Saved 'persisted'")
            guest.command("cat persisted", "Saved across reboots")
            guest.exit()
        finally:
            guest.close()

        guest = Guest(iso, disk)
        try:
            guest.command("cat persisted", "Saved across reboots")
            guest.command("rm persisted", "Removed 'persisted'")
            guest.command("ls", "No files.")
            guest.exit()
        finally:
            guest.close()

        guest = Guest(iso, disk)
        try:
            guest.command("ls", "No files.")
            guest.exit()
        finally:
            guest.close()
    print("PASS: disk-backed filesystem persists across QEMU restarts.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--iso", type=Path, required=True)
    args = parser.parse_args()
    smoke_test(args.iso.resolve())
