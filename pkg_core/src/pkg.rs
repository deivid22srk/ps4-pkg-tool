use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use memmap2::Mmap;
use orbis_pfs::directory::DirEntry;
use orbis_pfs::file::File as PfsFile;
use orbis_pfs::Pfs;
use serde::Serialize;

use crate::error::{io_err, CoreError, Result};
use crate::progress::{format_eta, format_speed, Progress};
use crate::sfo::Sfo;

pub const PKG_MAGIC: u32 = 0x7F43_4E54;

#[derive(Debug, Clone)]
pub struct PkgHeader {
    pub content_id: String,
    pub pkg_type: u32,
    pub entry_count: u32,
    pub table_offset: u64,
    pub body_offset: u64,
    pub body_size: u64,
    pub drm_type: u32,
    pub content_type: u32,
    pub content_flags: u32,
    pub pfs_offset: u64,
    pub pfs_size: u64,
    pub package_size: u64,
    pub version_date: u32,
}

#[derive(Debug, Clone)]
pub struct MetaEntry {
    pub id: u32,
    pub name_offset: u32,
    pub flags1: u32,
    pub flags2: u32,
    pub data_offset: u64,
    pub data_size: u64,
}

#[derive(Debug, Serialize)]
pub struct AnalyzeInfo {
    pub ok: bool,
    pub content_id: String,
    pub title: String,
    pub title_id: String,
    pub category: String,
    pub content_type: String,
    pub pkg_size: u64,
    pub pfs_size: u64,
    pub entry_count: u32,
    pub file_count: u64,
    pub total_bytes: u64,
    pub encrypted: bool,
    pub warning: String,
}

impl PkgHeader {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 0x1000 {
            return Err(CoreError::InvalidPkg("arquivo menor que o cabeçalho".into()));
        }
        let magic = be_u32(data, 0)?;
        if magic != PKG_MAGIC {
            return Err(CoreError::InvalidPkg(format!(
                "magic 0x{magic:08X} diferente de 0x{PKG_MAGIC:08X}"
            )));
        }
        let content_id = read_cstring(&data[0x40..0x70]);
        Ok(Self {
            content_id,
            pkg_type: be_u32(data, 0x04)?,
            entry_count: be_u32(data, 0x10)?,
            table_offset: be_u32(data, 0x18)? as u64,
            body_offset: be_u64(data, 0x20)?,
            body_size: be_u64(data, 0x28)?,
            drm_type: be_u32(data, 0x70)?,
            content_type: be_u32(data, 0x74)?,
            content_flags: be_u32(data, 0x78)?,
            pfs_offset: be_u64(data, 0x410)?,
            pfs_size: be_u64(data, 0x418)?,
            package_size: be_u64(data, 0x430)?,
            version_date: be_u32(data, 0x80)?,
        })
    }

    pub fn content_type_name(&self) -> &'static str {
        match self.content_type {
            0x1A => "GD (jogo)",
            0x1B => "AC (DLC/tema)",
            0x1C => "AL (licença)",
            0x1E => "DP (patch)",
            0x01 => "GD (jogo)",
            0x02 => "AC (DLC/tema)",
            0x03 => "AL (licença)",
            0x04 => "DP (patch)",
            _ => "Desconhecido",
        }
    }
}

pub fn parse_entries(data: &[u8], header: &PkgHeader) -> Result<Vec<MetaEntry>> {
    if header.entry_count > 100_000 {
        return Err(CoreError::InvalidPkg("tabela de entradas absurda".into()));
    }
    let table = header.table_offset;
    let table_len = header.entry_count as u64 * 0x20;
    if table
        .checked_add(table_len)
        .map(|end| end > data.len() as u64)
        .unwrap_or(true)
    {
        return Err(CoreError::InvalidPkg("tabela de entradas fora do arquivo".into()));
    }
    let mut entries = Vec::with_capacity(header.entry_count as usize);
    for i in 0..header.entry_count as u64 {
        let base = (table + i * 0x20) as usize;
        let entry = MetaEntry {
            id: be_u32(data, base)?,
            name_offset: be_u32(data, base + 4)?,
            flags1: be_u32(data, base + 8)?,
            flags2: be_u32(data, base + 12)?,
            data_offset: be_u32(data, base + 16)? as u64,
            data_size: be_u32(data, base + 20)? as u64,
        };
        if entry
            .data_offset
            .checked_add(entry.data_size)
            .map(|end| end > data.len() as u64)
            .unwrap_or(true)
        {
            return Err(CoreError::InvalidPkg(format!(
                "entrada 0x{:08X} aponta para fora do arquivo",
                entry.id
            )));
        }
        entries.push(entry);
    }
    Ok(entries)
}

