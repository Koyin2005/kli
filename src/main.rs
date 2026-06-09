use std::{collections::BTreeMap, env, path::Path, rc::Rc};

use kli::{
    ast::{self, Module},
    interpret::{Endianess, Interpret},
    parsing::parse::Parser,
    patterns::visit::PatternCheck,
    resolve::Resolve,
    resourcecheck::ResourceCheck,
    typecheck::root::TypeCheck,
};
enum ModuleError {
    Io(std::io::Error),
    InvalidModule,
}
#[derive(Debug)]
enum FileError {
    Io(std::io::Error),
    NotAFile,
    InvalidName,
}
const EXTENSION: &str = "kli";
#[derive(Debug)]
struct FileEntry {
    name: Rc<str>,
    kind: FileEntryKind,
}
#[derive(Debug)]
enum FileEntryKind {
    Single { src: String },
    Folder(BTreeMap<Rc<str>, FileEntry>),
}
struct Files {
    files: BTreeMap<Rc<str>, FileEntry>,
}
fn parse_source_file(name: Rc<str>, src: &str) -> Option<(Rc<str>, ast::Module)> {
    Some((name.clone(), Parser::new(name, src).parse_module().ok()?))
}
fn read_source_file(path: &Path, file_name: String) -> Result<(Rc<str>, String), ModuleError> {
    let mut name = file_name;
    if path
        .extension()
        .is_none_or(|ext| ext.to_str() != Some(EXTENSION))
    {
        return Err(ModuleError::InvalidModule);
    }
    name.truncate(name.len() - EXTENSION.chars().count() - 1);
    let src = std::fs::read_to_string(path).map_err(ModuleError::Io)?;
    Ok((name.into(), src))
}
fn read_source_files(path: String) -> std::io::Result<BTreeMap<Rc<str>, String>> {
    let dir = std::fs::read_dir(&path)?;
    let mut files = BTreeMap::default();
    for entry in dir {
        let entry = entry?;
        if entry.metadata()?.is_file() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let (name, src) = match read_source_file(&entry.path(), name) {
                Ok((name, src)) => (name, src),
                Err(e) => match e {
                    ModuleError::InvalidModule => continue,
                    ModuleError::Io(e) => return Err(e),
                },
            };
            files.insert(name, src);
        }
    }
    Ok(files)
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
                kind: FileEntryKind::Single { src },
            });
        } else if metadata.is_dir() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let name: Rc<str> = name.into();
            let files = find_src_files_at(&entry.path())?;
            file_entries.push(FileEntry {
                name: name.clone(),
                kind: FileEntryKind::Folder(
                    files
                        .into_iter()
                        .map(|file| (file.name.clone(), file))
                        .collect(),
                ),
            });
        }
    }
    Ok(file_entries)
}
fn find_all_src_files_in_dir(
    name: String,
    path: &Path,
) -> Result<(Rc<str>, BTreeMap<Rc<str>, FileEntry>), FileError> {
    let files = find_src_files_at(path)?;
    let files = files
        .into_iter()
        .map(|file| (file.name.clone(), file))
        .collect::<BTreeMap<_, _>>();
    Ok((name.into(), files))
}
fn find_all_src_files(path: &Path) -> Result<Files, FileError> {
    let metadata = path.metadata().map_err(FileError::Io)?;
    let name = path
        .file_name()
        .ok_or(FileError::InvalidName)?
        .to_str()
        .ok_or(FileError::InvalidName)?
        .to_string();
    let (_, files) = if metadata.is_dir() {
        find_all_src_files_in_dir(name, path)?
    } else if metadata.is_file() {
        let (name, src) = match read_source_file(path, name) {
            Ok((name, src)) => (name, src),
            Err(e) => match e {
                ModuleError::InvalidModule => return Err(FileError::InvalidName),
                ModuleError::Io(e) => return Err(FileError::Io(e)),
            },
        };
        (
            name.clone(),
            BTreeMap::from([(
                name.clone(),
                FileEntry {
                    name,
                    kind: FileEntryKind::Single { src },
                },
            )]),
        )
    } else {
        return Err(FileError::NotAFile);
    };
    Ok(Files { files })
}

fn parse_modules(entry: FileEntry) -> Option<Module> {
    let name = entry.name;
    Some(match entry.kind {
        FileEntryKind::Folder(modules) => {
            let modules = modules
                .into_iter()
                .map(|(_, file)| parse_modules(file))
                .collect::<Vec<Option<Module>>>();
            Module {
                functions: Vec::new(),
                child_modules: modules
                    .into_iter()
                    .map(std::convert::identity)
                    .collect::<Option<Vec<_>>>()?,
            }
        }
        FileEntryKind::Single { src } => {
            let Some((_, module)) = parse_source_file(name, &src) else {
                return None;
            };
            module
        }
    })
}
fn parse_all_modules(file_tree: Files) -> Option<Vec<Module>> {
    let modules = file_tree
        .files
        .into_iter()
        .map(|(_, file)| parse_modules(file))
        .collect::<Vec<Option<Module>>>();
    modules.into_iter().map(std::convert::identity).collect()
}
fn find_std_lib() -> FileEntry{
    let bool_file = include_str!("std/bool.kli");
    let intrinsics_file = include_str!("std/intrinsics.kli");
    let io_file = include_str!("std/io.kli");
    fn file_from(name:&str,src: &str) -> (Rc<str>,FileEntry){
        let name: Rc<str> = Rc::from(name);
        (name.clone(),FileEntry{
            name,
            kind:FileEntryKind::Single { src: src.to_string() }
        })
    }
    FileEntry { name: Rc::from("std"), kind: FileEntryKind::Folder(BTreeMap::from([
        file_from("bool", bool_file),
        file_from("intrinsics", intrinsics_file),
        file_from("io", io_file),
    ])) }

}
fn main() {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    let path = if args.len() == 1
        && let Some(name) = args.pop()
    {
        name
    } else {
        eprintln!("Invalid format");
        eprintln!("Expected 'program_path'");
        return;
    };
    let file_tree = match find_all_src_files(Path::new(&path)) {
        Ok(mut file_tree) => {
            file_tree.files.insert(Rc::from("std"), find_std_lib());
            file_tree
        },
        Err(e) => match e {
            FileError::InvalidName | FileError::NotAFile => {
                eprintln!("Invalid file or path");
                return;
            }
            FileError::Io(e) => {
                eprintln!("Unknown error : {:?}", e);
                return;
            }
        },
    };
    let Some(modules) = parse_all_modules(file_tree) else {
        return;
    };
    let Ok(program) = Resolve::new().resolve(BTreeMap::new()) else {
        return;
    };
    let Ok(program) = TypeCheck::new(&program).check(program) else {
        return;
    };
    let mut had_error = false;
    for function in &program.functions {
        if let Some(ref body) = function.body {
            had_error |= PatternCheck::new().check(body);
        }
    }
    for function in &program.functions {
        had_error |= ResourceCheck::new().check_function(function);
    }
    if had_error {
        return;
    }
    if let Err(e) = Interpret::new(Endianess::Big, &program.functions).interpret() {
        println!("{:?}", e)
    }
}
