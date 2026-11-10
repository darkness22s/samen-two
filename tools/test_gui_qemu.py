#!/usr/bin/env python3
"""Headless QEMU test: rendered Figma desktop, real i8042 motion and dock clicks."""
from __future__ import annotations

import argparse
from pathlib import Path
import re
import socket
import subprocess
import sys
import tempfile
import time

from test_qemu import Guest, ROOT


def connect_monitor(path: Path) -> socket.socket:
    end = time.monotonic() + 10
    while time.monotonic() < end:
        try:
            conn = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            conn.settimeout(1)
            conn.connect(str(path))
            conn.recv(4096)
            return conn
        except (OSError, TimeoutError):
            time.sleep(0.05)
    raise AssertionError("QEMU HMP socket did not appear")


def cmd(conn: socket.socket, command: str) -> str:
    conn.sendall((command + "\n").encode())
    time.sleep(0.08)
    try:
        return conn.recv(8192).decode("utf8", errors="replace")
    except TimeoutError:
        return ""


def ppm_rgb(path: Path) -> tuple[int,int,bytes]:
    data = path.read_bytes()
    parts = data.split(b"\n", 3)
    if len(parts) != 4 or parts[0] != b"P6" or parts[2] != b"255":
        raise AssertionError(f"Unexpected QEMU screendump PPM header: {data[:80]!r}")
    w,h = map(int, parts[1].split())
    return w,h,parts[3]


def status_pointer(guest: Guest) -> tuple[int,int]:
    pos=len(guest.log)
    guest.command("gui status", "GUI: selected")
    raw=guest.log[pos:].decode("utf-8",errors="replace")
    m=re.search(r"pointer=(\d+),(\d+)",raw)
    if not m:
        raise AssertionError("Missing pointer coordinates: "+raw)
    return int(m.group(1)),int(m.group(2))


def move_to(guest: Guest, monitor: socket.socket, target_x: int, target_y: int) -> None:
    # Small steps avoid legacy PS/2 8-bit delta overflow and track real guest motion.
    for _ in range(24):
        x,y=status_pointer(guest)
        dx=target_x-x;dy=target_y-y
        if abs(dx)<8 and abs(dy)<8:
            return
        cmd(monitor, f"mouse_move {max(-65,min(65,dx))} {max(-65,min(65,dy))}")
    x,y=status_pointer(guest)
    raise AssertionError(f"PS/2 cursor didn't reach dock: {(x,y)} vs {(target_x,target_y)}")


def verify(iso: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="samen-gui-") as temp:
        root=Path(temp)
        disk=root/"data.img"
        subprocess.run([sys.executable,str(ROOT/"tools"/"mkfs.py"),"--output",str(disk)],check=True)
        monitor_path=root/"hmp.sock"
        guest=Guest(iso,disk,monitor_socket=monitor_path)
        try:
            boot=guest.log.decode("utf8",errors="replace")
            if "GUI: ready framebuffer=" not in boot:
                raise AssertionError("Kernel did not initialize framebuffer: "+boot[-3000:])
            if "GUI: PS/2 mouse ready" not in boot:
                raise AssertionError("PS/2 mouse was not initialized: "+boot[-3000:])
            with connect_monitor(monitor_path) as monitor:
                dump=root/"frame.ppm"
                cmd(monitor,f"screendump {dump}")
                end=time.monotonic()+5
                while not dump.exists() and time.monotonic()<end: time.sleep(0.04)
                w,h,rgb=ppm_rgb(dump)
                if len(rgb)!=w*h*3 or w<640 or h<480:
                    raise AssertionError(f"Invalid framebuffer screenshot {(w,h)}")
                # Pixel (0,0) is pure source Figma art, not the cursor.
                if rgb[:3] != bytes([0,53,143]):
                    raise AssertionError(f"Framebuffer pixel != exact source: {rgb[:3]!r}")
                print(f"PASS: QEMU rendered exact Figma top-left pixel at {w}x{h}")
                move_to(guest,monitor,round(w*356/692),round(h*445/498))
                start=len(guest.log)
                cmd(monitor,"mouse_button 1")
                cmd(monitor,"mouse_button 0")
                guest.wait_for("GUI: selected Files",start=start,timeout=8)
                print("PASS: physical emulated PS/2 mouse click selects Files dock icon")
            guest.exit()
        finally:
            guest.close()


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("--iso",type=Path,required=True)
    args=parser.parse_args()
    verify(args.iso.resolve())
