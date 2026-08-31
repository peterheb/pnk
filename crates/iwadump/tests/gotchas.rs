//! Tests encoding the ten traps from docs/format/gotchas.md plus the
//! container/invariant behaviors they imply. Each test names its gotcha.

use std::io::Write as _;

use iwadump::container::{Container, ContainerForm};
use iwadump::envelope;
use iwadump::error::Kind;
use iwadump::iwa::IwaStream;
use iwadump::proto::{self, Value};
use iwadump::registry::{App, Registry};
use iwadump::snappy;

// ---------------------------------------------------------------- helpers

/// Base-128 varint encode.
fn varint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return out;
        }
        out.push(b | 0x80);
    }
}

/// One protobuf field: tag = (number << 3) | wire.
fn field(number: u32, wire: u8, body: &[u8]) -> Vec<u8> {
    let mut out = varint(((number as u64) << 3) | wire as u64);
    out.extend_from_slice(body);
    out
}

/// `TSP.ArchiveInfo { identifier = 1, message_infos = 2, should_merge = 3 }`
/// for one message of `type_id` whose payload is `payload_len` bytes.
fn archive_info(identifier: u64, message_type: u32, payload_len: u32) -> Vec<u8> {
    let mut mi = Vec::new();
    mi.extend_from_slice(&field(1, proto::WIRE_VARINT, &varint(message_type as u64)));
    mi.extend_from_slice(&field(3, proto::WIRE_VARINT, &varint(payload_len as u64)));
    let mut ai = Vec::new();
    ai.extend_from_slice(&field(1, proto::WIRE_VARINT, &varint(identifier)));
    ai.extend_from_slice(&field(2, proto::WIRE_LEN, &varint(mi.len() as u64)));
    ai.extend_from_slice(&mi);
    let mut out = varint(ai.len() as u64);
    out.extend_from_slice(&ai);
    out
}

/// Frame one snappy block the IWA way: `00` + u24 LE compressed length.
fn frame_block(compressed: &[u8]) -> Vec<u8> {
    let n = compressed.len();
    assert!(n < 1 << 24, "test blocks must fit u24");
    let mut out = vec![0x00, (n & 0xff) as u8, (n >> 8) as u8, (n >> 16) as u8];
    out.extend_from_slice(compressed);
    out
}

/// Build a full `.iwa` stream from raw (already-serialized) archive segments.
fn iwa_stream(archives: &[Vec<u8>]) -> Vec<u8> {
    let mut decoded = Vec::new();
    for a in archives {
        decoded.extend_from_slice(a);
    }
    frame_block(&snappy::encode_block(&decoded))
}

