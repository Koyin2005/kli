use std::env;

use kli::{parsing::parse::Parser, typecheck::root::TypeCheck};

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
    let src = match std::fs::read_to_string(&path) {
        Ok(src) => src,
        Err(e) => match e.kind() {
            std::io::ErrorKind::NotFound => {
                eprintln!("File at '{}' not found", path);
                return;
            }
            _ => {
                eprintln!("Unknown error '{:?}'", e);
                return;
            }
        },
    };
    let parser = Parser::new(&src);
    let Ok(program) = parser.parse_program() else {
        return;
    };
    TypeCheck::new(&program).check(&program);
}
