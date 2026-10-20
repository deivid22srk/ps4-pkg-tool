use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{io_err, CoreError, Result};

const BLOCK_SIZE: u64 = 0x10000;
const INODE_SIZE: u64 = 0xA8;
const INODES_PER_BLOCK: u64 = BLOCK_SIZE / INODE_SIZE; // 390
const PFSC_BLOCK: u64 = 0x10000;

const MODE_FILE: u16 = 0x8000 | 0x16D;
const MODE_DIR: u16 = 0x4000 | 0x16D;
const FLAG_READONLY: u32 = 0x10;
const FLAG_COMPRESSED: u32 = 0x1;
const FLAG_INTERNAL: u32 = 0x2_0000;

#[derive(Debug, Clone, Default)]
struct Inode {
    mode: u16,
    nlink: u16,
    flags: u32,
    size: u64,
    size_compressed: u64,
    times: [u64; 4],
    nsecs: [u32; 4],
    uid: u32,
    gid: u32,
    spare: [u8; 16],
    blocks: u32,
    direct: [i32; 12],
    indirect: [i32; 5],
}

impl Inode {
    fn new(mode: u16, nlink: u16, flags: u32, size: u64) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            mode,
            nlink,
            flags,
            size,
            size_compressed: size,
            times: [now; 4],
            ..Default::default()
        }
    }

    fn serialize(&self) -> [u8; INODE_SIZE as usize] {
        let mut out = [0u8; INODE_SIZE as usize];
        put_u16(&mut out, 0x00, self.mode);
        put_u16(&mut out, 0x02, self.nlink);
        put_u32(&mut out, 0x04, self.flags);
        put_u64(&mut out, 0x08, self.size);
        put_u64(&mut out, 0x10, self.size_compressed);
        for (i, t) in self.times.iter().enumerate() {
            put_u64(&mut out, 0x18 + i * 8, *t);
        }
        for (i, n) in self.nsecs.iter().enumerate() {
            put_u32(&mut out, 0x38 + i * 4, *n);
        }
        put_u32(&mut out, 0x48, self.uid);
        put_u32(&mut out, 0x4C, self.gid);
        out[0x50..0x60].copy_from_slice(&self.spare);
        put_u32(&mut out, 0x60, self.blocks);
        for (i, b) in self.direct.iter().enumerate() {
            put_i32(&mut out, 0x64 + i * 4, *b);
        }
        for (i, b) in self.indirect.iter().enumerate() {
            put_i32(&mut out, 0x94 + i * 4, *b);
        }
        out
    }
}

#[derive(Debug, Clone)]
enum Virt {
    None,
    Pfsc {
        inner: PathBuf,
        inner_size: u64,
    },
}

#[derive(Debug, Clone)]
struct Dirent {
    ino: u32,
    ty: u32,
    name: String,
    ent_size: u32,
}

impl Dirent {
    fn new(ino: u32, ty: u32, name: impl Into<String>) -> Self {
        let name = name.into();
        let raw = name.as_bytes().len() + 17;
        let ent_size = ((raw + 7) / 8 * 8) as u32;
        Self {
            ino,
            ty,
            name,
            ent_size,
        }
    }

    fn write_to(&self, out: &mut Vec<u8>) {
        let mut buf = vec![0u8; self.ent_size as usize];
        put_u32(&mut buf, 0, self.ino);
        put_u32(&mut buf, 4, self.ty);
        put_u32(&mut buf, 8, self.name.as_bytes().len() as u32);
        put_u32(&mut buf, 12, self.ent_size);
        let name = self.name.as_bytes();
        buf[16..16 + name.len()].copy_from_slice(name);
        out.extend_from_slice(&buf);
    }
}

#[derive(Debug, Clone)]
struct Node {
    name: String,
    full_path: String,
    parent: Option<usize>,
    is_dir: bool,
    source: Option<PathBuf>,
    virt: Virt,
    children_dirs: Vec<usize>,
    children_files: Vec<usize>,
    dirents: Vec<Dirent>,
    inode_no: u32,
    size: u64,
}

