//! What a skill archive is allowed to be.
//!
//! Every archive here is crafted rather than described: a cap asserted against
//! a hand-written struct proves the arithmetic, but only an archive that really
//! carries the traversal, the link or the bomb proves the reader refuses one.

#![allow(clippy::unwrap_used, clippy::cast_possible_truncation)]

use super::*;

use std::io::{Cursor, Write};

use zip::write::{SimpleFileOptions, ZipWriter};

const DOC: &str = "---\nname: Press Outreach\ndescription: Pitch a story.\n---\nSteps.\n";

/// A zip of `(path, contents)` pairs, stored uncompressed so the declared
/// sizes are the real ones. A path ending in `/` is a directory.
fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (path, contents) in entries {
        if path.ends_with('/') {
            writer.add_directory(*path, options).unwrap();
            continue;
        }
        writer.start_file(*path, options).unwrap();
        writer.write_all(contents).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// A tar of `(path, contents)` pairs. The path is written raw into the header
/// so a hostile one survives — `tar::Builder::append_data` would refuse it.
fn tar(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = ::tar::Builder::new(Vec::new());
    for (path, contents) in entries {
        let mut header = ::tar::Header::new_gnu();
        let name = &mut header.as_gnu_mut().unwrap().name;
        name[..path.len()].copy_from_slice(path.as_bytes());
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(::tar::EntryType::Regular);
        header.set_cksum();
        builder.append(&header, *contents).unwrap();
    }
    builder.into_inner().unwrap()
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn read(format: ArchiveFormat, bytes: &[u8]) -> Result<SkillArchive, ArchiveError> {
    read_skill_archive(format, bytes, &ArchiveLimits::default())
}

// --- formats ---

#[test]
fn the_format_is_chosen_by_file_name() {
    assert_eq!(
        ArchiveFormat::from_file_name("a.zip"),
        Some(ArchiveFormat::Zip)
    );
    assert_eq!(
        ArchiveFormat::from_file_name("A.SKILL"),
        Some(ArchiveFormat::Zip)
    );
    assert_eq!(
        ArchiveFormat::from_file_name("a.tar"),
        Some(ArchiveFormat::Tar)
    );
    assert_eq!(
        ArchiveFormat::from_file_name("a.tar.gz"),
        Some(ArchiveFormat::TarGz)
    );
    assert_eq!(
        ArchiveFormat::from_file_name("a.tgz"),
        Some(ArchiveFormat::TarGz)
    );
    assert_eq!(ArchiveFormat::from_file_name("SKILL.md"), None);
}

// --- shape ---

#[test]
fn a_document_at_the_root_has_no_root_directory() {
    let read = read(ArchiveFormat::Zip, &zip(&[("SKILL.md", DOC.as_bytes())])).unwrap();
    assert_eq!(read.root, None);
    assert_eq!(read.document, DOC);
    assert_eq!(read.resources, Vec::<ArchiveFile>::new());
}

#[test]
fn one_top_directory_is_the_root_and_its_files_are_resources() {
    let bytes = zip(&[
        ("press-outreach/", b""),
        ("press-outreach/SKILL.md", DOC.as_bytes()),
        ("press-outreach/scripts/pitch.py", b"print(1)"),
    ]);
    let read = read(ArchiveFormat::Zip, &bytes).unwrap();
    assert_eq!(read.root.as_deref(), Some("press-outreach"));
    assert_eq!(
        read.resources,
        vec![ArchiveFile {
            path: "scripts/pitch.py".into(),
            bytes: b"print(1)".to_vec(),
        }]
    );
}

#[test]
fn a_tar_and_a_tar_gz_read_the_same_as_a_zip() {
    let entries: &[(&str, &[u8])] = &[
        ("press-outreach/SKILL.md", DOC.as_bytes()),
        ("press-outreach/references/notes.md", b"notes"),
    ];
    let from_tar = read(ArchiveFormat::Tar, &tar(entries)).unwrap();
    let from_tgz = read(ArchiveFormat::TarGz, &gzip(&tar(entries))).unwrap();
    let from_zip = read(ArchiveFormat::Zip, &zip(entries)).unwrap();
    assert_eq!(from_tar, from_zip);
    assert_eq!(from_tgz, from_zip);
    assert_eq!(from_zip.resources[0].path, "references/notes.md");
}

#[test]
fn macos_bookkeeping_is_dropped() {
    let bytes = zip(&[
        ("skill/SKILL.md", DOC.as_bytes()),
        ("skill/.DS_Store", b"x"),
        ("__MACOSX/skill/._SKILL.md", b"x"),
    ]);
    let read = read(ArchiveFormat::Zip, &bytes).unwrap();
    assert_eq!(read.root.as_deref(), Some("skill"));
    assert_eq!(read.resources, Vec::<ArchiveFile>::new());
}

#[test]
fn leading_dot_segments_are_removed_from_zip_and_tar_entries() {
    let entries = [("./press-outreach/SKILL.md", DOC.as_bytes())];
    for (format, bytes) in [
        (ArchiveFormat::Zip, zip(&entries)),
        (ArchiveFormat::Tar, tar(&entries)),
    ] {
        let read = read(format, &bytes).unwrap();
        assert_eq!(read.root.as_deref(), Some("press-outreach"));
        assert_eq!(read.document, DOC);
    }
}

#[test]
fn dot_only_archive_entries_are_ignored() {
    let entries = [("./", b"".as_slice()), ("./SKILL.md", DOC.as_bytes())];
    for (format, bytes) in [
        (ArchiveFormat::Zip, zip(&entries)),
        (ArchiveFormat::Tar, tar(&entries)),
    ] {
        let read = read(format, &bytes).unwrap();
        assert_eq!(read.root, None);
        assert_eq!(read.document, DOC);
    }
}

#[test]
fn two_top_directories_are_refused() {
    let bytes = zip(&[
        ("a/SKILL.md", DOC.as_bytes()),
        ("b/SKILL.md", DOC.as_bytes()),
    ]);
    assert_eq!(
        read(ArchiveFormat::Zip, &bytes),
        Err(ArchiveError::MultipleRoots)
    );
}

#[test]
fn a_root_file_beside_a_directory_is_refused() {
    let bytes = zip(&[("SKILL.md", DOC.as_bytes()), ("a/notes.md", b"x")]);
    assert_eq!(
        read(ArchiveFormat::Zip, &bytes),
        Err(ArchiveError::MultipleRoots)
    );
}

#[test]
fn an_archive_without_a_skill_document_is_refused() {
    let bytes = zip(&[("skill/README.md", b"x")]);
    assert_eq!(
        read(ArchiveFormat::Zip, &bytes),
        Err(ArchiveError::NoSkillDocument)
    );
}

#[test]
fn a_document_that_is_not_utf8_is_refused() {
    let bytes = zip(&[("SKILL.md", &[0xff, 0xfe, 0x00])]);
    assert_eq!(read(ArchiveFormat::Zip, &bytes), Err(ArchiveError::NotUtf8));
}

#[test]
fn bytes_that_are_not_an_archive_are_refused() {
    for format in [ArchiveFormat::Zip, ArchiveFormat::TarGz] {
        assert!(matches!(
            read(format, b"not an archive"),
            Err(ArchiveError::Unreadable(_))
        ));
    }
}

// --- paths ---

#[test]
fn every_escaping_path_is_refused_in_both_formats() {
    for (path, expected) in [
        ("../SKILL.md", ArchiveError::Traversal("../SKILL.md".into())),
        (
            "skill/../../x",
            ArchiveError::Traversal("skill/../../x".into()),
        ),
        (
            "/etc/SKILL.md",
            ArchiveError::AbsolutePath("/etc/SKILL.md".into()),
        ),
        (
            "C:/SKILL.md",
            ArchiveError::AbsolutePath("C:/SKILL.md".into()),
        ),
        (
            "skill\\SKILL.md",
            ArchiveError::BackslashPath("skill\\SKILL.md".into()),
        ),
    ] {
        let entries: &[(&str, &[u8])] = &[(path, DOC.as_bytes())];
        assert_eq!(
            read(ArchiveFormat::Zip, &zip(entries)),
            Err(expected.clone()),
            "zip {path}"
        );
        assert_eq!(
            read(ArchiveFormat::Tar, &tar(entries)),
            Err(expected),
            "tar {path}"
        );
    }
}

#[test]
fn a_zip_symlink_is_refused() {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_symlink(
            "skill/SKILL.md",
            "/etc/passwd",
            SimpleFileOptions::default(),
        )
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    assert_eq!(
        read(ArchiveFormat::Zip, &bytes),
        Err(ArchiveError::Link("skill/SKILL.md".into()))
    );
}

#[test]
fn a_tar_symlink_or_hard_link_is_refused() {
    for kind in [::tar::EntryType::Symlink, ::tar::EntryType::Link] {
        let mut builder = ::tar::Builder::new(Vec::new());
        let mut header = ::tar::Header::new_gnu();
        header.set_entry_type(kind);
        header.set_size(0);
        builder
            .append_link(&mut header, "skill/SKILL.md", "/etc/passwd")
            .unwrap();
        let bytes = builder.into_inner().unwrap();
        assert_eq!(
            read(ArchiveFormat::Tar, &bytes),
            Err(ArchiveError::Link("skill/SKILL.md".into())),
            "{kind:?}"
        );
    }
}

#[test]
fn a_nested_archive_is_refused() {
    let bytes = zip(&[
        ("skill/SKILL.md", DOC.as_bytes()),
        ("skill/more.zip", b"PK"),
    ]);
    assert_eq!(
        read(ArchiveFormat::Zip, &bytes),
        Err(ArchiveError::NestedArchive("skill/more.zip".into()))
    );
}

// --- bombs ---

#[test]
fn too_many_entries_are_refused() {
    let names: Vec<String> = (0..=MAX_ARCHIVE_ENTRIES)
        .map(|i| format!("s/{i}.md"))
        .collect();
    let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b"x"[..])).collect();
    for (format, bytes) in [
        (ArchiveFormat::Zip, zip(&entries)),
        (ArchiveFormat::Tar, tar(&entries)),
    ] {
        assert_eq!(
            read(format, &bytes),
            Err(ArchiveError::TooManyEntries {
                max: MAX_ARCHIVE_ENTRIES
            }),
            "{format:?}"
        );
    }
}

