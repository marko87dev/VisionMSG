use std::{fs, path::Path};
use visionmsg::{Message, save_all_attachments};

#[test]
fn parses_a_real_msg_and_builds_eml() {
    let message = Message::open(Path::new("tests/fixtures/ascii.msg")).unwrap();
    assert!(!message.parsed.subject.is_empty());
    assert!(!message.readable_body.trim().is_empty());
    let eml = message.eml_bytes().unwrap();
    let content = String::from_utf8(eml).unwrap();
    assert!(content.contains("Subject:"));
    assert!(content.contains("MIME-Version:"));
}

#[test]
fn saves_real_attachments_without_overwriting_files() {
    let message = Message::open(Path::new("tests/fixtures/attachment.msg")).unwrap();
    assert!(!message.parsed.attachments.is_empty());
    let folder = tempfile::tempdir().unwrap();
    let count = save_all_attachments(&message, folder.path()).unwrap();
    assert_eq!(count, message.parsed.attachments.len());
    let first_round = fs::read_dir(folder.path()).unwrap().count();
    assert_eq!(first_round, count);
    save_all_attachments(&message, folder.path()).unwrap();
    assert_eq!(fs::read_dir(folder.path()).unwrap().count(), count * 2);
    let eml = String::from_utf8(message.eml_bytes().unwrap()).unwrap();
    assert!(eml.contains("multipart/mixed"));
    assert!(
        eml.contains("Content-Disposition: attachment")
            || eml.contains("Content-Disposition: inline"),
        "{}",
        eml.lines()
            .filter(|line| line.starts_with("Content-"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn rejects_corrupt_msg() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("broken.msg");
    fs::write(&path, b"not an Outlook file").unwrap();
    assert!(Message::open(&path).is_err());
}

#[test]
fn exports_readable_text_and_safe_html() {
    let mut message = Message::open(Path::new("tests/fixtures/attachment.msg")).unwrap();
    message.readable_body = "Bonjour <script>alert('x')</script> & merci".to_string();
    let text = message.export_text();
    assert!(text.contains("Sujet :"));
    assert!(text.contains("Pièces jointes :"));
    assert!(text.contains("<script>"));
    let html = message.export_html();
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("&amp; merci"));
    assert!(!html.contains("<script>"));
    assert!(!html.contains("http://"));
}
