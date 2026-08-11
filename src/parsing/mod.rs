use crate::{
    Symbol,
    ast::{self, Module, ModuleId},
    files::{FileEntry, FileEntryKind, Files},
};

mod lex;
pub mod parse;
pub mod tokens;

fn parse_source_file(id: ModuleId, name: Symbol, src: &str) -> Option<ast::Module> {
    parse::Parser::new(name, src).parse_module(name, id).ok()
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

pub fn parse_all_modules(file_tree: Files) -> Option<Vec<Module>> {
    let module_counter = &mut { ModuleId::ROOT };
    let modules = file_tree
        .files
        .into_values()
        .map(|file| parse_modules(module_counter, file))
        .collect::<Vec<Option<Module>>>();
    modules.into_iter().collect()
}