#[test]
fn a_declared_size_over_the_cap_is_refused() {
    let limits = ArchiveLimits {
        max_entries: 8,
        max_bytes: 16,
    };
    let entries: &[(&str, &[u8])] = &[("SKILL.md", DOC.as_bytes())];
    for (format, bytes) in [
        (ArchiveFormat::Zip, zip(entries)),
        (ArchiveFormat::Tar, tar(entries)),
    ] {
        assert_eq!(
            read_skill_archive(format, &bytes, &limits),
            Err(ArchiveError::TooLarge { max_bytes: 16 }),
            "{format:?}"
        );
    }
}

/// A few kilobytes of gzip that expand to megabytes are refused at the
/// ceiling rather than inflated.
#[test]
fn a_gzip_bomb_is_refused_at_the_ceiling() {
    let big = vec![b'a'; 4 * 1024 * 1024];
    let bomb = gzip(&tar(&[("SKILL.md", &big)]));
    assert!(bomb.len() < 64 * 1024, "sanity: it compresses");
    assert_eq!(
        read(ArchiveFormat::TarGz, &bomb),
        Err(ArchiveError::TooLarge {
            max_bytes: MAX_ARCHIVE_BYTES
        })
    );
}

#[test]
fn every_error_reads_as_a_sentence() {
    let message = ArchiveError::Traversal("../x".into()).to_string();
    assert!(message.contains("`../x`"), "{message}");
}
