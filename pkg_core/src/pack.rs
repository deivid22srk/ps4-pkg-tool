use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::{io_err, CoreError, Result};
use crate::pfs_builder::{build_tree, BuiltPfs, PackTree};
use crate::progress::{ByteProgress, Progress};
use crate::sfo::{Sfo, SfoValue};

const BODY_OFFSET: u64 = 0x2000;
const BODY_ALIGN: u64 = 0x80000;
const MIN_INNER_BLOCKS: u64 = 0x55;

#[derive(Debug, Clone, Default)]
pub struct PackOptions {
    pub content_id: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PackSummary {
    pub content_id: String,
    pub title: String,
    pub title_id: String,
    pub file_count: u64,
    pub input_bytes: u64,
    pub pkg_size: u64,
}

struct ExtraFile {
    id: u32,
    name: &'static str,
    rel: &'static str,
    path: PathBuf,
    size: u64,
}

enum EntryData {
    Bytes(Vec<u8>),
    File(PathBuf, u64),
    Metas,
    Digests,
}

struct Entry {
    id: u32,
    name: Option<&'static str>,
    data: EntryData,
    data_offset: u64,
    data_size: u64,
}

pub fn pack(
    input: &Path,
    output: &Path,
    options: &PackOptions,
    progress: &mut dyn Progress,
) -> Result<PackSummary> {
    progress.report(0, "Lendo pasta de entrada")?;

    let extras = find_extra_files(input);
    let sfo_path = input.join("sce_sys").join("param.sfo");
    let mut sfo = if sfo_path.is_file() {
        let bytes = fs::read(&sfo_path).map_err(|e| io_err("ler param.sfo", e))?;
        Sfo::parse(&bytes).unwrap_or_default()
    } else {
        Sfo::default()
    };

    let title = options
        .title
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| sfo.get_text("TITLE"))
        .unwrap_or_else(|| {
            input
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "PS4 Homebrew".into())
        });

    let content_id = options
        .content_id
        .as_ref()
        .map(|s| s.trim().to_ascii_uppercase())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            sfo.get_text("CONTENT_ID")
                .map(|s| s.trim().to_ascii_uppercase())
                .filter(|s| valid_content_id(s))
        });

    // O TITLE_ID deve casar com o Content ID (posições 7..16).
    let content_title_id = content_id
        .as_ref()
        .filter(|s| valid_content_id(s))
        .map(|s| s[7..16].to_string())
        .filter(|s| valid_title_id(s));
    let title_id = content_title_id
        .or_else(|| sfo.get_text("TITLE_ID").filter(|s| valid_title_id(s)))
        .unwrap_or_else(|| derive_title_id(&title));
    let content_id = content_id.unwrap_or_else(|| derive_content_id(&title_id, &title));

    if !valid_content_id(&content_id) {
        return Err(CoreError::InvalidInput(
            "Content ID inválido: use 36 caracteres, ex. UP0000-CUSA00000_00-NOME000000000000"
                .into(),
        ));
    }
    if !valid_title_id(&title_id) {
        return Err(CoreError::InvalidInput(
            "TITLE_ID inválido: use 9 caracteres, ex. CUSA00000".into(),
        ));
    }

    sfo.set_text("CONTENT_ID", content_id.clone());
    sfo.set_text("TITLE", title.clone());
    sfo.set_text("TITLE_ID", title_id.clone());
    ensure_sfo_defaults(&mut sfo);
    let sfo_bytes = sfo.serialize();
    let major_param = major_param_string(&sfo);

    let mut exclude = HashSet::new();
    exclude.insert("sce_sys/param.sfo".to_string());
    for extra in &extras {
        exclude.insert(extra.rel.to_string());
    }

    let tree = PackTree::from_directory(input, &exclude)?;
    let file_count = tree.file_count();
    let input_bytes = tree.total_bytes();
    let inner = build_tree(tree, MIN_INNER_BLOCKS)?;
    let inner_size = inner.size();

    let temp_inner = temp_inner_path(output);
    let result = pack_inner_and_outer(
        &inner,
        inner_size,
        &temp_inner,
        output,
        &content_id,
        &title,
        &title_id,
        &sfo_bytes,
        &major_param,
        &extras,
        file_count,
        input_bytes,
        progress,
    );

    let _ = fs::remove_file(&temp_inner);
    if result.is_err() {
        let _ = fs::remove_file(output);
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn pack_inner_and_outer(
    inner: &BuiltPfs,
    inner_size: u64,
    temp_inner: &Path,
    output: &Path,
    content_id: &str,
    title: &str,
    title_id: &str,
    sfo_bytes: &[u8],
    major_param: &str,
    extras: &[ExtraFile],
    file_count: u64,
    input_bytes: u64,
    progress: &mut dyn Progress,
) -> Result<PackSummary> {
    {
        progress.report(3, "Gerando PFS interno")?;
        let mut file = File::create(temp_inner).map_err(|e| io_err("criar PFS temporário", e))?;
        file.set_len(inner_size)
            .map_err(|e| io_err("reservar espaço do PFS", e))?;
        let mut bp = ByteProgress::new(
            &mut *progress,
            5,
            45,
            input_bytes.max(1),
            "Gravando conteúdo no PFS",
        );
        inner.write(&mut file, 0, |n| bp.add(n))?;
        bp.finish()?;
        file.sync_all().map_err(|e| io_err("sincronizar PFS", e))?;
    }

    let outer_tree = PackTree::single_virtual_file(temp_inner.to_path_buf(), inner_size);
    let outer = build_tree(outer_tree, 0)?;
    let outer_size = outer.size();

    let (mut entries, names) = build_entries(sfo_bytes, extras);
    let entry_count = entries.len();
    if entry_count > u16::MAX as usize {
        return Err(CoreError::Unsupported("entradas demais no PKG".into()));
    }
    layout_entries(&mut entries)?;
    let metas_offset = entries
        .iter()
        .find(|e| e.id == 0x100)
        .map(|e| e.data_offset)
        .unwrap_or(0);
    let body_end = entries
        .iter()
        .map(|e| e.data_offset + e.data_size)
        .max()
        .unwrap_or(BODY_OFFSET);
    let body_size = align(body_end, BODY_ALIGN) - BODY_OFFSET;
    let pfs_offset = BODY_OFFSET + body_size;
    let package_size = pfs_offset + outer_size;

    if package_size > u32::MAX as u64 {
        return Err(CoreError::Unsupported(
            "PKG maior que 4 GiB não é suportado nesta versão".into(),
        ));
    }

    progress.report(46, "Reservando arquivo PKG")?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(output)
        .map_err(|e| io_err("criar PKG de saída", e))?;
    file.set_len(package_size)
        .map_err(|e| io_err("reservar espaço do PKG", e))?;

    // Corpo: entradas
    let mut general_bytes = vec![0u8; 0x180];
    let mut digests_bytes = vec![0u8; entry_count * 32];
    let meta_bytes = build_meta_table(&entries, &names);

    for entry in &entries {
        match &entry.data {
            EntryData::Bytes(bytes) => {
                write_at(&mut file, entry.data_offset, bytes)?;
            }
            EntryData::File(path, size) => {
                if *size > 0 {
                    copy_file_at(&mut file, path, entry.data_offset, *size)?;
                }
            }
            EntryData::Metas => {
                write_at(&mut file, entry.data_offset, &meta_bytes)?;
            }
            EntryData::Digests => {
                write_at(&mut file, entry.data_offset, &digests_bytes)?;
            }
        }
    }

    // PFS externo
    progress.report(48, "Gravando imagem PFS no PKG")?;
    {
        let mut bp = ByteProgress::new(
            &mut *progress,
            48,
            82,
            inner_size.max(1),
            "Gravando PFS no PKG",
        );
        outer.write(&mut file, pfs_offset, |n| bp.add(n))?;
        bp.finish()?;
    }

    progress.report(84, "Calculando digests do PFS")?;
    let pfs_digest = sha256_range(&mut file, pfs_offset, outer_size)?;
    let pfs_signed_digest = sha256_range(&mut file, pfs_offset, outer_size.min(0x10000))?;

    // O header é montado antes dos digests gerais porque o GeneralDigests
    // inclui um hash de partes do próprio header.
    let main_ent_data_size: u64 = [0x10u32, 0x20, 0x80, 0x100, 0x01]
        .iter()
        .map(|id| entry_size(&entries, *id))
        .sum();
    let mut header = vec![0u8; 0x1000];
    header[0..4].copy_from_slice(&0x7F43_4E54u32.to_be_bytes());
    put_be_u32(&mut header, 0x04, 1);
    put_be_u32(&mut header, 0x0C, 0x0F);
    put_be_u32(&mut header, 0x10, entry_count as u32);
    put_be_u16(&mut header, 0x14, 6);
    put_be_u16(&mut header, 0x16, entry_count as u16);
    put_be_u32(&mut header, 0x18, metas_offset as u32);
    put_be_u32(&mut header, 0x1C, main_ent_data_size as u32);
    put_be_u64(&mut header, 0x20, BODY_OFFSET);
    put_be_u64(&mut header, 0x28, body_size);

    let cid = content_id.as_bytes();
    let cid_len = cid.len().min(36);
    header[0x40..0x40 + cid_len].copy_from_slice(&cid[..cid_len]);
    put_be_u32(&mut header, 0x70, 0x0F);
    put_be_u32(&mut header, 0x74, 0x1A);
    put_be_u32(&mut header, 0x78, 0x0A00_0000);
    put_be_u32(&mut header, 0x80, 0x2016_1020);
    put_be_u32(&mut header, 0x84, 0x0173_8551);
    put_be_u32(&mut header, 0x9C, 1);
    put_be_u32(&mut header, 0x400, 1);
    put_be_u32(&mut header, 0x404, 1);
    put_be_u64(&mut header, 0x408, 0x8000_0000_0000_03CC);
    put_be_u64(&mut header, 0x410, pfs_offset);
    put_be_u64(&mut header, 0x418, outer_size);
    put_be_u64(&mut header, 0x428, package_size);
    put_be_u64(&mut header, 0x430, package_size);
    put_be_u32(&mut header, 0x438, 0x10000);
    put_be_u32(&mut header, 0x43C, 0xD0000);
    header[0x440..0x460].copy_from_slice(&pfs_digest);
    header[0x460..0x480].copy_from_slice(&pfs_signed_digest);

    // GeneralDigests: Content, Game, Header, System, MajorParam e Param.
    let param_digest = sha256_bytes(sfo_bytes);
    let major_param_digest = sha256_bytes(major_param.as_bytes());
    let mut content_hasher = Sha256::new();
    content_hasher.update(content_id.as_bytes());
    content_hasher.update([0u8; 12]);
    content_hasher.update(0x0000_000Fu32.to_be_bytes());
    content_hasher.update(0x0000_001Au32.to_be_bytes());
    content_hasher.update(pfs_digest);
    content_hasher.update(major_param_digest);
    let content_digest: [u8; 32] = content_hasher.finalize().into();

    let mut general_header_hasher = Sha256::new();
    general_header_hasher.update(&header[0..0x40]);
    general_header_hasher.update(&header[0x400..0x480]);
    let general_header_digest: [u8; 32] = general_header_hasher.finalize().into();

    general_bytes[0x00..0x02].copy_from_slice(&0xD256u16.to_be_bytes());
    general_bytes[0x02..0x04].copy_from_slice(&0x0100u16.to_be_bytes());
    general_bytes[0x1C..0x20].copy_from_slice(&0x0000_006Eu32.to_be_bytes());
    general_bytes[0x20..0x40].copy_from_slice(&content_digest);
    general_bytes[0x40..0x60].copy_from_slice(&pfs_digest);
    general_bytes[0x60..0x80].copy_from_slice(&general_header_digest);
    general_bytes[0xA0..0xC0].copy_from_slice(&major_param_digest);
    general_bytes[0xC0..0xE0].copy_from_slice(&param_digest);

    let general_entry = entries
        .iter()
        .find(|e| e.id == 0x80)
        .ok_or_else(|| CoreError::Internal("entrada GeneralDigests ausente".into()))?;
    write_at(&mut file, general_entry.data_offset, &general_bytes)?;

    // Digests por entrada (tabela ordenada por ID)
    progress.report(88, "Calculando digests das entradas")?;
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by_key(|e| e.id);
    for (i, entry) in sorted.iter().enumerate() {
        if entry.id == 0x01 {
            continue;
        }
        let hash = sha256_range(&mut file, entry.data_offset, entry.data_size)?;
        digests_bytes[i * 32..i * 32 + 32].copy_from_slice(&hash);
    }
    let digests_entry = entries
        .iter()
        .find(|e| e.id == 0x01)
        .ok_or_else(|| CoreError::Internal("entrada Digests ausente".into()))?;
    write_at(&mut file, digests_entry.data_offset, &digests_bytes)?;

    let body_digest = sha256_range(&mut file, BODY_OFFSET, body_size)?;
    let digest_table_hash = sha256_bytes(&digests_bytes);
    let sc1 = sha256_ranges(
        &mut file,
        &[
            (entry_offset(&entries, 0x10), entry_size(&entries, 0x10)),
            (entry_offset(&entries, 0x20), entry_size(&entries, 0x20)),
            (entry_offset(&entries, 0x80), entry_size(&entries, 0x80)),
            (entry_offset(&entries, 0x100), entry_size(&entries, 0x100)),
            (entry_offset(&entries, 0x01), entry_size(&entries, 0x01)),
        ],
    )?;
    let sc2 = sha256_ranges(
        &mut file,
        &[
            (entry_offset(&entries, 0x10), entry_size(&entries, 0x10)),
            (entry_offset(&entries, 0x20), entry_size(&entries, 0x20)),
            (entry_offset(&entries, 0x80), entry_size(&entries, 0x80)),
            (entry_offset(&entries, 0x100), 6 * 0x20),
        ],
    )?;

    progress.report(93, "Gravando cabeçalho do PKG")?;
    header[0x100..0x120].copy_from_slice(&sc1);
    header[0x120..0x140].copy_from_slice(&sc2);
    header[0x140..0x160].copy_from_slice(&digest_table_hash);
    header[0x160..0x180].copy_from_slice(&body_digest);
    write_at(&mut file, 0, &header)?;

    let header_digest = sha256_range(&mut file, 0, 0xFE0)?;
    write_at(&mut file, 0xFE0, &header_digest)?;
    file.sync_all().map_err(|e| io_err("sincronizar PKG", e))?;

    progress.report(100, "PKG criado")?;
    Ok(PackSummary {
        content_id: content_id.to_string(),
        title: title.to_string(),
        title_id: title_id.to_string(),
        file_count,
        input_bytes,
        pkg_size: package_size,
    })
}

fn build_entries(sfo_bytes: &[u8], extras: &[ExtraFile]) -> (Vec<Entry>, HashMap<String, u32>) {
    let mut names: Vec<&str> = vec!["", "param.sfo", "psreserved.dat"];
    for extra in extras {
        names.push(extra.name);
    }
    let mut name_offsets = HashMap::new();
    let mut offset = 1u32;
    for name in &names {
        if name.is_empty() {
            name_offsets.insert(String::new(), 0);
            continue;
        }
        name_offsets.insert((*name).to_string(), offset);
        offset += name.len() as u32 + 1;
    }
    let mut name_bytes = vec![0u8];
    for name in &names {
        if name.is_empty() {
            continue;
        }
        name_bytes.extend_from_slice(name.as_bytes());
        name_bytes.push(0);
    }

    let count = 8 + extras.len();
    let mut entries = vec![
        Entry {
            id: 0x10,
            name: None,
            data: EntryData::Bytes(vec![0u8; 0x800]),
            data_offset: 0,
            data_size: 0,
        },
        Entry {
            id: 0x20,
            name: None,
            data: EntryData::Bytes(vec![0u8; 0x100]),
            data_offset: 0,
            data_size: 0,
        },
        Entry {
            id: 0x80,
            name: None,
            data: EntryData::Bytes(vec![0u8; 0x180]),
            data_offset: 0,
            data_size: 0,
        },
        Entry {
            id: 0x100,
            name: None,
            data: EntryData::Metas,
            data_offset: 0,
            data_size: (count * 32) as u64,
        },
        Entry {
            id: 0x01,
            name: None,
            data: EntryData::Digests,
            data_offset: 0,
            data_size: (count * 32) as u64,
        },
        Entry {
            id: 0x200,
            name: None,
            data: EntryData::Bytes(name_bytes),
            data_offset: 0,
            data_size: 0,
        },
        Entry {
            id: 0x1000,
            name: Some("param.sfo"),
            data: EntryData::Bytes(sfo_bytes.to_vec()),
            data_offset: 0,
            data_size: 0,
        },
        Entry {
            id: 0x409,
            name: Some("psreserved.dat"),
            data: EntryData::Bytes(vec![0u8; 0x2000]),
            data_offset: 0,
            data_size: 0,
        },
    ];
    for extra in extras {
        entries.push(Entry {
            id: extra.id,
            name: Some(extra.name),
            data: EntryData::File(extra.path.clone(), extra.size),
            data_offset: 0,
            data_size: 0,
        });
    }
    (entries, name_offsets)
}

fn layout_entries(entries: &mut [Entry]) -> Result<()> {
    let mut current = BODY_OFFSET;
    for entry in entries.iter_mut() {
        current = align16(current);
        let size = match &entry.data {
            EntryData::Bytes(bytes) => bytes.len() as u64,
            EntryData::File(_, size) => *size,
            EntryData::Metas => entry.data_size,
            EntryData::Digests => entry.data_size,
        };
        entry.data_offset = current;
        entry.data_size = size;
        current = current
            .checked_add(size)
            .ok_or_else(|| CoreError::InvalidInput("PKG grande demais".into()))?;
    }
    Ok(())
}

fn build_meta_table(entries: &[Entry], names: &HashMap<String, u32>) -> Vec<u8> {
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by_key(|e| e.id);
    let mut out = Vec::with_capacity(sorted.len() * 32);
    for entry in sorted {
        let name_offset = entry
            .name
            .and_then(|n| names.get(n).copied())
            .unwrap_or(0);
        let (flags1, flags2) = match entry.id {
            0x01 => (0x4000_0000u32, 0u32),
            0x10 => (0x6000_0000, 0),
            0x20 => (0xE000_0000, 3u32 << 12),
            0x80 => (0x6000_0000, 0),
            0x100 => (0x6000_0000, 0),
            0x200 => (0x4000_0000, 0),
            0x400 => (0x8000_0000, 3u32 << 12),
            0x401 => (0x8000_0000, 2u32 << 12),
            _ => (0, 0),
        };
        out.extend_from_slice(&entry.id.to_be_bytes());
        out.extend_from_slice(&name_offset.to_be_bytes());
        out.extend_from_slice(&flags1.to_be_bytes());
        out.extend_from_slice(&flags2.to_be_bytes());
        out.extend_from_slice(&(entry.data_offset as u32).to_be_bytes());
        out.extend_from_slice(&(entry.data_size as u32).to_be_bytes());
        out.extend_from_slice(&[0u8; 8]);
    }
    out
}

fn find_extra_files(input: &Path) -> Vec<ExtraFile> {
    let candidates: [(u32, &'static str, &'static str); 4] = [
        (0x1200, "icon0.png", "sce_sys/icon0.png"),
        (0x1220, "pic0.png", "sce_sys/pic0.png"),
        (0x1006, "pic1.png", "sce_sys/pic1.png"),
        (0x1240, "snd0.at9", "sce_sys/snd0.at9"),
    ];
    let mut out = Vec::new();
    for (id, name, rel) in candidates {
        let path = input.join(rel);
        if let Ok(metadata) = fs::metadata(&path) {
            if metadata.is_file() {
                out.push(ExtraFile {
                    id,
                    name,
                    rel,
                    path,
                    size: metadata.len(),
                });
            }
        }
    }
    out
}

fn ensure_sfo_defaults(sfo: &mut Sfo) {
    if sfo.get("ATTRIBUTE").is_none() {
        sfo.set_int("ATTRIBUTE", 0);
    }
    if sfo.get("ATTRIBUTE2").is_none() {
        sfo.set_int("ATTRIBUTE2", 0);
    }
    if sfo.get("FORMAT").is_none() {
        sfo.set_text("FORMAT", "obs");
    }
    if sfo.get("APP_TYPE").is_none() {
        sfo.set_int("APP_TYPE", 1);
    }
    if sfo.get("APP_VER").is_none() {
        sfo.set_text("APP_VER", "01.00");
    }
    if sfo.get("CATEGORY").is_none() {
        sfo.set_text("CATEGORY", "gd");
    }
    if sfo.get("PARENTAL_LEVEL").is_none() {
        sfo.set_int("PARENTAL_LEVEL", 5);
    }
    if sfo.get("PUBTOOLVER").is_none() {
        sfo.set_int("PUBTOOLVER", 0x0289_0000);
    }
    if sfo.get("SYSTEM_VER").is_none() {
        sfo.set_int("SYSTEM_VER", 0);
    }
    if sfo.get("VERSION").is_none() {
        sfo.set_text("VERSION", "01.00");
    }
    let empty_category = sfo
        .get_text("CATEGORY")
        .map(|c| c.trim().is_empty())
        .unwrap_or(true);
    if empty_category {
        sfo.set_text("CATEGORY", "gd");
    }
}

fn sfo_value_text(sfo: &Sfo, key: &str) -> String {
    match sfo.get(key) {
        Some(SfoValue::Text(value)) => value.clone(),
        Some(SfoValue::Int(value)) => format!("0x{value:08x}"),
        None => String::new(),
    }
}

fn major_param_string(sfo: &Sfo) -> String {
    let mut out = String::new();
    out.push_str("ATTRIBUTE");
    out.push_str(&sfo_value_text(sfo, "ATTRIBUTE"));
    if sfo.get("ATTRIBUTE2").is_some() {
        out.push_str("ATTRIBUTE2");
        out.push_str(&sfo_value_text(sfo, "ATTRIBUTE2"));
    }
    out.push_str("CATEGORY");
    out.push_str(&sfo_value_text(sfo, "CATEGORY"));
    out.push_str("FORMAT");
    out.push_str(&sfo_value_text(sfo, "FORMAT"));
    out.push_str("PUBTOOLVER");
    out.push_str(&sfo_value_text(sfo, "PUBTOOLVER"));
    out
}

fn derive_title_id(seed: &str) -> String {
    let hash = sha256_bytes(seed.as_bytes());
    let value = u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]]) & 0x000F_FFFF;
    format!("CUSA{value:05X}")
}

