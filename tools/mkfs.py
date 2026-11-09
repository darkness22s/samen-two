#!/usr/bin/env python3
"""Create a raw, persistent SamenFS1 data disk; refuse accidental overwrites."""
from __future__ import annotations

import argparse
from pathlib import Path
import struct

BLOCK_SIZE = 512
MAGIC = b"SAMENFS1"
DIRECTORY_SECTORS = 16
DATA_START = 1 + DIRECTORY_SECTORS


def create_disk(path: Path, size_mib: int) -> None:
    size = size_mib * 1024 * 1024
    if size % BLOCK_SIZE or size // BLOCK_SIZE <= DATA_START:
        raise ValueError("disk must have at least 18 sectors")
    if size // BLOCK_SIZE >= 2**28:
        raise ValueError("ATA LBA28 disk size exceeded")
    path.parent.mkdir(parents=True, exist_ok=True)
    header = bytearray(BLOCK_SIZE)
    header[:8] = MAGIC
    struct.pack_into("<IIII", header, 8, 1, size // BLOCK_SIZE, DIRECTORY_SECTORS, DATA_START)
    try:
        with path.open("xb") as disk:
            disk.truncate(size)
            disk.seek(0)
            disk.write(header)
            disk.flush()
    except FileExistsError as exc:
        raise SystemExit(f"Refusing to overwrite existing data disk: {path}") from exc
    print(f"Created {path} ({size_mib} MiB, SamenFS1)")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--size-mib", type=int, default=16)
    args = parser.parse_args()
    create_disk(args.output, args.size_mib)