pub fn read_param_sfo(data: &[u8], entries: &[MetaEntry]) -> Option<Sfo> {
    let entry = entries.iter().find(|e| e.id == 0x1000)?;
    let start = entry.data_offset as usize;
    let end = start.checked_add(entry.data_size as usize)?;
    let bytes = data.get(start..end)?;
    Sfo::parse(bytes).ok()
}

pub fn analyze(path: &Path) -> Result<AnalyzeInfo> {
    let file = File::open(path).map_err(|e| io_err("abrir PKG", e))?;
    let mmap = unsafe { Mmap::map(&file) }.map_err(|e| io_err("mapear PKG", e))?;
    analyze_bytes(&mmap)
}

/// Analisa um PKG já aberto por file descriptor (usado pelo SAF no Android).
/// O fd continua pertencendo ao Kotlin e NÃO é fechado aqui.
pub fn analyze_fd(fd: std::os::unix::io::RawFd) -> Result<AnalyzeInfo> {
    use std::mem::ManuallyDrop;
    use std::os::unix::io::FromRawFd;
    let file = ManuallyDrop::new(unsafe { File::from_raw_fd(fd) });
    let mmap = unsafe { Mmap::map(&*file) }.map_err(|e| io_err("mapear descritor do PKG", e))?;
    analyze_bytes(&mmap)
}

pub fn analyze_bytes(data: &[u8]) -> Result<AnalyzeInfo> {
    let header = PkgHeader::parse(data)?;
    let entries = parse_entries(data, &header)?;
    let sfo = read_param_sfo(data, &entries);

    let mut info = AnalyzeInfo {
        ok: true,
        content_id: header.content_id.clone(),
        title: sfo
            .as_ref()
            .and_then(|s| s.get_text("TITLE"))
            .unwrap_or_default(),
        title_id: sfo
            .as_ref()
            .and_then(|s| s.get_text("TITLE_ID"))
            .unwrap_or_default(),
        category: sfo
            .as_ref()
            .and_then(|s| s.get_text("CATEGORY"))
            .unwrap_or_default(),
        content_type: header.content_type_name().to_string(),
        pkg_size: if header.package_size > 0 {
            header.package_size.min(data.len() as u64)
        } else {
            data.len() as u64
        },
        pfs_size: header.pfs_size,
        entry_count: header.entry_count,
        file_count: 0,
        total_bytes: 0,
        encrypted: false,
        warning: String::new(),
    };

    let pfs = match pfs_slice(data, &header) {
        Ok(slice) => slice,
        Err(e) => {
            info.warning = e.message();
            return Ok(info);
        }
    };

    if pfs_is_encrypted(pfs) {
        info.encrypted = true;
        info.warning =
            "PFS criptografado: a extração exige chaves que não acompanham o aplicativo.".into();
        return Ok(info);
    }

    match open_content_pfs(pfs) {
        Ok(pfs) => match collect_content(&pfs) {
            Ok(content) => {
                info.file_count = content.files.len() as u64;
                info.total_bytes = content.files.iter().map(|f| f.file.len()).sum();
            }
            Err(e) => info.warning = e.message(),
        },
        Err(e) => info.warning = e.message(),
    }

    Ok(info)
}

pub struct Content<'a> {
    pub dirs: Vec<PathBuf>,
    pub files: Vec<FileJob<'a>>,
}

pub struct FileJob<'a> {
    pub rel: PathBuf,
    pub file: PfsFile<'a>,
}

fn pfs_slice<'a>(data: &'a [u8], header: &PkgHeader) -> Result<&'a [u8]> {
    if header.pfs_size == 0 {
        return Err(CoreError::NoPfsImage);
    }
    let start = header.pfs_offset as usize;
    let end = header
        .pfs_offset
        .checked_add(header.pfs_size)
        .ok_or_else(|| CoreError::InvalidPkg("tamanho de PFS inválido".into()))? as usize;
    data.get(start..end).ok_or_else(|| {
        CoreError::InvalidPkg("imagem PFS fora dos limites do PKG".into())
    })
}

fn pfs_is_encrypted(pfs: &[u8]) -> bool {
    if pfs.len() < 0x20 {
        return false;
    }
    let mode = u16::from_le_bytes([pfs[0x1C], pfs[0x1D]]);
    mode & 0x4 != 0
}

