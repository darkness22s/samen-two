#!/usr/bin/env python3
"""Headless QEMU regression: high-res wallpaper, native dock, cursor-only updates and windows."""
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


def logical_xy(w: int,h: int,x: int,y: int) -> tuple[int,int]:
    scale=min(w/692,h/498)
    x0=int((w-692*scale)*0.5)
    y0=int((h-498*scale)*0.5)
    return (int(x0+x*scale),int(y0+y*scale))


def render_stats(guest: Guest) -> tuple[int,int]:
    start=len(guest.log)
    guest.command('gui render stats','GUI: redraws=')
    m=re.search(r'GUI: redraws=(\d+) cursor_updates=(\d+)',guest.log[start:].decode('utf8',errors='replace'))
    if not m:raise AssertionError('Missing renderer metrics')
    return tuple(map(int,m.groups()))


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
                if "native 2048x1152 wallpaper, double buffered, separate real dock" not in boot:
                    raise AssertionError("New native renderer not initialized")
                if w<1600 or h<900:
                    raise AssertionError(f"Requested high-resolution VGA mode unavailable: {(w,h)}")
                if not (rgb[0]<30 and 25<rgb[1]<90 and 95<rgb[2]<200):
                    raise AssertionError(f"Unexpected high-resolution wallpaper pixel: {rgb[:3]!r}")
                def pixel(x,y):
                    sx,sy=logical_xy(w,h,x,y)
                    i=(sy*w+sx)*3
                    return rgb[i:i+3]
                if pixel(309,445) != bytes([46,140,231]):
                    raise AssertionError(f"Dock icon is not procedurally rendered Chrome: {pixel(309,445)!r}")
                print(f"PASS: high-resolution {(w,h)} wallpaper with independent native dock")
                initial=render_stats(guest)
                move_to(guest,monitor,*logical_xy(w,h,309,445))
                after=render_stats(guest)
                if after[0]!=initial[0] or after[1]<=initial[1]:
                    raise AssertionError(f"Cursor motion incorrectly redrew whole screen: {initial} -> {after}")
                print("PASS: cursor motion restores only cursor rectangles, no full redraw")
                move_to(guest,monitor,*logical_xy(w,h,356,445))
                start=len(guest.log)
                cmd(monitor,"mouse_button 1")
                cmd(monitor,"mouse_button 0")
                guest.wait_for("GUI: selected Files",start=start,timeout=8)
                print("PASS: physical emulated PS/2 mouse click selects Files dock icon")
                guest.command("gui windows", "GUI: Files open=true minimized=false")
                # A previously blue desktop pixel should be inside the new light window.
                window_dump=root/"window.ppm"
                cmd(monitor,f"screendump {window_dump}")
                end=time.monotonic()+5
                while not window_dump.exists() and time.monotonic()<end: time.sleep(0.04)
                _,_,window_rgb=ppm_rgb(window_dump)
                sx,sy=logical_xy(w,h,200,200)
                px=(sy*w+sx)*3
                if min(window_rgb[px:px+3])<175:
                    raise AssertionError("Files window did not render an opaque light surface")
                print("PASS: Files application window draws above unchanged desktop")

                def click_logical(x: int,y: int) -> None:
                    move_to(guest,monitor,*logical_xy(w,h,x,y))
                    cmd(monitor,"mouse_button 1")
                    cmd(monitor,"mouse_button 0")

                # Real filesystem changes through pointer clicks, not mocked UI data.
                click_logical(168,125) # Files toolbar: NEW SAMPLE
                guest.command("ls","Welcome.txt")
                guest.command("cat Welcome.txt","Welcome to Samen OS")
                print("PASS: Files window creates persistent SamenFS1 file")
                click_logical(154,184) # Select first file row
                guest.command("gui windows","GUI: Files open=true")

                # Grab titlebar while holding PS/2 left mouse button and drag.
                guest.command("gui windows","GUI: Files open=true")
                click_title=(203,88)
                move_to(guest,monitor,*logical_xy(w,h,*click_title))
                cmd(monitor,"mouse_button 1")
                cmd(monitor,"mouse_move 38 23")
                cmd(monitor,"mouse_button 0")
                before=len(guest.log)
                guest.command("gui windows","GUI: Files open=true")
                out=guest.log[before:].decode("utf8",errors="replace")
                match=re.search(r"GUI: Files open=true minimized=false maximized=false rect=(\d+),(\d+)",out)
                if not match or int(match.group(1))<=117:
                    raise AssertionError("Window dragging did not update its rectangle: "+out)
                fx,fy=map(int,match.groups())
                print(f"PASS: Files window draggable to {(fx,fy)}")

                # Minimize, then restore the same window via the existing dock.
                click_logical(fx+474-46,fy+13)
                guest.command("gui windows","GUI: Files open=true minimized=true")
                click_logical(356,445)
                guest.command("gui windows","GUI: Files open=true minimized=false")
                # Close button and dock reopen work independently.
                click_logical(fx+474-20,fy+13)
                guest.command("gui windows","GUI: Files open=false")
                print("PASS: minimize, restore and close controls")
                click_logical(356,445)
                guest.command("gui windows","GUI: Files open=true")

                # Settings toggles alter actual in-kernel state.
                click_logical(408,445)
                guest.command("gui settings status","GUI: settings glass=true")
                click_logical(552,195)
                guest.command("gui settings status","GUI: settings glass=false")
                print("PASS: Settings glass toggle accepts mouse clicks")

                # Browser contains honest local Home and Help screens.
                click_logical(307,445)
                guest.command("gui browser status","GUI: browser help=false")
                click_logical(215,120)
                guest.command("gui browser status","GUI: browser help=true")
                print("PASS: browser local navigation changes visible state")

            guest.exit()
        finally:
            guest.close()


if __name__=="__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("--iso",type=Path,required=True)
    args=parser.parse_args()
    verify(args.iso.resolve())
