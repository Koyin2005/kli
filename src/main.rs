use std::{collections::BTreeMap, path::Path};

use kli::{
    Symbol,
    ast::{self, Module, ModuleId},
    config::{Config, Feature, config},
    mir::{self, passes::passes},
    monomorph::collect::{Instance, InstanceCollector, InstanceKind},
    parsing::parse::Parser,
    patterns::visit::PatternCheck,
    resolve::Resolve,
    resourcecheck::ResourceCheck,
    typecheck::root::TypeCheck,
    unsafety::SafetyCheck,
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
    name: Symbol,
    kind: FileEntryKind,
}
#[derive(Debug)]
enum FileEntryKind {
    Single { src: String },
    Folder(BTreeMap<Symbol, FileEntry>),
}
struct Files {
    files: BTreeMap<Symbol, FileEntry>,
}
fn parse_source_file(id: ModuleId, name: Symbol, src: &str) -> Option<ast::Module> {
    Parser::new(name, src).parse_module(name, id).ok()
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
                kind: FileEntryKind::Single { src },
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
                    kind: FileEntryKind::Single { src },
                },
            )]),
        }
    } else {
        return Err(FileError::NotAFile);
    };
    Ok(Files { files })
}

fn parse_modules(module_counter: &mut ModuleId, entry: FileEntry) -> Option<Module> {
    let id = std::mem::replace(module_counter, module_counter.next());
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
                items: Vec::new(),
                child_modules: modules.into_iter().collect::<Option<Vec<_>>>()?,
            }
        }
        FileEntryKind::Single { src } => parse_source_file(id, name, &src)?,
    })
}
fn parse_all_modules(file_tree: Files) -> Option<Vec<Module>> {
    let module_counter = &mut { ModuleId::ROOT };
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
    let optional_file = include_str!("std/optional.kli");
    let phantom_file = include_str!("std/phantom.kli");
    let cmp_file = include_str!("std/cmp.kli");
    fn file_from(name: &str, src: &str) -> (Symbol, FileEntry) {
        let name = Symbol::intern(name);
        (
            name,
            FileEntry {
                name,
                kind: FileEntryKind::Single {
                    src: src.to_string(),
                },
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
            file_from("optional", optional_file),
            file_from("phantom", phantom_file),
            file_from("refs", ref_file),
            file_from("strings", string_file),
        ])),
    }
}
fn find_builtins() -> FileEntry {
    let builtins = include_str!("builtins.kli");
    FileEntry {
        name: Symbol::BUILTINS,
        kind: FileEntryKind::Single {
            src: builtins.to_string(),
        },
    }
}
fn build_file_tree(config: &Config) -> Result<Files, FileError> {
    let path = &config.path;
    let include_std = !config.features.contains_key(&Feature::NoStd);
    let file_tree = {
        let mut file_tree = find_all_src_files(Path::new(path))?;
        file_tree.files.insert(Symbol::BUILTINS, find_builtins());
        if include_std {
            file_tree.files.insert(Symbol::STD, find_std_lib());
        }
        file_tree
    };
    Ok(file_tree)
}
fn main() {
    let Ok(config) = config() else {
        return;
    };
    let file_tree = match build_file_tree(&config) {
        Ok(file_tree) => file_tree,
        Err(FileError::InvalidName | FileError::NotAFile) => {
            eprintln!("Invalid file or path");
            return;
        }
        Err(FileError::Io(e)) => {
            eprintln!("Unknown error : {:?}", e);
            return;
        }
    };

    let Some(modules) = parse_all_modules(file_tree) else {
        return;
    };
    let Ok(context) = Resolve::new(config).resolve(modules) else {
        return;
    };
    let ctxt = context.as_ref();
    let Ok(program) = TypeCheck::new(ctxt).check() else {
        return;
    };
    let mut had_error = false;
    for (&id, function) in program.functions.iter() {
        if let Some(ref body) = function.body {
            had_error |= PatternCheck::new(ctxt, id).check(body);
        }
        had_error |= SafetyCheck::check(ctxt, id, function).is_err();
    }
    for (id, function) in program.functions.iter() {
        had_error |= ResourceCheck::new(ctxt).check_function(*id, function);
    }
    if had_error {
        return;
    }
    let mut mir_context = mir::Context::new(true);
    for (&id, function) in program.functions.iter() {
        if ctxt.builtins().builtin_for(id).is_some() {
            continue;
        }
        mir::build::Builder::build_from_function(ctxt, &mut mir_context, id, function);
    }
    for pass in passes(ctxt.config()) {
        mir_context.for_each_body_mut(|body| {
            pass.run(ctxt, body);
        });
    }
    if ctxt
        .config()
        .features
        .contains_key(&Feature::OutputInstances)
        && let Some((main, _)) = ctxt.main_function()
    {
        let instances = InstanceCollector::new(&mir_context)
            .collect(Instance::non_generic(InstanceKind::Function(main)));
        for instance in &instances {
            println!("{:?}", instance);
        }
    }
}