fn open_content_pfs<'a>(image: &'a [u8]) -> Result<Arc<Pfs<'a>>> {
    let outer = orbis_pfs::open_slice(image, None)
        .map_err(|e| CoreError::InvalidPkg(format!("PFS externo: {e}")))?;

    let mut root = outer
        .root()
        .open()
        .map_err(|e| CoreError::InvalidPkg(format!("raiz PFS: {e}")))?;

    if let Some(DirEntry::Directory(dir)) = root.remove(b"uroot") {
        let mut uroot = dir
            .open()
            .map_err(|e| CoreError::InvalidPkg(format!("uroot: {e}")))?;

        if let Some(DirEntry::File(file)) = uroot.remove(b"pfs_image.dat") {
            let mut magic = [0u8; 4];
            let _ = file.read_at(0, &mut magic);
            let is_pfsc = file.is_compressed() || &magic == b"PFSC";
            let image = file.into_image();
            let inner = if is_pfsc {
                let pfsc = orbis_pfs::pfsc::PfscImage::open(image)
                    .map_err(|e| CoreError::InvalidPkg(format!("PFSC: {e}")))?;
                orbis_pfs::open_image(pfsc)
                    .map_err(|e| CoreError::InvalidPkg(format!("PFS interno: {e}")))?
            } else {
                orbis_pfs::open_image(image)
                    .map_err(|e| CoreError::InvalidPkg(format!("PFS interno: {e}")))?
            };
            return Ok(inner);
        }
    }

    Ok(outer)
}

fn collect_content<'a>(pfs: &Arc<Pfs<'a>>) -> Result<Content<'a>> {
    let root = pfs
        .root()
        .open()
        .map_err(|e| CoreError::InvalidPkg(format!("raiz PFS: {e}")))?;
    let mut content = Content {
        dirs: Vec::new(),
        files: Vec::new(),
    };

    // O conteúdo do jogo fica em /uroot. Se não houver uroot, tratamos a
    // própria raiz como conteúdo (layout simplificado).
    let mut root = root;
    if let Some(DirEntry::Directory(dir)) = root.remove(b"uroot") {
        let entries = dir
            .open()
            .map_err(|e| CoreError::InvalidPkg(format!("uroot: {e}")))?;
        walk_dir(entries, PathBuf::new(), &mut content)?;
    } else {
        walk_dir(root, PathBuf::new(), &mut content)?;
    }
    Ok(content)
}

fn walk_dir<'a>(
    entries: orbis_pfs::directory::DirEntries<'a>,
    prefix: PathBuf,
    content: &mut Content<'a>,
) -> Result<()> {
    for (name, entry) in entries {
        let component = safe_component(&name)?;
        let rel = prefix.join(component);
        match entry {
            DirEntry::Directory(dir) => {
                content.dirs.push(rel.clone());
                let sub = dir
                    .open()
                    .map_err(|e| CoreError::InvalidPkg(format!("diretório {}: {e}", rel.display())))?;
                walk_dir(sub, rel, content)?;
            }
            DirEntry::File(file) => content.files.push(FileJob { rel, file }),
            _ => {}
        }
    }
    Ok(())
}

fn safe_component(name: &[u8]) -> Result<String> {
    let value = String::from_utf8_lossy(name).to_string();
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.contains('\0')
    {
        return Err(CoreError::InvalidPkg(format!(
            "nome de arquivo inseguro: {value:?}"
        )));
    }
    Ok(value)
}

#[derive(Debug, Serialize)]
pub struct ExtractSummary {
    pub bytes: u64,
    pub file_count: u64,
    pub content_id: String,
    pub title: String,
}

/// Mantém a API simples usada pelos testes de host.
pub fn extract(path: &Path, output: &Path, progress: &mut dyn Progress) -> Result<u64> {
    Ok(extract_summary(path, output, progress)?.bytes)
}