#[derive(Debug)]
pub struct PackTree {
    nodes: Vec<Node>,
}

impl PackTree {
    pub fn from_directory(root: &Path, exclude: &HashSet<String>) -> Result<Self> {
        if !root.is_dir() {
            return Err(CoreError::InvalidInput(
                "a pasta de entrada não existe ou não é uma pasta".into(),
            ));
        }
        let mut nodes = vec![Node {
            name: "uroot".into(),
            full_path: String::new(),
            parent: None,
            is_dir: true,
            source: None,
            virt: Virt::None,
            children_dirs: Vec::new(),
            children_files: Vec::new(),
            dirents: Vec::new(),
            inode_no: 0,
            size: 0,
        }];
        scan_dir(&mut nodes, 0, root, "", exclude)?;
        Ok(Self { nodes })
    }

    pub fn single_virtual_file(inner: PathBuf, inner_size: u64) -> Self {
        let total = pfsc_header_size(inner_size) + inner_size;
        let nodes = vec![
            Node {
                name: "uroot".into(),
                full_path: String::new(),
                parent: None,
                is_dir: true,
                source: None,
                virt: Virt::None,
                children_dirs: Vec::new(),
                children_files: vec![1],
                dirents: Vec::new(),
                inode_no: 0,
                size: 0,
            },
            Node {
                name: "pfs_image.dat".into(),
                full_path: "/pfs_image.dat".into(),
                parent: Some(0),
                is_dir: false,
                source: None,
                virt: Virt::Pfsc { inner, inner_size },
                children_dirs: Vec::new(),
                children_files: Vec::new(),
                dirents: Vec::new(),
                inode_no: 0,
                size: total,
            },
        ];
        Self { nodes }
    }

    pub fn file_count(&self) -> u64 {
        self.nodes.iter().filter(|n| !n.is_dir).count() as u64
    }

    pub fn total_bytes(&self) -> u64 {
        self.nodes.iter().filter(|n| !n.is_dir).map(|n| n.size).sum()
    }
}

fn scan_dir(
    nodes: &mut Vec<Node>,
    parent: usize,
    path: &Path,
    rel_prefix: &str,
    exclude: &HashSet<String>,
) -> Result<()> {
    let mut items: Vec<PathBuf> = fs::read_dir(path)
        .map_err(|e| io_err("ler pasta de entrada", e))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    items.sort_by(|a, b| {
        let an = a
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let bn = b
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        an.cmp(&bn)
    });

    for item in items {
        let metadata = fs::symlink_metadata(&item).map_err(|e| io_err("ler metadados", e))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        let name = item
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.is_empty() || name == "." || name == ".." {
            continue;
        }
        let rel = if rel_prefix.is_empty() {
            name.clone()
        } else {
            format!("{rel_prefix}/{name}")
        };
        if exclude.contains(&rel) {
            continue;
        }

        let full_path = if nodes[parent].full_path.is_empty() {
            format!("/{name}")
        } else {
            format!("{}/{}", nodes[parent].full_path, name)
        };
        let idx = nodes.len();
        if metadata.is_dir() {
            nodes.push(Node {
                name,
                full_path,
                parent: Some(parent),
                is_dir: true,
                source: Some(item.clone()),
                virt: Virt::None,
                children_dirs: Vec::new(),
                children_files: Vec::new(),
                dirents: Vec::new(),
                inode_no: 0,
                size: 0,
            });
            nodes[parent].children_dirs.push(idx);
            scan_dir(nodes, idx, &item, &rel, exclude)?;
        } else if metadata.is_file() {
            let size = metadata.len();
            nodes.push(Node {
                name,
                full_path,
                parent: Some(parent),
                is_dir: false,
                source: Some(item),
                virt: Virt::None,
                children_dirs: Vec::new(),
                children_files: Vec::new(),
                dirents: Vec::new(),
                inode_no: 0,
                size,
            });
            nodes[parent].children_files.push(idx);
        }
    }
    Ok(())
}

