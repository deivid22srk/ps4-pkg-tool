use std::fs;
use std::path::PathBuf;

use pkg_core::sfo::Sfo;
use pkg_core::{analyze, extract, pack, NoopProgress, PackOptions};

fn temp_dir(name: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pkgtool-{name}-{}-{stamp}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn roundtrip_folder_pkg_folder() {
    let base = temp_dir("roundtrip");
    let input = base.join("input");
    let output = base.join("output");
    let extracted = base.join("extracted");
    fs::create_dir_all(input.join("sce_sys")).unwrap();
    fs::create_dir_all(input.join("data/sub")).unwrap();

    let sfo = Sfo::default_for(
        "Teste PKG",
        "CUSA12345",
        "UP0000-CUSA12345_00-TESTE00000000000",
    );
    fs::write(input.join("sce_sys/param.sfo"), sfo.serialize()).unwrap();
    fs::write(input.join("sce_sys/icon0.png"), [1u8, 2, 3, 4]).unwrap();
    fs::write(input.join("data/hello.txt"), b"hello pkg").unwrap();
    fs::write(input.join("data/empty.bin"), b"").unwrap();

    let summary = pack(&input, &output, &PackOptions::default(), &mut NoopProgress).unwrap();
    assert_eq!(summary.title, "Teste PKG");
    assert_eq!(summary.title_id, "CUSA12345");
    assert_eq!(summary.content_id, "UP0000-CUSA12345_00-TESTE00000000000");
    assert!(output.is_file());
    assert_eq!(fs::metadata(&output).unwrap().len(), summary.pkg_size);

    let info = analyze(&output).unwrap();
    assert!(!info.encrypted);
    assert_eq!(info.content_id, summary.content_id);
    assert_eq!(info.title, "Teste PKG");
    assert_eq!(info.file_count, 2, "warning: {}", info.warning);

    let total = extract(&output, &extracted, &mut NoopProgress).unwrap();
    assert_eq!(total, 9);
    assert_eq!(fs::read(extracted.join("data/hello.txt")).unwrap(), b"hello pkg");
    assert_eq!(fs::read(extracted.join("data/empty.bin")).unwrap(), b"");
    let extracted_sfo = Sfo::parse(&fs::read(extracted.join("sce_sys/param.sfo")).unwrap()).unwrap();
    assert_eq!(extracted_sfo.get_text("TITLE").unwrap(), "Teste PKG");
    assert_eq!(extracted_sfo.get_text("TITLE_ID").unwrap(), "CUSA12345");
    assert_eq!(
        extracted_sfo.get_text("CONTENT_ID").unwrap(),
        "UP0000-CUSA12345_00-TESTE00000000000"
    );
    assert_eq!(
        fs::read(extracted.join("sce_sys/icon0.png")).unwrap(),
        vec![1u8, 2, 3, 4]
    );

    let _ = fs::remove_dir_all(&base);
}

#[test]
fn pack_generates_param_sfo_when_missing() {
    let base = temp_dir("nosfo");
    let input = base.join("input");
    fs::create_dir_all(input.join("game")).unwrap();
    fs::write(input.join("game/file.txt"), b"abc").unwrap();
    let output = base.join("generated.pkg");

    let summary = pack(&input, &output, &PackOptions::default(), &mut NoopProgress).unwrap();
    assert_eq!(summary.file_count, 1);
    assert!(summary.content_id.starts_with("UP0000-CUSA"));
    let info = analyze(&output).unwrap();
    assert!(!info.encrypted);
    assert_eq!(info.file_count, 1);
    assert!(!info.title.is_empty());

    let _ = fs::remove_dir_all(&base);
}
