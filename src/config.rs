use std::{
    collections::{HashMap, HashSet},
    env,
};

use crate::Symbol;

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub enum Feature {
    NoStd,
    OutputMir,
    OutputInstances,
    Optimise,
}
pub struct Config {
    pub path: String,
    pub features: HashMap<Feature, HashSet<Symbol>>,
}
pub struct ConfigError;
pub fn config() -> Result<Config, ConfigError> {
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
                "optimise" => Feature::Optimise,
                _ => return None,
            };
            Some((feature, pieces.map(Symbol::intern).collect()))
        })
        .collect();
    Ok(Config { path, features })
}