pub struct BuiltPfs {
    nodes: Vec<Node>,
    inodes: Vec<Inode>,
    inode_count: usize,
    inode_block_count: u64,
    ndblock: u64,
    fpt_bytes: Vec<u8>,
    cr_bytes: Option<Vec<u8>>,
    super_dirents: Vec<Dirent>,
    fpt_ino: u32,
    cr_ino: Option<u32>,
    /// Ordem em que os blocos de dados foram reservados: raiz, diretórios e
    /// arquivos. A gravação precisa seguir exatamente esta ordem.
    order: Vec<usize>,
}

impl BuiltPfs {
    pub fn size(&self) -> u64 {
        self.ndblock * BLOCK_SIZE
    }

    pub fn inode_count(&self) -> usize {
        self.inode_count
    }
}

pub fn build_tree(mut tree: PackTree, min_blocks: u64) -> Result<BuiltPfs> {
    let (dirs, files) = collect_order(&tree.nodes, 0);
    let has_collision = {
        let mut seen = HashSet::new();
        dirs.iter()
            .chain(files.iter())
            .any(|idx| !seen.insert(path_hash(&tree.nodes[*idx].full_path)))
    };

    let mut inodes: Vec<Inode> = Vec::new();
    let super_no = 0u32;
    inodes.push(Inode::new(
        MODE_DIR,
        1,
        FLAG_READONLY | FLAG_INTERNAL,
        BLOCK_SIZE,
    ));
    let fpt_no = 1u32;
    inodes.push(Inode::new(
        MODE_FILE,
        1,
        FLAG_READONLY | FLAG_INTERNAL,
        0,
    ));
    let cr_no = if has_collision {
        let n = inodes.len() as u32;
        inodes.push(Inode::new(
            MODE_FILE,
            1,
            FLAG_READONLY | FLAG_INTERNAL,
            0,
        ));
        Some(n)
    } else {
        None
    };
    let root_no = inodes.len() as u32;
    inodes.push(Inode::new(MODE_DIR, 3, FLAG_READONLY, BLOCK_SIZE));
    tree.nodes[0].inode_no = root_no;
    tree.nodes[0].dirents.push(Dirent::new(root_no, 4, "."));
    tree.nodes[0].dirents.push(Dirent::new(root_no, 5, ".."));

    for idx in &dirs {
        let no = inodes.len() as u32;
        inodes.push(Inode::new(MODE_DIR, 2, FLAG_READONLY, BLOCK_SIZE));
        let parent = tree.nodes[*idx].parent.unwrap_or(0);
        let parent_inode = tree.nodes[parent].inode_no;
        tree.nodes[*idx].inode_no = no;
        tree.nodes[*idx].dirents.push(Dirent::new(no, 4, "."));
        tree.nodes[*idx]
            .dirents
            .push(Dirent::new(parent_inode, 5, ".."));
        let name = tree.nodes[*idx].name.clone();
        tree.nodes[parent]
            .dirents
            .push(Dirent::new(no, 3, name));
        inodes[tree.nodes[parent].inode_no as usize].nlink += 1;
    }

    for idx in &files {
        let no = inodes.len() as u32;
        let size = tree.nodes[*idx].size;
        let blocks = ceil_div(size, BLOCK_SIZE);
        let compressed = matches!(&tree.nodes[*idx].virt, Virt::Pfsc { .. });
        let flags = FLAG_READONLY | if compressed { FLAG_COMPRESSED } else { 0 };
        let mut inode = Inode::new(MODE_FILE, 1, flags, size);
        inode.blocks = blocks.min(u32::MAX as u64) as u32;
        inodes.push(inode);
        tree.nodes[*idx].inode_no = no;
        let parent = tree.nodes[*idx].parent.unwrap_or(0);
        let name = tree.nodes[*idx].name.clone();
        tree.nodes[parent]
            .dirents
            .push(Dirent::new(no, 2, name));
    }

    let mut super_dirents = vec![Dirent::new(fpt_no, 2, "flat_path_table")];
    if let Some(cr) = cr_no {
        super_dirents.push(Dirent::new(cr, 2, "collision_resolver"));
    }
    super_dirents.push(Dirent::new(root_no, 3, "uroot"));

    let (fpt_bytes, cr_bytes) = build_path_table(&tree.nodes, &dirs, &files, has_collision);

    let inode_count = inodes.len();
    let inode_block_count = ceil_div(inode_count as u64, INODES_PER_BLOCK);
    let mut ndblock = 1u64 + inode_block_count;

    // super-root
    inodes[super_no as usize].blocks = 1;
    inodes[super_no as usize].direct[0] = ndblock as i32;
    ndblock += 1;

    // flat path table
    let fpt_size = fpt_bytes.len() as u64;
    inodes[fpt_no as usize].size = fpt_size;
    inodes[fpt_no as usize].size_compressed = fpt_size;
    let fpt_blocks = ceil_div(fpt_size, BLOCK_SIZE);
    inodes[fpt_no as usize].blocks = fpt_blocks.min(u32::MAX as u64) as u32;
    inodes[fpt_no as usize].direct[0] = ndblock as i32;
    ndblock += 1;
    for i in 1..fpt_blocks.min(12) {
        inodes[fpt_no as usize].direct[i as usize] = ndblock as i32;
        ndblock += 1;
    }
    if fpt_blocks > 12 {
        return Err(CoreError::Unsupported(
            "árvore com arquivos demais para a tabela de caminhos".into(),
        ));
    }

    if let (Some(cr), Some(bytes)) = (cr_no, cr_bytes.as_ref()) {
        let cr_size = bytes.len() as u64;
        inodes[cr as usize].size = cr_size;
        inodes[cr as usize].size_compressed = cr_size;
        let cr_blocks = ceil_div(cr_size, BLOCK_SIZE);
        inodes[cr as usize].blocks = cr_blocks.min(u32::MAX as u64) as u32;
        inodes[cr as usize].direct[0] = ndblock as i32;
        ndblock += 1;
        for i in 1..cr_blocks.min(12) {
            inodes[cr as usize].direct[i as usize] = ndblock as i32;
            ndblock += 1;
        }
        if cr_blocks > 12 {
            return Err(CoreError::Unsupported(
                "collision_resolver grande demais".into(),
            ));
        }
    } else {
        // Bloco vazio depois da FPT, como nos DATs reais.
        ndblock += 1;
    }

    let mut order = Vec::with_capacity(1 + dirs.len() + files.len());
    order.push(0usize);
    order.extend(dirs.iter().copied());
    order.extend(files.iter().copied());

    for idx in &order {
        let idx = *idx;
        let no = tree.nodes[idx].inode_no as usize;
        let is_dir = tree.nodes[idx].is_dir;
        let blocks = if is_dir {
            dir_block_count(&tree.nodes[idx].dirents)
        } else {
            ceil_div(tree.nodes[idx].size, BLOCK_SIZE)
        };
        let inode = &mut inodes[no];
        inode.blocks = blocks.min(u32::MAX as u64) as u32;
        if is_dir {
            inode.size = blocks * BLOCK_SIZE;
            inode.size_compressed = blocks * BLOCK_SIZE;
        } else {
            inode.size = tree.nodes[idx].size;
            inode.size_compressed = match &tree.nodes[idx].virt {
                Virt::Pfsc { inner_size, .. } => *inner_size,
                Virt::None => tree.nodes[idx].size,
            };
        }
        inode.direct[0] = ndblock as i32;
        for i in 1..blocks.min(12) {
            inode.direct[i as usize] = -1;
        }
        ndblock += blocks;
    }

    ndblock = ndblock.max(min_blocks);

    Ok(BuiltPfs {
        nodes: tree.nodes,
        inodes,
        inode_count,
        inode_block_count,
        ndblock,
        fpt_bytes,
        cr_bytes,
        super_dirents,
        fpt_ino: fpt_no,
        cr_ino: cr_no,
        order,
    })
}

