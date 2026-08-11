use std::{
    collections::{HashMap, HashSet},
    env,
    fmt::Display,
    path::{Path, PathBuf},
};

use crate::Symbol;

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub enum Feature {
    NoStd,
    OutputMir,
    OutputInstances,
    Optimise,
    WithMirPass,
    OutputBackendIr,
}
impl Display for Feature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(match self {
            Self::NoStd => "no-std",
            Self::Optimise => "optimise",
            Self::OutputInstances => "output-instances",
            Self::OutputMir => "output-mir",
            Self::WithMirPass => "with-mir-pass",
            Self::OutputBackendIr => "output-backend-ir",
        })
    }
}
pub struct FeatureArgSet {
    args: Vec<Symbol>,
    seen: HashSet<Symbol>,
}
impl FeatureArgSet {
    pub fn has_arg(&self, name: Symbol) -> bool {
        self.seen.contains(&name)
    }
    pub fn iter(&self) -> impl Iterator<Item = Symbol> {
        self.args.iter().copied()
    }
}
pub enum CommandArg {
    Build,
    Run,
}
pub struct Config {
    path: PathBuf,
    command: CommandArg,
    features: HashMap<Feature, FeatureArgSet>,
}
impl Config {
    pub fn path(&self) -> &'_ Path {
        self.path.as_path()
    }
    pub fn arguments_for(&self, feature: Feature) -> Option<&FeatureArgSet> {
        self.features.get(&feature)
    }
    pub fn has_feature(&self, feature: Feature) -> bool {
        self.features.contains_key(&feature)
    }
    pub fn command(&self) -> &'_ CommandArg {
        &self.command
    }
}
pub struct ConfigError;

pub fn config() -> Result<Config, ConfigError> {
    let mut args = env::args().skip(1);

    let Some(command) = args.next() else {
        eprintln!("Invalid format");
        eprintln!("Expected 'command' 'features' ");
        return Err(ConfigError);
    };

    let path = std::env::current_dir().map_err(|_| ConfigError)?;
    let command = match command.as_str() {
        "run" => CommandArg::Run,
        "build" => CommandArg::Build,
        name => {
            eprintln!("Unknown command '{name}'");
            return Err(ConfigError);
        }
    };
    let arg_src = args.fold(String::from(""), |mut output, current| {
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
                "with-mir-pass" => Feature::WithMirPass,
                "output-backend-ir" => Feature::OutputBackendIr,
                _ => return None,
            };
            let args = pieces.map(Symbol::intern).collect::<Vec<_>>();
            Some((
                feature,
                FeatureArgSet {
                    seen: args.iter().copied().collect(),
                    args,
                },
            ))
        })
        .collect();
    Ok(Config {
        path,
        features,
        command,
    })
}