fn derive_content_id(title_id: &str, title: &str) -> String {
    let mut label: String = title
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .take(16)
        .collect();
    while label.len() < 16 {
        label.push('0');
    }
    format!("UP0000-{title_id}_00-{label}")
}

fn valid_title_id(value: &str) -> bool {
    value.len() == 9
        && value
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

fn valid_content_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes[0..2].iter().all(|b| b.is_ascii_uppercase())
        && bytes[2..6].iter().all(|b| b.is_ascii_digit())
        && bytes[6] == b'-'
        && bytes[7..16]
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        && bytes[16] == b'_'
        && bytes[17..19].iter().all(|b| b.is_ascii_digit())
        && bytes[19] == b'-'
        && bytes[20..36]
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

fn temp_inner_path(output: &Path) -> PathBuf {
    let name = output
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "output.pkg".into());
    output.with_file_name(format!(".{name}.inner.tmp"))
}

fn entry_offset(entries: &[Entry], id: u32) -> u64 {
    entries
        .iter()
        .find(|e| e.id == id)
        .map(|e| e.data_offset)
        .unwrap_or(0)
}

fn entry_size(entries: &[Entry], id: u32) -> u64 {
    entries
        .iter()
        .find(|e| e.id == id)
        .map(|e| e.data_size)
        .unwrap_or(0)
}