fn collect_order(nodes: &[Node], root: usize) -> (Vec<usize>, Vec<usize>) {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    fn walk(nodes: &[Node], idx: usize, dirs: &mut Vec<usize>, files: &mut Vec<usize>) {
        for child in &nodes[idx].children_dirs {
            dirs.push(*child);
            walk(nodes, *child, dirs, files);
        }
        for child in &nodes[idx].children_files {
            files.push(*child);
        }
    }
    walk(nodes, root, &mut dirs, &mut files);
    dirs.sort_by(|a, b| nodes[*a].full_path.cmp(&nodes[*b].full_path));
    files.sort_by(|a, b| nodes[*a].full_path.cmp(&nodes[*b].full_path));
    (dirs, files)
}

fn build_path_table(
    nodes: &[Node],
    dirs: &[usize],
    files: &[usize],
    has_collision: bool,
) -> (Vec<u8>, Option<Vec<u8>>) {
    let mut groups: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for idx in dirs.iter().chain(files.iter()) {
        groups
            .entry(path_hash(&nodes[*idx].full_path))
            .or_default()
            .push(*idx);
    }

    if !has_collision {
        let mut out = Vec::with_capacity(groups.len() * 8);
        for (hash, list) in &groups {
            let idx = list[0];
            let value = nodes[idx].inode_no | if nodes[idx].is_dir { 0x2000_0000 } else { 0 };
            out.extend_from_slice(&hash.to_le_bytes());
            out.extend_from_slice(&value.to_le_bytes());
        }
        return (out, None);
    }

    let mut fpt = Vec::with_capacity(groups.len() * 8);
    let mut cr_groups: Vec<Vec<Dirent>> = Vec::new();
    let mut offset = 0u32;
    for (hash, list) in &groups {
        let value = if list.len() > 1 {
            let group_start = offset;
            let mut entries = Vec::new();
            for idx in list {
                let d = Dirent::new(
                    nodes[*idx].inode_no,
                    if nodes[*idx].is_dir { 3 } else { 2 },
                    nodes[*idx].full_path.clone(),
                );
                offset = offset.wrapping_add(d.ent_size);
                entries.push(d);
            }
            offset = offset.wrapping_add(0x18);
            cr_groups.push(entries);
            0x8000_0000 | group_start
        } else {
            let idx = list[0];
            nodes[idx].inode_no | if nodes[idx].is_dir { 0x2000_0000 } else { 0 }
        };
        fpt.extend_from_slice(&hash.to_le_bytes());
        fpt.extend_from_slice(&value.to_le_bytes());
    }

    let mut cr = Vec::new();
    for group in cr_groups {
        for d in group {
            d.write_to(&mut cr);
        }
        cr.resize(cr.len() + 0x18, 0);
    }
    (fpt, Some(cr))
}

