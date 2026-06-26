use std::{
    collections::{BTreeMap, HashSet},
    env,
    path::Path,
    rc::Rc,
};

use kli::{
    ast::{self, Module, ModuleId},
    mir,
    monomorph::collect::{Instance, InstanceCollector, InstanceKind},
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
fn parse_source_file(id: ModuleId, name: Rc<str>, src: &str) -> Option<ast::Module> {
    Parser::new(name.clone(), src).parse_module(name, id).ok()
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

struct FileTree {
    files: BTreeMap<Rc<str>, FileEntry>,
}

fn find_all_src_files_in_dir(path: &Path) -> Result<FileTree, FileError> {
    let files = find_src_files_at(path)?;
    let files = files
        .into_iter()
        .map(|file| (file.name.clone(), file))
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
                name.clone(),
                FileEntry {
                    name,
                    kind: FileEntryKind::Single { src },
                },
            )]),
        }
    } else {
        return Err(FileError::NotAFile);
    };
    Ok(Files { files })
}

fn parse_modules(module_counter: &mut u32, entry: FileEntry) -> Option<Module> {
    let id = ModuleId(std::mem::replace(module_counter, *module_counter + 1));
    let name = entry.name;
    Some(match entry.kind {
        FileEntryKind::Folder(modules) => {
            let modules = modules
                .into_values()
                .map(|file| parse_modules(module_counter, file))
                .collect::<Vec<Option<Module>>>();
            Module {
                id,
                name,
                functions: Vec::new(),
                child_modules: modules.into_iter().collect::<Option<Vec<_>>>()?,
            }
        }
        FileEntryKind::Single { src } => parse_source_file(id, name, &src)?,
    })
}
fn parse_all_modules(file_tree: Files) -> Option<Vec<Module>> {
    let module_counter = &mut 0;
    let modules = file_tree
        .files
        .into_values()
        .map(|file| parse_modules(module_counter, file))
        .collect::<Vec<Option<Module>>>();
    modules.into_iter().collect()
}
fn find_std_lib() -> FileEntry {
    let bool_file = include_str!("std/bools.kli");
    let int_file = include_str!("std/ints.kli");
    let io_file = include_str!("std/io.kli");
    let box_file = include_str!("std/boxed.kli");
    let ref_file = include_str!("std/refs.kli");
    let string_file = include_str!("std/strings.kli");
    let array_file = include_str!("std/arrays.kli");
    fn file_from(name: &str, src: &str) -> (Rc<str>, FileEntry) {
        let name: Rc<str> = Rc::from(name);
        (
            name.clone(),
            FileEntry {
                name,
                kind: FileEntryKind::Single {
                    src: src.to_string(),
                },
            },
        )
    }
    FileEntry {
        name: Rc::from("std"),
        kind: FileEntryKind::Folder(BTreeMap::from([
            file_from("bools", bool_file),
            file_from("ints", int_file),
            file_from("io", io_file),
            file_from("boxed", box_file),
            file_from("refs", ref_file),
            file_from("strings", string_file),
            file_from("arrays", array_file),
        ])),
    }
}
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub enum Feature {
    NoStd,
    OutputMir,
    OutputInstances,
}
struct Config {
    path: String,
    flags: HashSet<Feature>,
}
struct ConfigError;
fn config() -> Result<Config, ConfigError> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        eprintln!("Invalid format");
        eprintln!("Expected 'program_path' and features ");
        return Err(ConfigError);
    }
    let path = args.remove(0);
    let arg_src = args
        .into_iter()
        .fold(String::from(""), |mut output, current| {
            output.push_str(&current);
            output.push(' ');
            output
        });
    let features = arg_src
        .split("--")
        .filter_map(|src| {
            if src.is_empty() {
                return None;
            }
            let mut pieces = src.split_whitespace();
            let name = pieces.next()?;
            let feature = match name {
                "no-std" => Feature::NoStd,
                "output-mir" => Feature::OutputMir,
                "output-instances" => Feature::OutputInstances,
                _ => return None,
            };
            Some(feature)
        })
        .collect();
    Ok(Config {
        path,
        flags: features,
    })
}
fn main() {
    let Ok(config) = config() else {
        return;
    };
    let path = config.path;
    let include_std = !config.flags.contains(&Feature::NoStd);
    let file_tree = match find_all_src_files(Path::new(&path)) {
        Ok(mut file_tree) => {
            if include_std {
                file_tree.files.insert(Rc::from("std"), find_std_lib());
            }
            file_tree
        }
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
    let Ok(program) = Resolve::new().resolve(modules) else {
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
    let mut context = mir::Context::new(true);
    for function in program.functions.iter() {
        context.function_names.push(function.name.clone());
    }
    for (id, function) in program.functions.iter_enumerated() {
        mir::build::Builder::build_from_function(&mut context, id, function);
    }
    if config.flags.contains(&Feature::OutputMir) {
        for body in context.body_iter() {
            mir::dump::MirDump::new(std::io::stdout(), &context)
                .write_body(body)
                .unwrap();
        }
    }
    if config.flags.contains(&Feature::OutputInstances) {
        let instances = InstanceCollector::new(&context)
            .collect(Instance::non_generic(InstanceKind::Function(program.main)));
        for instance in &instances {
            println!("{:?}", instance);
        }
    }
}
