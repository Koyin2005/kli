use std::{collections::BTreeMap, env, path::Path, rc::Rc};

use kli::{
    ast, interpret::Interpret, parsing::parse::Parser, patterns::visit::PatternCheck,
    resolve::Resolve, resourcecheck::ResourceCheck, typecheck::root::TypeCheck,
};
enum ModuleError {
    Io(std::io::Error),
    InvalidModule,
}
const EXTENSION: &str = "kli";
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
    let meta = match std::fs::metadata(&path) {
        Ok(meta) => meta,
        Err(e) => match e.kind() {
            std::io::ErrorKind::NotFound => {
                eprintln!("File or folder at '{}' not found", path);
                return;
            }
            _ => {
                eprintln!("Unknown error '{:?}'", e);
                return;
            }
        },
    };
    let files = if meta.is_file() {
        if !Path::new(&path).exists() {
            eprintln!("File at '{}' not found", path);
            return;
        }
        match read_source_file(
            Path::new(&path),
            Path::new(&path)
                .file_name()
                .expect("Should be a file")
                .to_string_lossy()
                .into_owned(),
        ) {
            Ok((name, src)) => BTreeMap::from([(name, src)]),
            Err(ModuleError::InvalidModule) => {
                eprintln!("Cannot compile non kli file at {}", path);
                return;
            }
            Err(ModuleError::Io(error)) => {
                eprintln!("Unkown error : {:?}", error);
                return;
            }
        }
    } else {
        match read_source_files(path) {
            Ok(files) => files,
            Err(err) => {
                eprintln!("Unknown error '{:?}'", err);
                return;
            }
        }
    };
    let files = {
        let mut files = files;
        files.insert(Rc::from("std"), include_str!("std.kli").to_string());
        files
    };

    let mut had_error = false;
    let modules = files
        .into_iter()
        .filter_map(|(name, source)| {
            let Some(program) = parse_source_file(name, &source) else {
                had_error = true;
                return None;
            };
            if had_error {
                return None;
            };
            Some(program)
        })
        .collect::<BTreeMap<_, _>>();
    if had_error {
        return;
    }
    let Ok(program) = Resolve::new().resolve(modules) else {
        return;
    };
    let Ok(program) = TypeCheck::new(&program).check(program) else {
        return;
    };
    for function in &program.functions {
        PatternCheck::new().check(&function.body);
    }
    for function in &program.functions {
        ResourceCheck::new().check_function(function);
    }
    if let Err(e) = Interpret::new(&program.functions).interpret() {
        println!("{:?}", e)
    }
}
