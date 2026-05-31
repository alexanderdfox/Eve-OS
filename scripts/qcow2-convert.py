#!/usr/bin/env python3
# Convert a raw disk image to qcow2 (no qemu-img required).
# Based on QEMU scripts/qcow2-to-stdout.py (GPL-2.0-or-later); writes to -o FILE instead of stdout.
# Copyright (C) 2024 Igalia, S.L. — adapted for Eve OS repo packaging.

import argparse
import errno
import math
import os
import signal
import struct
import subprocess
import sys
import tempfile
import time
from contextlib import contextmanager

QCOW2_DEFAULT_CLUSTER_SIZE = 65536
QCOW2_DEFAULT_REFCOUNT_BITS = 16
QCOW2_FEATURE_NAME_TABLE = 0x6803F857
QCOW2_DATA_FILE_NAME_STRING = 0x44415441
QCOW2_V3_HEADER_LENGTH = 112
QCOW2_INCOMPAT_DATA_FILE_BIT = 2
QCOW2_AUTOCLEAR_DATA_FILE_RAW_BIT = 1
QCOW_OFLAG_COPIED = 1 << 63
QEMU_STORAGE_DAEMON = "qemu-storage-daemon"


def bitmap_set(bitmap, idx):
    bitmap[idx // 8] |= 1 << (idx % 8)


def bitmap_is_set(bitmap, idx):
    return (bitmap[idx // 8] & (1 << (idx % 8))) != 0


def bitmap_iterator(bitmap, length):
    for idx in range(length):
        if bitmap_is_set(bitmap, idx):
            yield idx


def align_up(num, d):
    return d * math.ceil(num / d)


def clusters_with_data(fd, cluster_size):
    data_to = 0
    while True:
        try:
            data_from = os.lseek(fd, data_to, os.SEEK_DATA)
            data_to = align_up(os.lseek(fd, data_from, os.SEEK_HOLE), cluster_size)
            for idx in range(data_from // cluster_size, data_to // cluster_size):
                yield idx
        except OSError as err:
            if err.errno == errno.ENXIO:
                break
            raise err


@contextmanager
def get_input_as_raw_file(input_file, input_format):
    if input_format == "raw":
        yield input_file
        return
    try:
        temp_dir = tempfile.mkdtemp()
        pid_file = os.path.join(temp_dir, "pid")
        raw_file = os.path.join(temp_dir, "raw")
        open(raw_file, "wb").close()
        ret = subprocess.run(
            [
                QEMU_STORAGE_DAEMON,
                "--daemonize",
                "--pidfile",
                pid_file,
                "--blockdev",
                f"driver=file,node-name=file0,driver=file,filename={input_file},read-only=on",
                "--blockdev",
                f"driver={input_format},node-name=disk0,file=file0,read-only=on",
                "--export",
                f"type=fuse,id=export0,node-name=disk0,mountpoint={raw_file},writable=off",
            ],
            capture_output=True,
        )
        if ret.returncode != 0:
            sys.exit(
                "[Error] Could not start the qemu-storage-daemon:\n"
                + ret.stderr.decode().rstrip("\n")
            )
        yield raw_file
    finally:
        if os.path.exists(pid_file):
            with open(pid_file, "r", encoding="ascii") as f:
                pid = int(f.readline())
            os.kill(pid, signal.SIGTERM)
            while os.path.exists(pid_file):
                time.sleep(0.1)
        if os.path.exists(raw_file):
            os.unlink(raw_file)
        if os.path.exists(temp_dir):
            os.rmdir(temp_dir)


def write_features(cluster, offset, data_file_name):
    if data_file_name is not None:
        encoded_name = data_file_name.encode("utf-8")
        padded_name_len = align_up(len(encoded_name), 8)
        struct.pack_into(
            f">II{padded_name_len}s",
            cluster,
            offset,
            QCOW2_DATA_FILE_NAME_STRING,
            len(encoded_name),
            encoded_name,
        )
        offset += 8 + padded_name_len

    qcow2_features = [
        (0, 0, "dirty bit"),
        (0, 1, "corrupt bit"),
        (0, 2, "external data file"),
        (0, 3, "compression type"),
        (0, 4, "extended L2 entries"),
        (1, 0, "lazy refcounts"),
        (2, 0, "bitmaps"),
        (2, 1, "raw external data"),
    ]
    struct.pack_into(">I", cluster, offset, QCOW2_FEATURE_NAME_TABLE)
    struct.pack_into(">I", cluster, offset + 4, len(qcow2_features) * 48)
    offset += 8
    for feature_type, feature_bit, feature_name in qcow2_features:
        struct.pack_into(
            ">BB46s",
            cluster,
            offset,
            feature_type,
            feature_bit,
            feature_name.encode("ascii"),
        )
        offset += 48


def write_qcow2_content(out, input_file, cluster_size, refcount_bits, data_file_name, data_file_raw):
    l1_entries_per_table = cluster_size // 8
    l2_entries_per_table = cluster_size // 8
    refcounts_per_table = cluster_size // 8
    refcounts_per_block = cluster_size * 8 // refcount_bits

    disk_size = align_up(os.path.getsize(input_file), 512)
    total_data_clusters = math.ceil(disk_size / cluster_size)
    l1_entries = math.ceil(total_data_clusters / l2_entries_per_table)
    allocated_l1_tables = math.ceil(l1_entries / l1_entries_per_table)

    if (l1_entries * 8) > (32 * 1024 * 1024):
        sys.exit("[Error] The image size is too large. Try using a larger cluster size.")

    l1_bitmap = bytearray(allocated_l1_tables * l1_entries_per_table // 8)
    l2_bitmap = bytearray(l1_entries * l2_entries_per_table // 8)
    allocated_l2_tables = 0
    allocated_data_clusters = 0

    if data_file_raw:
        allocated_l2_tables = l1_entries
        for idx in range(l1_entries):
            bitmap_set(l1_bitmap, idx)
        for idx in range(total_data_clusters):
            bitmap_set(l2_bitmap, idx)
    else:
        fd = os.open(input_file, os.O_RDONLY)
        zero_cluster = bytes(cluster_size)
        for idx in clusters_with_data(fd, cluster_size):
            cluster = os.pread(fd, cluster_size, cluster_size * idx)
            if len(cluster) < cluster_size:
                cluster += bytes(cluster_size - len(cluster))
            if cluster != zero_cluster:
                bitmap_set(l2_bitmap, idx)
                allocated_data_clusters += 1
                l1_idx = math.floor(idx / l2_entries_per_table)
                if not bitmap_is_set(l1_bitmap, l1_idx):
                    bitmap_set(l1_bitmap, l1_idx)
                    allocated_l2_tables += 1

    total_allocated_clusters = 1 + allocated_l1_tables + allocated_l2_tables
    if data_file_name is None:
        total_allocated_clusters += allocated_data_clusters

    allocated_refcount_blocks = math.ceil(total_allocated_clusters / refcounts_per_block)
    allocated_refcount_tables = math.ceil(allocated_refcount_blocks / refcounts_per_table)

    while True:
        new_total_allocated_clusters = (
            total_allocated_clusters + allocated_refcount_tables + allocated_refcount_blocks
        )
        new_allocated_refcount_blocks = math.ceil(
            new_total_allocated_clusters / refcounts_per_block
        )
        if new_allocated_refcount_blocks > allocated_refcount_blocks:
            allocated_refcount_blocks = new_allocated_refcount_blocks
            allocated_refcount_tables = math.ceil(
                allocated_refcount_blocks / refcounts_per_table
            )
        else:
            break

    total_allocated_clusters += allocated_refcount_tables + allocated_refcount_blocks

    current_cluster_idx = 1
    refcount_table_offset = current_cluster_idx * cluster_size
    current_cluster_idx += allocated_refcount_tables
    refcount_block_offset = current_cluster_idx * cluster_size
    current_cluster_idx += allocated_refcount_blocks
    l1_table_offset = current_cluster_idx * cluster_size
    current_cluster_idx += allocated_l1_tables
    l2_table_offset = current_cluster_idx * cluster_size
    current_cluster_idx += allocated_l2_tables
    data_clusters_offset = current_cluster_idx * cluster_size

    if allocated_l1_tables == 0:
        l1_table_offset = 0

    hdr_cluster_bits = int(math.log2(cluster_size))
    hdr_refcount_bits = int(math.log2(refcount_bits))
    hdr_length = QCOW2_V3_HEADER_LENGTH
    hdr_incompat_features = 0
    if data_file_name is not None:
        hdr_incompat_features |= 1 << QCOW2_INCOMPAT_DATA_FILE_BIT
    hdr_autoclear_features = 0
    if data_file_raw:
        hdr_autoclear_features |= 1 << QCOW2_AUTOCLEAR_DATA_FILE_RAW_BIT

    cluster = bytearray(cluster_size)
    struct.pack_into(
        ">4sIQIIQIIQQIIQQQQII",
        cluster,
        0,
        b"QFI\xfb",
        3,
        0,
        0,
        hdr_cluster_bits,
        disk_size,
        0,
        l1_entries,
        l1_table_offset,
        refcount_table_offset,
        allocated_refcount_tables,
        0,
        0,
        hdr_incompat_features,
        0,
        hdr_autoclear_features,
        hdr_refcount_bits,
        hdr_length,
    )
    write_features(cluster, hdr_length, data_file_name)
    out.write(cluster)

    cur_offset = refcount_block_offset
    remaining_refcount_table_entries = allocated_refcount_blocks
    while remaining_refcount_table_entries > 0:
        cluster = bytearray(cluster_size)
        to_write = min(remaining_refcount_table_entries, refcounts_per_table)
        remaining_refcount_table_entries -= to_write
        for idx in range(to_write):
            struct.pack_into(">Q", cluster, idx * 8, cur_offset)
            cur_offset += cluster_size
        out.write(cluster)

    remaining_refcount_block_entries = total_allocated_clusters
    for _tbl in range(allocated_refcount_blocks):
        cluster = bytearray(cluster_size)
        to_write = min(remaining_refcount_block_entries, refcounts_per_block)
        remaining_refcount_block_entries -= to_write
        for idx in range(to_write):
            if refcount_bits == 64:
                struct.pack_into(">Q", cluster, idx * 8, 1)
            elif refcount_bits == 32:
                struct.pack_into(">L", cluster, idx * 4, 1)
            elif refcount_bits == 16:
                struct.pack_into(">H", cluster, idx * 2, 1)
            elif refcount_bits == 8:
                cluster[idx] = 1
            elif refcount_bits == 4:
                cluster[idx // 2] |= 1 << ((idx % 2) * 4)
            elif refcount_bits == 2:
                cluster[idx // 4] |= 1 << ((idx % 4) * 2)
            elif refcount_bits == 1:
                cluster[idx // 8] |= 1 << (idx % 8)
        out.write(cluster)

    cur_offset = l2_table_offset
    for tbl in range(allocated_l1_tables):
        cluster = bytearray(cluster_size)
        for idx in range(l1_entries_per_table):
            l1_idx = tbl * l1_entries_per_table + idx
            if bitmap_is_set(l1_bitmap, l1_idx):
                struct.pack_into(">Q", cluster, idx * 8, cur_offset | QCOW_OFLAG_COPIED)
                cur_offset += cluster_size
        out.write(cluster)

    cur_offset = data_clusters_offset
    for tbl in range(l1_entries):
        if bitmap_is_set(l1_bitmap, tbl):
            cluster = bytearray(cluster_size)
            for idx in range(l2_entries_per_table):
                l2_idx = tbl * l2_entries_per_table + idx
                if bitmap_is_set(l2_bitmap, l2_idx):
                    if data_file_name is None:
                        struct.pack_into(">Q", cluster, idx * 8, cur_offset | QCOW_OFLAG_COPIED)
                        cur_offset += cluster_size
                    else:
                        struct.pack_into(
                            ">Q",
                            cluster,
                            idx * 8,
                            (l2_idx * cluster_size) | QCOW_OFLAG_COPIED,
                        )
            out.write(cluster)

    if data_file_name is None:
        for idx in bitmap_iterator(l2_bitmap, total_data_clusters):
            cluster = os.pread(fd, cluster_size, cluster_size * idx)
            if len(cluster) < cluster_size:
                cluster += bytes(cluster_size - len(cluster))
            out.write(cluster)
        os.close(fd)


def main():
    parser = argparse.ArgumentParser(description="Convert a raw disk image to qcow2.")
    parser.add_argument("input_file", help="raw input image")
    parser.add_argument("-o", "--output", required=True, help="output .qcow2 path")
    parser.add_argument(
        "-f",
        dest="input_format",
        default="raw",
        help="input format (default: raw)",
    )
    parser.add_argument(
        "-c",
        dest="cluster_size",
        default=QCOW2_DEFAULT_CLUSTER_SIZE,
        type=int,
        choices=[1 << x for x in range(9, 22)],
    )
    parser.add_argument(
        "-r",
        dest="refcount_bits",
        default=QCOW2_DEFAULT_REFCOUNT_BITS,
        type=int,
        choices=[1 << x for x in range(7)],
    )
    args = parser.parse_args()

    if not os.path.isfile(args.input_file):
        sys.exit(f"[Error] {args.input_file} does not exist or is not a regular file.")

    with get_input_as_raw_file(args.input_file, args.input_format) as raw_file:
        with open(args.output, "wb") as out:
            write_qcow2_content(
                out,
                raw_file,
                args.cluster_size,
                args.refcount_bits,
                None,
                False,
            )


if __name__ == "__main__":
    main()