fn dir_block_count(dirents: &[Dirent]) -> u64 {
    let mut pos = 0u64;
    for d in dirents {
        let in_block = pos % BLOCK_SIZE;
        if in_block + d.ent_size as u64 > BLOCK_SIZE {
            pos += BLOCK_SIZE - in_block;
        }
        pos += d.ent_size as u64;
    }
    ceil_div(pos, BLOCK_SIZE)
}

pub fn pfsc_header_size(inner_size: u64) -> u64 {
    let num_blocks = ceil_div(inner_size, PFSC_BLOCK);
    let table = 8 + (num_blocks + 1) * 8;
    let extra = if table > 0xFC00 {
        ceil_div(table - 0xFC00, BLOCK_SIZE)
    } else {
        0
    };
    0x10000 + extra * BLOCK_SIZE
}

pub struct Sink<'a> {
    file: &'a mut File,
    base: u64,
}

impl<'a> Sink<'a> {
    pub fn new(file: &'a mut File, base: u64) -> Self {
        Self { file, base }
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.file
            .seek(SeekFrom::Start(self.base + offset))
            .map_err(|e| io_err("posicionar no arquivo", e))?;
        self.file
            .write_all(data)
            .map_err(|e| io_err("gravar no arquivo", e))
    }
}

impl BuiltPfs {
    pub fn write(
        &self,
        file: &mut File,
        base: u64,
        mut on_bytes: impl FnMut(u64) -> Result<()>,
    ) -> Result<()> {
        let mut sink = Sink::new(file, base);
        sink.write_at(0, &self.header_bytes())?;

        // Inodes
        let mut pos = BLOCK_SIZE;
        for inode in &self.inodes {
            sink.write_at(pos, &inode.serialize())?;
            pos += INODE_SIZE;
            let in_block = pos % BLOCK_SIZE;
            if in_block > BLOCK_SIZE - INODE_SIZE {
                pos += BLOCK_SIZE - in_block;
            }
        }

        // Dirents do super-root
        let super_start = self.inodes[0].direct[0] as u64 * BLOCK_SIZE;
        let mut buf = Vec::new();
        for d in &self.super_dirents {
            d.write_to(&mut buf);
        }
        sink.write_at(super_start, &buf)?;

        // Flat path table
        let fpt_start = self.inodes[self.fpt_ino as usize].direct[0] as u64 * BLOCK_SIZE;
        sink.write_at(fpt_start, &self.fpt_bytes)?;

        // Collision resolver
        if let (Some(cr), Some(bytes)) = (self.cr_ino, self.cr_bytes.as_ref()) {
            let start = self.inodes[cr as usize].direct[0] as u64 * BLOCK_SIZE;
            sink.write_at(start, bytes)?;
        }

        // Conteúdo dos nós, na mesma ordem em que os blocos foram reservados.
        for &idx in &self.order {
            let node = &self.nodes[idx];
            let inode = &self.inodes[node.inode_no as usize];
            if inode.blocks == 0 && inode.direct[0] <= 0 {
                continue;
            }
            let start = inode.direct[0] as u64 * BLOCK_SIZE;
            if node.is_dir {
                let mut pos = start;
                let mut buf = Vec::new();
                for d in &node.dirents {
                    let in_block = pos % BLOCK_SIZE;
                    if in_block + d.ent_size as u64 > BLOCK_SIZE {
                        pos += BLOCK_SIZE - in_block;
                    }
                    buf.clear();
                    d.write_to(&mut buf);
                    sink.write_at(pos, &buf)?;
                    pos += d.ent_size as u64;
                }
            } else {
                match &node.virt {
                    Virt::None => {
                        if let Some(source) = &node.source {
                            copy_file(source, &mut sink, start, node.size, &mut on_bytes)?;
                        }
                    }
                    Virt::Pfsc {
                        inner,
                        inner_size,
                    } => {
                        write_pfsc(inner, *inner_size, &mut sink, start, &mut on_bytes)?;
                    }
                }
            }
        }

        Ok(())
    }

