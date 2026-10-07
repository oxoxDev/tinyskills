use super::*;

#[test]
fn the_digest_is_lowercase_hex_sha256() {
    assert_eq!(
        document_digest(""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        document_digest("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn every_byte_of_the_document_counts() {
    let doc = "---\nname: N\ndescription: D\n---\nbody\n";
    assert_ne!(
        document_digest(doc),
        document_digest(&doc.replace("D\n", "E\n"))
    );
    assert_ne!(document_digest(doc), document_digest(doc.trim_end()));
    assert_ne!(
        document_digest(doc),
        document_digest(&doc.replace('\n', "\r\n"))
    );
}