fn write_at(file: &mut File, offset: u64, data: &[u8]) -> Result<()> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| io_err("posicionar no PKG", e))?;
    file.write_all(data)
        .map_err(|e| io_err("gravar no PKG", e))
}

fn copy_file_at(file: &mut File, path: &Path, offset: u64, size: u64) -> Result<()> {
    let mut input = File::open(path).map_err(|e| io_err("abrir arquivo extra", e))?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| io_err("posicionar no PKG", e))?;
    let mut remaining = size;
    let mut buffer = vec![0u8; 4 * 1024 * 1024];
    while remaining > 0 {
        let want = remaining.min(buffer.len() as u64) as usize;
        let read = input
            .read(&mut buffer[..want])
            .map_err(|e| io_err("ler arquivo extra", e))?;
        if read == 0 {
            return Err(CoreError::InvalidInput(format!(
                "arquivo extra diminuiu durante o empacotamento: {}",
                path.display()
            )));
        }
        file.write_all(&buffer[..read])
            .map_err(|e| io_err("gravar arquivo extra", e))?;
        remaining -= read as u64;
    }
    Ok(())
}

fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

fn sha256_range(file: &mut File, offset: u64, len: u64) -> Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| io_err("posicionar para digest", e))?;
    let mut remaining = len;
    let mut buffer = vec![0u8; 4 * 1024 * 1024];
    while remaining > 0 {
        let want = remaining.min(buffer.len() as u64) as usize;
        let read = file
            .read(&mut buffer[..want])
            .map_err(|e| io_err("ler para digest", e))?;
        if read == 0 {
            return Err(CoreError::InvalidInput("arquivo PKG encurtou durante o cálculo de digest".into()));
        }
        hasher.update(&buffer[..read]);
        remaining -= read as u64;
    }
    Ok(hasher.finalize().into())
}