pub fn extract_summary(
    path: &Path,
    output: &Path,
    progress: &mut dyn Progress,
) -> Result<ExtractSummary> {
    let started = Instant::now();
    progress.report(0, "Lendo cabeçalho do PKG")?;
    let file = File::open(path).map_err(|e| io_err("abrir PKG", e))?;
    let mmap = unsafe { Mmap::map(&file) }.map_err(|e| io_err("mapear PKG", e))?;
    let data = &mmap[..];
    let header = PkgHeader::parse(data)?;
    let entries = parse_entries(data, &header)?;
    let pfs = pfs_slice(data, &header)?;
    if pfs_is_encrypted(pfs) {
        return Err(CoreError::EncryptedPkg);
    }

    progress.report(2, "Abrindo sistema de arquivos PFS")?;
    let pfs = open_content_pfs(pfs)?;
    let content = collect_content(&pfs)?;
    let total: u64 = content.files.iter().map(|f| f.file.len()).sum();
    let total_files = content.files.len() as u64;

    fs::create_dir_all(output).map_err(|e| io_err("criar pasta de saída", e))?;
    for dir in &content.dirs {
        fs::create_dir_all(output.join(dir)).map_err(|e| io_err("criar subpasta", e))?;
    }

    let mut done: u64 = 0;
    let mut buffer = vec![0u8; 8 * 1024 * 1024];
    for (index, job) in content.files.iter().enumerate() {
        let out_path = output.join(&job.rel);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_err("criar pasta do arquivo", e))?;
        }
        let mut out = File::create(&out_path).map_err(|e| io_err("criar arquivo extraído", e))?;

        let mut offset = 0u64;
        let len = job.file.len();
        while offset < len {
            let want = (len - offset).min(buffer.len() as u64) as usize;
            let read = job
                .file
                .read_at(offset, &mut buffer[..want])
                .map_err(|e| io_err("ler arquivo do PFS", e))?;
            if read == 0 {
                break;
            }
            use std::io::Write;
            out.write_all(&buffer[..read])
                .map_err(|e| io_err("gravar arquivo extraído", e))?;
            offset += read as u64;
            done += read as u64;
            report_extract(progress, done, total, &job.rel, index, total_files, started)?;
        }
    }

    write_pkg_extras(data, &entries, output, progress)?;

    progress.report(100, "Extração concluída")?;
    let title = read_param_sfo(data, &entries)
        .and_then(|s| s.get_text("TITLE"))
        .unwrap_or_default();
    Ok(ExtractSummary {
        bytes: total,
        file_count: total_files,
        content_id: header.content_id,
        title,
    })
}

/// Extrai as entradas de metadados do PKG (param.sfo, ícones etc.) que não
/// fazem parte da imagem PFS. Elas são gravadas apenas se ainda não existirem,
/// para não sobrescrever arquivos vindos do PFS.
fn write_pkg_extras(
    data: &[u8],
    entries: &[MetaEntry],
    output: &Path,
    progress: &mut dyn Progress,
) -> Result<()> {
    const EXTRAS: [(u32, &str); 5] = [
        (0x1000, "sce_sys/param.sfo"),
        (0x1200, "sce_sys/icon0.png"),
        (0x1220, "sce_sys/pic0.png"),
        (0x1006, "sce_sys/pic1.png"),
        (0x1240, "sce_sys/snd0.at9"),
    ];
    for (id, rel) in EXTRAS {
        let Some(entry) = entries.iter().find(|e| e.id == id) else {
            continue;
        };
        if entry.data_size == 0 || entry.data_size > 64 * 1024 * 1024 {
            continue;
        }
        let start = entry.data_offset as usize;
        let end = match start.checked_add(entry.data_size as usize) {
            Some(end) if end <= data.len() => end,
            _ => continue,
        };
        let path = output.join(rel);
        if path.exists() {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_err("criar pasta de metadados", e))?;
        }
        progress.report(99, &format!("Extraindo {rel}"))?;
        fs::write(&path, &data[start..end]).map_err(|e| io_err("gravar metadados", e))?;
    }
    Ok(())
}

fn report_extract(
    progress: &mut dyn Progress,
    done: u64,
    total: u64,
    rel: &Path,
    index: usize,
    total_files: u64,
    started: Instant,
) -> Result<()> {
    let pct = if total == 0 {
        100
    } else {
        ((done.min(total) * 100) / total).min(100) as u8
    };
    let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or("arquivo");
    let elapsed = started.elapsed().as_secs_f64().max(0.001);
    let speed = done as f64 / elapsed;
    let remaining = total.saturating_sub(done.min(total));
    let eta = if speed > 0.01 { remaining as f64 / speed } else { 0.0 };
    progress.report(
        pct,
        &format!(
            "Extraindo {name} ({}/{total_files}) • {}/s • ETA {}",
            index + 1,
            format_speed(speed),
            format_eta(eta)
        ),
    )
}

fn be_u32(data: &[u8], off: usize) -> Result<u32> {
    data.get(off..off + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| CoreError::InvalidPkg("cabeçalho truncado".into()))
}

fn be_u64(data: &[u8], off: usize) -> Result<u64> {
    data.get(off..off + 8)
        .map(|b| u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
        .ok_or_else(|| CoreError::InvalidPkg("cabeçalho truncado".into()))
}

fn read_cstring(data: &[u8]) -> String {
    let end = data.iter().position(|b| *b == 0).unwrap_or(data.len());
    String::from_utf8_lossy(&data[..end]).trim().to_string()
}
