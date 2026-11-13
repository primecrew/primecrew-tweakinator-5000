#!/usr/bin/env python3
"""Write a PNG from raw RGBA (width * height * 4 bytes). Used by the Linux UI preview."""
import struct, sys, zlib

def main():
    path, w, h, rawp = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
    data = open(rawp, "rb").read()
    expect = w * h * 4
    if len(data) != expect:
        raise SystemExit(f"raw size {len(data)} != {expect}")
    rows = b"".join(b"\x00" + data[i * w * 4:(i + 1) * w * 4] for i in range(h))

    def chunk(tag, body):
        t = tag.encode()
        return struct.pack(">I", len(body)) + t + body + struct.pack(">I", zlib.crc32(t + body) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)
    open(path, "wb").write(b"\x89PNG\r\n\x1a\n" + chunk("IHDR", ihdr) + chunk("IDAT", zlib.compress(rows, 9)) + chunk("IEND", b""))

if __name__ == "__main__":
    main()