fn sha256_ranges(file: &mut File, ranges: &[(u64, u64)]) -> Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 4 * 1024 * 1024];
    for (offset, len) in ranges {
        file.seek(SeekFrom::Start(*offset))
            .map_err(|e| io_err("posicionar para digest", e))?;
        let mut remaining = *len;
        while remaining > 0 {
            let want = remaining.min(buffer.len() as u64) as usize;
            let read = file
                .read(&mut buffer[..want])
                .map_err(|e| io_err("ler para digest", e))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            remaining -= read as u64;
        }
    }
    Ok(hasher.finalize().into())
}

fn align(value: u64, alignment: u64) -> u64 {
    if value == 0 {
        0
    } else {
        (value + alignment - 1) / alignment * alignment
    }
}

fn align16(value: u64) -> u64 {
    align(value, 16)
}

fn put_be_u16(out: &mut [u8], off: usize, value: u16) {
    out[off..off + 2].copy_from_slice(&value.to_be_bytes());
}

fn put_be_u32(out: &mut [u8], off: usize, value: u32) {
    out[off..off + 4].copy_from_slice(&value.to_be_bytes());
}

fn put_be_u64(out: &mut [u8], off: usize, value: u64) {
    out[off..off + 8].copy_from_slice(&value.to_be_bytes());
}

