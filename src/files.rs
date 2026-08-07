use std::{borrow::Cow, collections::BTreeMap, path::Path};

use crate::{
    Symbol,
    config::{Config, Feature},
};

pub fn build_file_tree(config: &Config) -> Result<Files, FileError> {
    let path = config.path();
    let include_std = !config.has_feature(Feature::NoStd);
    let file_tree = {
        let mut file_tree = find_all_src_files(path)?;
        file_tree.files.insert(Symbol::BUILTINS, find_builtins());
        if include_std {
            file_tree.files.insert(Symbol::STD, find_std_lib());
        }
        file_tree
    };
    Ok(file_tree)
}

pub enum ModuleError {
    Io(std::io::Error),
    InvalidModule,
}
#[derive(Debug)]
pub enum FileError {
    Io(std::io::Error),
    NotAFile,
    InvalidName,
}
const EXTENSION: &str = "kli";
#[derive(Debug)]
pub struct FileEntry {
    pub(super) name: Symbol,
    pub(super) kind: FileEntryKind,
}
#[derive(Debug)]
pub enum FileEntryKind {
    Single { src: Cow<'static, str> },
    Folder(BTreeMap<Symbol, FileEntry>),
}
pub struct Files {
    pub files: BTreeMap<Symbol, FileEntry>,
}
fn read_source_file(path: &Path, file_name: String) -> Result<(Symbol, String), ModuleError> {
    let mut name = file_name;
    if path
        .extension()
        .is_none_or(|ext| ext.to_str() != Some(EXTENSION))
    {
        return Err(ModuleError::InvalidModule);
    }
    name.truncate(name.len() - EXTENSION.chars().count() - 1);
    let src = std::fs::read_to_string(path).map_err(ModuleError::Io)?;
    Ok((Symbol::intern(&name), src))
}

fn find_src_files_at(path: &Path) -> Result<Vec<FileEntry>, FileError> {
    let dir = std::fs::read_dir(path).map_err(FileError::Io)?;
    let mut file_entries = Vec::new();
    for entry in dir {
        let entry = entry.map_err(FileError::Io)?;
        let metadata = entry.metadata().map_err(FileError::Io)?;
        if metadata.is_file() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let (name, src) = match read_source_file(&entry.path(), name) {
                Ok((name, src)) => (name, src),
                Err(e) => match e {
                    ModuleError::InvalidModule => continue,
                    ModuleError::Io(e) => return Err(FileError::Io(e)),
                },
            };
            file_entries.push(FileEntry {
                name,
                kind: FileEntryKind::Single { src: src.into() },
            });
        } else if metadata.is_dir() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let name = Symbol::intern(&name);
            let files = find_src_files_at(&entry.path())?;
            file_entries.push(FileEntry {
                name,
                kind: FileEntryKind::Folder(
                    files.into_iter().map(|file| (file.name, file)).collect(),
                ),
            });
        }
    }
    Ok(file_entries)
}

struct FileTree {
    files: BTreeMap<Symbol, FileEntry>,
}

fn find_all_src_files_in_dir(path: &Path) -> Result<FileTree, FileError> {
    let files = find_src_files_at(path)?;
    let files = files
        .into_iter()
        .map(|file| (file.name, file))
        .collect::<BTreeMap<_, _>>();
    Ok(FileTree { files })
}
fn find_all_src_files(path: &Path) -> Result<Files, FileError> {
    let metadata = path.metadata().map_err(FileError::Io)?;
    let name = path
        .file_name()
        .ok_or(FileError::InvalidName)?
        .to_str()
        .ok_or(FileError::InvalidName)?
        .to_string();
    let FileTree { files } = if metadata.is_dir() {
        find_all_src_files_in_dir(path)?
    } else if metadata.is_file() {
        let (name, src) = match read_source_file(path, name) {
            Ok((name, src)) => (name, src),
            Err(e) => match e {
                ModuleError::InvalidModule => return Err(FileError::InvalidName),
                ModuleError::Io(e) => return Err(FileError::Io(e)),
            },
        };
        FileTree {
            files: BTreeMap::from([(
                name,
                FileEntry {
                    name,
                    kind: FileEntryKind::Single { src: src.into() },
                },
            )]),
        }
    } else {
        return Err(FileError::NotAFile);
    };
    Ok(Files { files })
}

fn find_std_lib() -> FileEntry {
    let bool_file = include_str!("std/bools.kli");
    let int_file = include_str!("std/ints.kli");
    let io_file = include_str!("std/io.kli");
    let box_file = include_str!("std/boxed.kli");
    let ref_file = include_str!("std/refs.kli");
    let string_file = include_str!("std/strings.kli");
    let array_file = include_str!("std/arrays.kli");
    let optional_file = include_str!("std/optional.kli");
    let phantom_file = include_str!("std/phantom.kli");
    let cmp_file = include_str!("std/cmp.kli");
    let map_file = include_str!("std/maps.kli");
    let slice_file = include_str!("std/slices.kli");
    let panic_file = include_str!("std/panicking.kli");
    fn file_from(name: &str, src: &'static str) -> (Symbol, FileEntry) {
        let name = Symbol::intern(name);
        (
            name,
            FileEntry {
                name,
                kind: FileEntryKind::Single { src: src.into() },
            },
        )
    }
    FileEntry {
        name: Symbol::STD,
        kind: FileEntryKind::Folder(BTreeMap::from([
            file_from("arrays", array_file),
            file_from("bools", bool_file),
            file_from("boxed", box_file),
            file_from("cmp", cmp_file),
            file_from("ints", int_file),
            file_from("io", io_file),
            file_from("maps", map_file),
            file_from("optional", optional_file),
            file_from("phantom", phantom_file),
            file_from("refs", ref_file),
            file_from("strings", string_file),
            file_from("slices", slice_file),
            file_from("panicking", panic_file),
        ])),
    }
}
fn find_builtins() -> FileEntry {
    let builtins = include_str!("builtins.kli");
    FileEntry {
        name: Symbol::BUILTINS,
        kind: FileEntryKind::Single {
            src: builtins.into(),
        },
    }
}
