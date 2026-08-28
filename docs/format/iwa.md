# IWA streams — Snappy framing and protobuf layout

A `.pages`/`.numbers`/`.key` document (iWork '13+) stores its object database as
`.iwa` streams inside `Index.zip` (see [container.md](container.md)). Each `.iwa`
stream is a sequence of Snappy-compressed blocks wrapping a protobuf message
stream (see [objects.md](objects.md)).

## Block framing

An `.iwa` stream is a concatenation of blocks:

```
+--------+----------------------------+
| 4-byte | Snappy-compressed payload  |
| header | of `length` bytes          |
+--------+----------------------------+
```

- **Byte 0 is always `0x00`** (a chunk-type byte). All four known implementations
  either require or assume it:
  `[parser: keynote-parser@56a4d3b0] keynote_parser/codec.py:194-197` raises
  "IWA chunk does not start with 0x00" otherwise;
  `[parser: iwork@02c26ebf] index/index.go:184-186` errors on non-zero;
  `[parser: litchi@92293640] crates/litchi-iwa/src/snappy.rs:43-49` errors on non-zero
  chunk type.
- **Bytes 1–3 are the compressed length, 24-bit little-endian.**
  `[parser: iwork@02c26ebf] index/index.go:188` (`l := data[1] | data[2]<<8 | data[3]<<16`),
  `[parser: keynote-parser@56a4d3b0] codec.py:199-201` (`struct.unpack_from("<I", header[1:] + b"\x00")`),
  `[parser: litchi@92293640] snappy.rs:52` (`u32::from_le_bytes([header[1], header[2], header[3], 0])`),
  and `[parser: libetonyek (consult-only)] src/lib/IWASnappyStream.cpp` `uncompress()`:
  `readU16` + `blockLength += 65536 * readU8()` (same 24-bit LE total).
- **The header carries NO uncompressed size.** The uncompressed length is the
  leading varint inside the raw Snappy block itself (that is how the Snappy raw
  format works); none of the four implementations reads an uncompressed size
  from the header.
- **The payload is a raw Snappy block — NOT the Snappy "framing format"**:
  no stream-identifier chunk, no CRC-32C, per-block raw compress.
  `[parser: litchi@92293640] crates/litchi-iwa/src/snappy.rs:1-26` (module doc
  states exactly this), `[parser: keynote-parser@56a4d3b0] codec.py:204-210`
  calls `snappy.uncompress()` on the whole payload.
- **Blocks larger than 64 KiB occur.** libetonyek documents a real-world
  `06 00 01` header = compressed length 0x010006 (65542)
  `[parser: libetonyek (consult-only)] IWASnappyStream.cpp` comment. keynote-parser's
  writer splits the *uncompressed* stream into 65536-byte pieces before compressing
  each `[parser: keynote-parser@56a4d3b0] codec.py:232-243`, so compressed sizes
  cluster near but can exceed 64 KiB.

> **Header folklore warning.** Widely-copied writeups (and our own early AGENTS.md
> primer) describe the header as "u16 LE compressed size + u16 *big-endian*
> uncompressed size". That reading is **wrong**: it is incompatible with
> keynote-parser's round-tripping writer and with the byte-0-must-be-0 check.
> Treat the verified form as: `00` + u24 LE compressed length. `[inferred: the
> four implementations above agree and keynote-parser round-trips real files;
> fixture verification still pending]`.

## Implementation notes for pnk (Rust)

- Decompress with a raw Snappy decoder per block, concatenating output; keep
  going until the stream ends `[parser: litchi@92293640] snappy.rs:27-88`.
- keynote-parser tolerates a failed Snappy decode by emitting the compressed
  bytes verbatim (treated as already-uncompressed) `[parser: keynote-parser@56a4d3b0]
  codec.py:204-210`. A strict decoder SHOULD fail loudly instead
  `[inferred: pnk wants corruption surfaced, not masked]`.
- After decompression the buffer is a protobuf message stream parsed by
  [objects.md](objects.md); ids are resolved with [registry.md](registry.md).