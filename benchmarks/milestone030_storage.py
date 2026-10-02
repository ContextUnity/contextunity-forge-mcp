#!/usr/bin/env python3
"""Measure finalized SQLite storage and decoded Zstd fact bytes without writing it."""

import argparse
import ctypes
import ctypes.util
import datetime
import json
import pathlib
import sqlite3


class Buffer(ctypes.Structure):
    _fields_ = [("data", ctypes.c_void_p), ("size", ctypes.c_size_t), ("pos", ctypes.c_size_t)]


def fact_sizes(connection):
    zstd = ctypes.CDLL(ctypes.util.find_library("zstd"))
    zstd.ZSTD_createDStream.restype = ctypes.c_void_p
    zstd.ZSTD_initDStream.argtypes = [ctypes.c_void_p]
    zstd.ZSTD_initDStream.restype = ctypes.c_size_t
    zstd.ZSTD_decompressStream.argtypes = [ctypes.c_void_p, ctypes.POINTER(Buffer), ctypes.POINTER(Buffer)]
    zstd.ZSTD_decompressStream.restype = ctypes.c_size_t
    zstd.ZSTD_freeDStream.argtypes = [ctypes.c_void_p]
    zstd.ZSTD_isError.argtypes = [ctypes.c_size_t]
    zstd.ZSTD_getErrorName.argtypes = [ctypes.c_size_t]
    zstd.ZSTD_getErrorName.restype = ctypes.c_char_p
    stream = zstd.ZSTD_createDStream()
    if not stream:
        raise RuntimeError("cannot create Zstd decoder")
    output = ctypes.create_string_buffer(65536)
    compressed = decoded = 0
    try:
        for (blob,) in connection.execute("SELECT facts_blob FROM local_facts ORDER BY path"):
            compressed += len(blob)
            zstd.ZSTD_initDStream(stream)
            source = ctypes.create_string_buffer(blob)
            incoming = Buffer(ctypes.addressof(source), len(blob), 0)
            remaining = 1
            while remaining:
                outgoing = Buffer(ctypes.addressof(output), len(output), 0)
                remaining = zstd.ZSTD_decompressStream(stream, ctypes.byref(outgoing), ctypes.byref(incoming))
                if zstd.ZSTD_isError(remaining):
                    raise RuntimeError(zstd.ZSTD_getErrorName(remaining).decode())
                decoded += outgoing.pos
                if remaining and incoming.pos == incoming.size and not outgoing.pos:
                    raise RuntimeError("truncated Zstd fact frame")
            if incoming.pos != incoming.size:
                raise RuntimeError("unexpected bytes after Zstd fact frame")
    finally:
        zstd.ZSTD_freeDStream(stream)
    return compressed, decoded


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--db", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, default=pathlib.Path("/tmp/m030-storage.json"))
    args = parser.parse_args()
    database = args.db.resolve()
    connection = sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)
    files = connection.execute("SELECT count(*) FROM files").fetchone()[0]
    nodes = connection.execute("SELECT count(*) FROM nodes").fetchone()[0]
    compressed, decoded = fact_sizes(connection)
    tables = [dict(name=row[0], bytes=row[1], payload_bytes=row[2]) for row in connection.execute(
        "SELECT name,sum(pgsize),sum(payload) FROM dbstat GROUP BY name ORDER BY sum(pgsize) DESC,name"
    )]
    report = {
        "measured_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "db": str(database),
        "metadata": dict(connection.execute("SELECT key,value FROM metadata WHERE key IN('schema_version','index_semantics_version','corpus_hash','output_root')")),
        "database_bytes": database.stat().st_size,
        "database_mib": database.stat().st_size / 1048576,
        "files": files,
        "nodes": nodes,
        "kib_per_file": database.stat().st_size / 1024 / files,
        "kib_per_node": database.stat().st_size / 1024 / nodes,
        "page_size": connection.execute("PRAGMA page_size").fetchone()[0],
        "freelist_pages": connection.execute("PRAGMA freelist_count").fetchone()[0],
        "average_node_details_bytes": connection.execute("SELECT avg(length(CAST(details AS BLOB))) FROM nodes").fetchone()[0],
        "compressed_fact_bytes": compressed,
        "decoded_fact_bytes": decoded,
        "fact_compression_ratio": decoded / compressed,
        "compressed_fact_kib_per_file": compressed / 1024 / files,
        "tables": tables,
    }
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({key: value for key, value in report.items() if key != "tables"}, indent=2))


if __name__ == "__main__":
    main()
