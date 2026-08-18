use std::collections::HashMap;

use kli::{
    Arenas,
    builtin_check::BuiltinCheck,
    config::{CommandArg, Feature, config},
    files::{FileError, build_file_tree},
    literal_check::LiteralCheck,
    mir::{self, passes::passes},
    parsing,
    patterns::visit::PatternCheck,
    resolve::Resolve,
    typecheck::root::TypeCheck,
    unsafety::SafetyCheck,
};
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

    let Some(modules) = parsing::parse_all_modules(file_tree) else {
        return;
    };
    let arenas = Arenas::default();
    let Ok(context) = Resolve::resolve(&arenas, config, modules) else {
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
        had_error |= BuiltinCheck::check(ctxt, function);
        had_error |= LiteralCheck::check(ctxt, function);
    }
    if had_error {
        return;
    }
    let mut mir_context = mir::Context::new(true);
    for (&id, function) in program.functions.iter() {
        if ctxt.builtin_for(id).is_some() {
            continue;
        }
        mir::build::Builder::build_from_function(
            ctxt,
            &mut mir_context,
            function,
            mir::BodySource::Function(id),
        );
    }
    let pass_args = ctxt
        .config()
        .arguments_for(Feature::WithMirPass)
        .map(|args| args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>())
        .unwrap_or_default();
    let run_pass = pass_args
        .iter()
        .map(|name| {
            let name_no_negate = name.strip_prefix("!");
            (
                name_no_negate.unwrap_or(name.as_str()),
                name.strip_prefix("!").is_none(),
            )
        })
        .collect::<HashMap<_, _>>();

    mir_context.for_each_body_mut(move |body| {
        for pass in passes() {
            let overidde = run_pass.get(pass.name()).copied();
            let should_run = overidde.unwrap_or_else(|| pass.enabled(ctxt));
            if !should_run {
                continue;
            }
            pass.run(ctxt, body);
        }
    });
    if let Some((..)) = ctxt.main_function()
        && !matches!(ctxt.config().command(), CommandArg::Check)
    {}
}
