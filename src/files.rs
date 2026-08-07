use std::{
    borrow::Cow,
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::{
    Symbol,
    config::{Config, Feature},
};

pub fn kli_path() -> Option<PathBuf> {
    option_env!("KLI_PATH")
        .map(Path::new)
        .map(Path::to_path_buf)
}

pub fn kli_std_lib_path() -> Option<PathBuf> {
    kli_path().map(|mut path| {
        path.push("std");
        path
    })
}

pub fn kli_runtime_path() -> Option<PathBuf> {
    kli_path().map(|mut path| {
        path.push("rt");
        path
    })
}
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
    let kli_std_lib_path = kli_std_lib_path().expect("should have std lib path");
    let files = find_all_src_files(&kli_std_lib_path).unwrap();
    FileEntry {
        name: Symbol::STD,
        kind: FileEntryKind::Folder(files.files),
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