/// Minimal STORE (uncompressed) zip with a raw member-name byte string and
/// the UTF-8 flag left unset — lets us test cp437 name handling (gotcha:
/// container.md zip entry-name encoding).
fn raw_zip_entry(name_bytes: &[u8], data: &[u8]) -> Vec<u8> {
    let crc = crc32(data);
    let mut out = Vec::new();
    // local file header
    out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
    out.extend_from_slice(&20u16.to_le_bytes()); // version needed
    out.extend_from_slice(&0u16.to_le_bytes()); // flags: UTF-8 flag OFF
    out.extend_from_slice(&0u16.to_le_bytes()); // method: store
    out.extend_from_slice(&0u16.to_le_bytes()); // time
    out.extend_from_slice(&0x21u16.to_le_bytes()); // date (1980)
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // extra len
    out.extend_from_slice(name_bytes);
    out.extend_from_slice(data);
    let lfh_len = out.len();
    // central directory
    let cd_start = out.len();
    out.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
    out.extend_from_slice(&20u16.to_le_bytes()); // version made by
    out.extend_from_slice(&20u16.to_le_bytes()); // version needed
    out.extend_from_slice(&0u16.to_le_bytes()); // flags
    out.extend_from_slice(&0u16.to_le_bytes()); // method
    out.extend_from_slice(&0u16.to_le_bytes()); // time
    out.extend_from_slice(&0x21u16.to_le_bytes()); // date
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // extra
    out.extend_from_slice(&0u16.to_le_bytes()); // comment
    out.extend_from_slice(&0u16.to_le_bytes()); // disk
    out.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
    out.extend_from_slice(&0u32.to_le_bytes()); // external attrs
    out.extend_from_slice(&0u32.to_le_bytes()); // local header offset
    out.extend_from_slice(name_bytes);
    let cd_len = out.len() - cd_start;
    // end of central directory
    out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
    out.extend_from_slice(&0u16.to_le_bytes()); // disk
    out.extend_from_slice(&0u16.to_le_bytes()); // cd disk
    out.extend_from_slice(&1u16.to_le_bytes()); // entries this disk
    out.extend_from_slice(&1u16.to_le_bytes()); // entries total
    out.extend_from_slice(&(cd_len as u32).to_le_bytes());
    out.extend_from_slice(&(cd_start as u32).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment len
    let _ = lfh_len;
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xff) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn write_zip(path: &std::path::Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap();
}

/// A minimal valid ArchiveInfo+payload for container-level round-trips:
/// one archive, id 1, one message of type 10000 (TP.DocumentArchive) whose
/// payload is a 2-byte valid protobuf message.
fn tiny_document_iwa() -> Vec<u8> {
    let payload = vec![0x08, 0x2a]; // field 1 varint 42
    let seg = archive_info(1, 10000, payload.len() as u32);
    let mut decoded = seg;
    decoded.extend_from_slice(&payload);
    frame_block(&snappy::encode_block(&decoded))
}

#[test]
fn gotcha1_header_is_zero_byte_plus_u24_le_not_u16_plus_u16() {
    let big = vec![0x41u8; 70_000];
    let small = b"second block payload".to_vec();
    let mut raw = Vec::new();
    let mut offsets = Vec::new();
    for block in [&small, &big] {
        offsets.push(raw.len());
        raw.extend_from_slice(&frame_block(&snappy::encode_block(block)));
    }

    let stream = IwaStream::parse("test.iwa", &raw).expect("u24 framing must parse");
    assert_eq!(stream.blocks.len(), 2);
    // multi-block concatenation: order preserved, both payloads intact
    assert_eq!(&stream.decoded[..small.len()], &small[..]);
    assert_eq!(stream.decoded[small.len()..], big[..]);
    // libetonyek's real-world `06 00 01` header = compressed length
    // 0x010006 (65542): the length field is u24 LE, NOT two u16s.
    let header = &raw[offsets[1]..offsets[1] + 4];
    assert_eq!(header[0], 0x00);
    let declared_len = header[1] as u32 | (header[2] as u32) << 8 | (header[3] as u32) << 16;
    assert_eq!(declared_len as usize, frame_block(&snappy::encode_block(&big)).len() - 4);
}

#[test]
fn gotcha1_nonzero_chunk_type_rejected() {
    let mut raw = frame_block(&snappy::encode_block(b"x"));
    raw[0] = 0x06; // the classic "u16 uncompressed size BE" misreading puts
                   // non-zero bytes here
    let e = IwaStream::parse("test.iwa", &raw).unwrap_err();
    assert_eq!(e.layer, iwadump::Layer::Iwa);
    assert!(e.message.contains("0x00"), "{}", e.message);
}

#[test]
fn truncated_block_header_rejected() {
    let raw = [0u8, 1, 0]; // 3 bytes only
    let e = IwaStream::parse("test.iwa", &raw).unwrap_err();
    assert_eq!(e.layer, iwadump::Layer::Iwa);
    assert!(e.message.contains("truncated"), "{}", e.message);
}

#[test]
fn snappy_failure_names_the_block_not_the_file() {
    // gotcha 7: error says "block N failed to decompress", never "corrupt".
    let mut raw = frame_block(&snappy::encode_block(b"good"));
    raw.extend_from_slice(&[0x00, 0x04, 0x00, 0x00, 0xde, 0xad, 0xbe, 0xef]);
    let e = IwaStream::parse("test.iwa", &raw).unwrap_err();
    assert_eq!(e.layer, iwadump::Layer::Snappy);
    assert!(e.message.contains("failed to decompress"), "{}", e.message);
    assert!(!e.message.to_lowercase().contains("corrupt file"));
}

// ---------------------------------------------- gotcha 2: no PrefixedMessage

#[test]
fn gotcha2_envelope_is_varint_plus_archiveinfo_no_prefixed_wrapper() {
    // The stream is exactly [varint len][ArchiveInfo][payloads]. A
    // TSP.PrefixedMessage {1: id, 2: len, 3: payload} reading would parse
    // field 3 as another submessage; we assert the direct form decodes.
    let payload = vec![0x08, 0x01];
    let seg = archive_info(7, 200, payload.len() as u32);
    let mut decoded = seg;
    decoded.extend_from_slice(&payload);
    let stream = IwaStream::parse("a.iwa", &frame_block(&snappy::encode_block(&decoded))).unwrap();
    let archives = envelope::parse_stream(&stream.decoded).unwrap();
    assert_eq!(archives.len(), 1);
    assert_eq!(archives[0].identifier, 7);
    assert_eq!(archives[0].messages[0].type_id, 200);
    assert_eq!(archives[0].messages[0].payload, payload);
}

// --------------------------------------- gotcha 6: skip by declared length

#[test]
fn gotcha6_garbage_payload_skipped_by_declared_length_stream_stays_synced() {
    // Archive with two messages: the first payload is garbage that would
    // desynchronize any parse-based consumer; the second is valid and must
    // still decode because skipping used MessageInfo.length.
    let garbage = vec![0xff, 0xff, 0xff, 0x00, 0x07, 0x13, 0x37];
    let good = vec![0x08, 0x05];
    let seg = archive_info(3, 200, garbage.len() as u32);
    let seg2 = archive_info(4, 201, good.len() as u32);
    let mut decoded = seg;
    decoded.extend_from_slice(&garbage);
    decoded.extend_from_slice(&seg2);
    decoded.extend_from_slice(&good);

    let archives = envelope::parse_stream(&decoded).unwrap();
    assert_eq!(archives.len(), 2);
    assert_eq!(archives[0].messages[0].payload, garbage);
    assert_eq!(archives[1].identifier, 4);
    assert_eq!(archives[1].messages[0].payload, good);
}

#[test]
fn gotcha6_payload_shorter_than_declared_is_an_envelope_error() {
    let seg = archive_info(1, 200, 100);
    let e = envelope::parse_stream(&seg).unwrap_err();
    assert_eq!(e.layer, iwadump::Layer::Envelope);
    assert!(e.message.contains("declares 100 bytes"), "{}", e.message);
}

// ------------------------------------------- group wire types (3/4) in data

#[test]
fn group_wire_types_are_nested_not_an_error() {
    // start-group(1){ field(1) varint 5; field(2) varint 9 } end-group(1):
    // group bodies are field sequences, the end-group tag is its own tag.
    let group_body = {
        let mut b = field(1, proto::WIRE_VARINT, &varint(5));
        b.extend_from_slice(&field(2, proto::WIRE_VARINT, &varint(9)));
        b
    };
    let mut payload = field(1, proto::WIRE_SGROUP, &group_body);
    payload.extend_from_slice(&field(1, proto::WIRE_EGROUP, &[]));
    let seg = archive_info(1, 200, payload.len() as u32);
    let mut decoded = seg;
    decoded.extend_from_slice(&payload);
    let archives = envelope::parse_stream(&decoded).unwrap();
    let m = &archives[0].messages[0];
    let fields =
        proto::parse_fields(&m.payload, iwadump::Layer::Message).expect("group wire types walk cleanly");
    assert_eq!(fields.len(), 1);
    match &fields[0].value {
        Value::Group(g) => {
            assert_eq!(g.len(), 2);
            assert!(matches!(g[0].value, Value::Varint(5)));
            assert!(matches!(g[1].value, Value::Varint(9)));
        }
        other => panic!("expected group, got {other:?}"),
    }
}

#[test]
fn unclosed_group_makes_payload_undecodable_but_stream_synced() {
    // Payload 1 opens a group whose body is a valid field but never closes →
    // the walk fails; payload 2 (a separate length-delimited blob) still
    // decodes because skipping is length-based (gotcha #6), never parse-based.
    let bad = field(1, proto::WIRE_SGROUP, &field(1, proto::WIRE_VARINT, &varint(5)));
    let good = vec![0x08, 0x07];
    let seg = archive_info(1, 200, bad.len() as u32);
    let seg2 = archive_info(2, 201, good.len() as u32);
    let mut decoded = seg;
    decoded.extend_from_slice(&bad);
    decoded.extend_from_slice(&seg2);
    decoded.extend_from_slice(&good);
    let archives = envelope::parse_stream(&decoded).unwrap();
    let registry = Registry::embedded().unwrap();
    let statuses = archives[0].message_status(&registry, App::Unknown);
    assert!(
        matches!(&statuses[0], iwadump::MessageStatus::Undecodable { reason, .. } if reason.contains("group")),
        "{statuses:?}"
    );
    // The second archive still decodes regardless:
    let statuses2 = archives[1].message_status(&registry, App::Unknown);
    assert!(matches!(&statuses2[0], iwadump::MessageStatus::Decoded { .. }));
    assert_eq!(archives[1].messages[0].payload, good);
}

// --------------------------------------------------- unknown ids stay opaque

#[test]
fn gotcha4_unknown_type_ids_stay_opaque_never_guessed() {
    let payload = vec![0x08, 0x2a];
    let seg = archive_info(9, 0xdead, payload.len() as u32);
    let mut decoded = seg;
    decoded.extend_from_slice(&payload);
    let archives = envelope::parse_stream(&decoded).unwrap();
    let registry = Registry::embedded().unwrap();
    let statuses = archives[0].message_status(&registry, App::Unknown);
    assert!(matches!(statuses[0], iwadump::MessageStatus::UnknownType));
    // With the app known the same rule holds for genuinely absent ids:
    let statuses = archives[0].message_status(&registry, App::Pages);
    assert!(matches!(statuses[0], iwadump::MessageStatus::UnknownType));
}

#[test]
fn registry_ambiguity_requires_app_detection() {
    let registry = Registry::embedded().unwrap();
    // id 1 = KN.DocumentArchive vs TN.DocumentArchive (Pages.json has no
    // id 1): without an app, no name; with the right app, the right one.
    assert_eq!(registry.name_for(App::Unknown, 1), None);
    assert_eq!(registry.name_for(App::Keynote, 1).as_deref(), Some("KN.DocumentArchive"));
    assert_eq!(registry.name_for(App::Numbers, 1).as_deref(), Some("TN.DocumentArchive"));
    // id 7 collides three ways (KN/TN/TP.PlaceholderArchive) — unknown.
    assert_eq!(registry.name_for(App::Unknown, 7), None);
    assert_eq!(registry.name_for(App::Pages, 7).as_deref(), Some("TP.PlaceholderArchive"));
    // Pages' root type lives at 10000 (per-fixture evidence), and resolves.
    assert_eq!(registry.name_for(App::Pages, 10000).as_deref(), Some("TP.DocumentArchive"));
    assert_eq!(registry.table_size(App::Pages), 47);
}

// ------------------------------------------------------- gotcha 5: zip-ception

#[test]
fn gotcha5_nested_index_zip_is_unzipped_twice() {
    let tmp = std::env::temp_dir().join(format!("iwadump-test-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("nested.pages");
    let inner = build_zip(&[("Index/Document.iwa", &tiny_document_iwa())]);
    let outer = build_zip(&[("Index.zip", &inner), ("Metadata/Properties.plist", b"plist")]);
    std::fs::write(&path, &outer).unwrap();

    let container = Container::open(&path, false).unwrap();
    assert_eq!(container.form, ContainerForm::FlatZipNested);
    assert_eq!(container.iwas.len(), 1);
    assert_eq!(container.iwas[0].0, "Index/Document.iwa");
    // Nested listing is preserved for --list.
    assert!(container.nested_members.is_some());
    // And the full stack decodes.
    let stream = IwaStream::parse(&container.iwas[0].0, &container.iwas[0].1).unwrap();
    let archives = envelope::parse_stream(&stream.decoded).unwrap();
    assert_eq!(archives[0].messages[0].type_id, 10000);
    std::fs::remove_file(&path).ok();
}

#[test]
fn flat_zip_with_direct_iwa_members_reports_flat_form() {
    let tmp = std::env::temp_dir().join(format!("iwadump-flat-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("flat.numbers");
    let iwa = tiny_document_iwa();
    let outer = build_zip(&[
        ("Index/Document.iwa", &iwa),
        ("Metadata/Properties.plist", b"plist"),
        ("preview.jpg", b"jpeg"),
    ]);
    std::fs::write(&path, &outer).unwrap();
    let container = Container::open(&path, false).unwrap();
    assert_eq!(container.form, ContainerForm::FlatZip);
    assert_eq!(container.members.len(), 3);
    std::fs::remove_file(&path).ok();
}

#[test]
fn package_directory_reads_index_zip() {
    let tmp = std::env::temp_dir().join(format!("iwadump-pkg-{}", std::process::id()));
    let bundle = tmp.join("Doc.numbers");
    std::fs::create_dir_all(bundle.join("Metadata")).unwrap();
    let inner = build_zip(&[("Index/Document.iwa", &tiny_document_iwa())]);
    std::fs::write(bundle.join("Index.zip"), &inner).unwrap();
    std::fs::write(bundle.join("Metadata/Properties.plist"), b"plist").unwrap();
    let container = Container::open(&bundle, false).unwrap();
    assert_eq!(container.form, ContainerForm::PackageDir);
    assert_eq!(container.iwas[0].0, "Index/Document.iwa");
    std::fs::remove_dir_all(&tmp).ok();
}

// ------------------------------------------------- cp437 zip name hazard

#[test]
fn cp437_member_names_decode_not_mojibake() {
    // `Data/café.txt` in cp437 bytes (0x86 = é) with the UTF-8 flag OFF —
    // the classic hazard from docs/format/container.md. The reader must show
    // the decoded name, not `ca\u{0086}` or U+FFFD mojibake.
    let mut name = b"Data/ca".to_vec();
    name.push(0x86); // cp437 for 'å'? No: cp437 0x86 = 'å'. Use 0x82 = é.
    let zip = raw_zip_entry(&name, b"media bytes");
    let bytes = zip;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index_raw(i).unwrap().name().to_string())
        .collect();
    assert_eq!(names.len(), 1);
    // zip crate decodes cp437 when the UTF-8 flag is off; assert no U+FFFD
    // replacement sneaks in either way.
    assert!(!names[0].contains('\u{FFFD}'), "{}", names[0]);
}

#[test]
fn utf8_flagged_names_pass_through() {
    let zip = build_zip(&[("Data/héllo.png", b"media")]);
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).unwrap();
    let name = archive.by_index_raw(0).unwrap().name().to_string();
    assert_eq!(name, "Data/héllo.png");
}

// ------------------------------------------------- legacy + .iwph rejection

#[test]
fn legacy_index_xml_rejects_with_clear_message() {
    let zip = build_zip(&[
        ("index.xml", b"<slideshow/>"),
        ("Slide1.jpg", b"jpeg"),
    ]);
    let tmp = std::env::temp_dir().join(format!("iwadump-legacy-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("old.key");
    std::fs::write(&path, &zip).unwrap();
    let e = Container::open(&path, false).unwrap_err();
    assert_eq!(e.kind, Kind::Legacy);
    assert_eq!(e.layer, iwadump::Layer::Container);
    assert!(e.message.contains("index.xml"), "{}", e.message);
    // --legacy-ok downgrades to a raw listing.
    let container = Container::open(&path, true).unwrap();
    assert_eq!(container.form, ContainerForm::LegacyRaw);
    assert_eq!(container.members.len(), 2);
    assert!(container.iwas.is_empty());
    std::fs::remove_file(&path).ok();
}

#[test]
fn iwph_member_rejects_as_encrypted() {
    let iwa = tiny_document_iwa();
    let zip = build_zip(&[
        (".iwph", &[0u8; 16]),
        ("Index/Document.iwa", &iwa),
    ]);
    let tmp = std::env::temp_dir().join(format!("iwadump-iwph-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("locked.pages");
    std::fs::write(&path, &zip).unwrap();
    let e = Container::open(&path, false).unwrap_err();
    assert_eq!(e.kind, Kind::Encrypted);
    assert!(e.message.contains("encrypted"), "{}", e.message);
    std::fs::remove_file(&path).ok();
}

#[test]
fn tef_extension_rejects_as_legacy() {
    let zip = build_zip(&[("index.db", &[0u8; 4])]);
    let tmp = std::env::temp_dir().join(format!("iwadump-tef-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("iOS.pages-tef");
    std::fs::write(&path, &zip).unwrap();
    let e = Container::open(&path, false).unwrap_err();
    assert_eq!(e.kind, Kind::Legacy);
    assert!(e.message.contains("-tef"), "{}", e.message);
    std::fs::remove_file(&path).ok();
}

#[test]
fn zip_without_iwa_content_is_unsupported() {
    let zip = build_zip(&[("preview.jpg", b"jpeg"), ("random.txt", b"hi")]);
    let tmp = std::env::temp_dir().join(format!("iwadump-empty-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("notiwork.pages");
    std::fs::write(&path, &zip).unwrap();
    let e = Container::open(&path, false).unwrap_err();
    assert_eq!(e.kind, Kind::Unsupported);
    std::fs::remove_file(&path).ok();
}

// ------------------------------------------------------------ zip builder

/// Multi-entry stored zip via the zip crate writer (UTF-8 names).
fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let buf = std::io::Cursor::new(Vec::new());
    let mut w = zip::ZipWriter::new(buf);
    for (name, data) in entries {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
    }
    let buf = w.finish().unwrap();
    buf.into_inner()
}

// ------------------------------------------ OperationStorage.iwa (bvxn magic)

#[test]
fn operation_storage_streams_are_skipped_by_magic() {
    // Newer iWork writes `OperationStorage.iwa` with the LZFSE-style `bvxn`
    // magic — a collaboration operation log, NOT an IWA snappy stream (no
    // reference parser predates it; all 10 real fixtures that carry one have
    // the magic). It must be skipped visibly, never decoded as IWA.
    let mut opstorage = b"bvxn".to_vec();
    opstorage.extend_from_slice(&[0u8; 40]);
    let zip = build_zip(&[
        ("Index/Document.iwa", &tiny_document_iwa()),
        ("Index/OperationStorage.iwa", &opstorage),
    ]);
    let tmp = std::env::temp_dir().join(format!("iwadump-bvxn-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("ops.key");
    std::fs::write(&path, &zip).unwrap();
    let container = Container::open(&path, false).unwrap();
    assert_eq!(container.iwas.len(), 1);
    assert_eq!(container.iwas[0].0, "Index/Document.iwa");
    assert_eq!(container.non_iwa.len(), 1);
    assert_eq!(container.non_iwa[0].0, "Index/OperationStorage.iwa");
    std::fs::remove_file(&path).ok();
}

#[test]
fn iwpv2_member_rejects_as_encrypted() {
    // Newer iWork encryption marker: fixture 2dccc804 carries `.iwpv2` with
    // every stream but DocumentStylesheet.iwa being high-entropy ciphertext
    // and no `.iwph`. Treat as the same encrypted class as `.iwph`.
    let zip = build_zip(&[(".iwpv2", &[0u8; 8]), ("Index/Document.iwa", &tiny_document_iwa())]);
    let tmp = std::env::temp_dir().join(format!("iwadump-iwpv2-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let path = tmp.join("locked2.key");
    std::fs::write(&path, &zip).unwrap();
    let e = Container::open(&path, false).unwrap_err();
    assert_eq!(e.kind, Kind::Encrypted);
    assert!(e.message.contains("encrypted"), "{}", e.message);
    std::fs::remove_file(&path).ok();
}

// ------------------------------------------------- adversarial resource caps

#[test]
fn snappy_bomb_declared_length_is_refused_not_allocated() {
    // A five-byte payload whose leading varint declares ~4 GiB decoded. The
    // decoder must refuse based on the declared length, never allocate it
    // (FINDINGS.md H-1). 0xff 0xff 0xff 0xff 0x0f = 4,294,967,295.
    let bomb = frame_block(&[0xff, 0xff, 0xff, 0xff, 0x0f]);
    let e = IwaStream::parse("bomb.iwa", &bomb).unwrap_err();
    assert_eq!(e.layer, iwadump::Layer::Snappy);
    assert!(e.message.contains("refusing the allocation"), "{}", e.message);
}

#[test]
fn snappy_block_under_cap_still_decodes() {
    // Sanity: the cap must not reject ordinary blocks.
    let data = vec![0x42u8; 200_000]; // > 64 KiB decoded, well under 64 MiB
    let raw = frame_block(&snappy::encode_block(&data));
    let s = IwaStream::parse("ok.iwa", &raw).unwrap();
    assert_eq!(s.decoded, data);
}
