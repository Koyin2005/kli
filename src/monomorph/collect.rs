use std::collections::{HashSet, VecDeque};

use crate::{
    mir::{BodySource, Constant, ConstantValue, Context, visitor::Visit},
    resolved_ast::{FunctionId, LambdaId},
    types::GenericArg,
};

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum InstanceKind {
    Lambda(LambdaId),
    Function(FunctionId),
}
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct Instance {
    pub args: Vec<GenericArg>,
    pub kind: InstanceKind,
}
impl Instance {
    pub fn non_generic(kind: InstanceKind) -> Self {
        Self {
            args: Vec::new(),
            kind,
        }
    }
    pub fn body_src(&self) -> BodySource {
        match self.kind {
            InstanceKind::Function(function) => BodySource::Function(function),
            InstanceKind::Lambda(lambda_id) => BodySource::Lambda(lambda_id),
        }
    }
}

pub struct InstanceCollector<'ctxt> {
    seen_instances: HashSet<Instance>,
    instances: Vec<Instance>,
    ctxt: &'ctxt Context,
}
impl<'ctxt> InstanceCollector<'ctxt> {
    pub fn new(context: &'ctxt Context) -> Self {
        Self {
            seen_instances: HashSet::new(),
            instances: Vec::new(),
            ctxt: context,
        }
    }
    pub fn collect(mut self, entry: Instance) -> Vec<Instance> {
        let mut unvisited = VecDeque::new();
        unvisited.push_back(entry);
        while let Some(instance) = unvisited.pop_front() {
            struct Collector<'unv> {
                v: &'unv mut VecDeque<Instance>,
                args: &'unv Vec<GenericArg>,
            }
            impl Visit for Collector<'_> {
                fn visit_constant(&mut self, constant: &Constant) {
                    let new_instance = match constant.value {
                        ConstantValue::Function(id, ref args) => Some(Instance {
                            args: args.clone(),
                            kind: InstanceKind::Function(id),
                        }),
                        ConstantValue::Lambda(id, _) => Some(Instance {
                            args: self.args.clone(),
                            kind: InstanceKind::Lambda(id),
                        }),
                        _ => None,
                    };
                    if let Some(instance) = new_instance {
                        self.v.push_back(instance);
                    }
                }
            }
            if !self.seen_instances.insert(instance.clone()) {
                continue;
            }
            self.instances.push(instance.clone());
            let body = self.ctxt.expect_body(instance.body_src());
            let mut collector = Collector {
                v: &mut unvisited,
                args: &instance.args,
            };
            for (id, block) in body.blocks.iter_enumerated() {
                collector.visit_block(id, block);
            }
        }
        self.instances
    }
}