    fn header_bytes(&self) -> Vec<u8> {
        let mut out = vec![0u8; 0x380];
        put_u64(&mut out, 0x00, 1);
        put_u64(&mut out, 0x08, 20130315);
        out[0x18..0x1C].copy_from_slice(&[0, 0, 1, 0]);
        put_u16(&mut out, 0x1C, 0x0008);
        put_u32(&mut out, 0x20, BLOCK_SIZE as u32);
        put_u64(&mut out, 0x28, 1);
        put_u64(&mut out, 0x30, self.inode_count as u64);
        put_u64(&mut out, 0x38, self.ndblock);
        put_u64(&mut out, 0x40, self.inode_block_count);
        put_u64(&mut out, 0x48, 0);
        // InodeBlockSig (DinodeS64): aponta para o bloco 1. Os blocos de
        // inodes são contíguos; direct[1] = -1 informa isso ao leitor.
        let inode_bytes = self.inode_block_count * BLOCK_SIZE;
        put_u16(&mut out, 0x52, 1);
        put_u32(&mut out, 0x54, 0);
        put_u64(&mut out, 0x58, inode_bytes);
        put_u64(&mut out, 0x60, inode_bytes);
        put_u64(&mut out, 0xB0, self.inode_block_count);
        put_u64(&mut out, 0xD8, 1);
        for i in 1..self.inode_block_count.min(12) {
            let block_off = 0xD8 + (i as usize) * 0x28;
            out[block_off..block_off + 8].copy_from_slice(&(-1i64).to_le_bytes());
        }
        // C# grava este índice para PFS não assinado.
        put_u32(&mut out, 0x368, 1);
        out
    }
}

