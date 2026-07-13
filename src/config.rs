use std::{
    collections::{HashMap, HashSet},
    env,
    fmt::Display,
};

use crate::Symbol;

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub enum Feature {
    NoStd,
    OutputMir,
    OutputInstances,
    Optimise,
    WithMirPass,
}
impl Display for Feature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(match self {
            Self::NoStd => "no-std",
            Self::Optimise => "optimise",
            Self::OutputInstances => "output-instances",
            Self::OutputMir => "output-mir",
            Self::WithMirPass => "with-mir-pass",
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
pub struct Config {
    path: String,
    features: HashMap<Feature, FeatureArgSet>,
}
impl Config {
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn arguments_for(&self, feature: Feature) -> Option<&FeatureArgSet> {
        self.features.get(&feature)
    }
    pub fn has_feature(&self, feature: Feature) -> bool {
        self.features.contains_key(&feature)
    }
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
                "with-mir-pass" => Feature::WithMirPass,
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
    Ok(Config { path, features })
}