fn copy_file(
    source: &Path,
    sink: &mut Sink,
    offset: u64,
    size: u64,
    on_bytes: &mut impl FnMut(u64) -> Result<()>,
) -> Result<()> {
    let mut input = File::open(source).map_err(|e| io_err("abrir arquivo de origem", e))?;
    input
        .seek(SeekFrom::Start(0))
        .map_err(|e| io_err("posicionar arquivo de origem", e))?;
    sink.file
        .seek(SeekFrom::Start(sink.base + offset))
        .map_err(|e| io_err("posicionar saída", e))?;

    let mut remaining = size;
    let mut buffer = vec![0u8; 8 * 1024 * 1024];
    while remaining > 0 {
        let want = remaining.min(buffer.len() as u64) as usize;
        let read = input
            .read(&mut buffer[..want])
            .map_err(|e| io_err("ler arquivo de origem", e))?;
        if read == 0 {
            return Err(CoreError::InvalidInput(format!(
                "arquivo diminuiu durante o empacotamento: {}",
                source.display()
            )));
        }
        sink.file
            .write_all(&buffer[..read])
            .map_err(|e| io_err("gravar arquivo no PFS", e))?;
        remaining -= read as u64;
        on_bytes(read as u64)?;
    }
    Ok(())
}

fn write_pfsc(
    inner: &Path,
    inner_size: u64,
    sink: &mut Sink,
    offset: u64,
    on_bytes: &mut impl FnMut(u64) -> Result<()>,
) -> Result<()> {
    let header_size = pfsc_header_size(inner_size);
    let num_blocks = ceil_div(inner_size, PFSC_BLOCK);
    let mut header = vec![0u8; header_size as usize];
    header[0..4].copy_from_slice(b"PFSC");
    put_u32(&mut header, 0x04, 0);
    put_u32(&mut header, 0x08, 6);
    put_u32(&mut header, 0x0C, PFSC_BLOCK as u32);
    put_u64(&mut header, 0x10, PFSC_BLOCK);
    put_u64(&mut header, 0x18, 0x400);
    put_u64(&mut header, 0x20, header_size);
    put_u64(&mut header, 0x28, num_blocks * PFSC_BLOCK);
    for i in 0..=num_blocks {
        put_u64(&mut header, (0x400 + i * 8) as usize, header_size + i * PFSC_BLOCK);
    }
    sink.write_at(offset, &header)?;

    copy_file(
        inner,
        sink,
        offset + header_size,
        inner_size,
        on_bytes,
    )
}

fn path_hash(path: &str) -> u32 {
    let mut hash = 0u32;
    for ch in path.chars() {
        let upper = ch.to_uppercase().next().unwrap_or(ch);
        hash = (upper as u32).wrapping_add(31u32.wrapping_mul(hash));
    }
    hash
}

fn ceil_div(value: u64, divisor: u64) -> u64 {
    if value == 0 {
        0
    } else {
        (value + divisor - 1) / divisor
    }
}

fn put_u16(out: &mut [u8], off: usize, value: u16) {
    out[off..off + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut [u8], off: usize, value: u32) {
    out[off..off + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_i32(out: &mut [u8], off: usize, value: i32) {
    out[off..off + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut [u8], off: usize, value: u64) {
    out[off..off + 8].copy_from_slice(&value.to_le_bytes());
}
