use std::collections::{HashSet, VecDeque};

use crate::{
    def_ids::DefId,
    mir::{BodySource, ConstValue, Constant, Context, Location, visitor::Visit},
    scheme::Scheme,
    types::{GenericArgs, GenericArgsRef},
};

type FunctionId = DefId;
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum InstanceKind {
    Function(FunctionId),
}
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct Instance {
    pub args: GenericArgs,
    pub kind: InstanceKind,
}
impl Instance {
    pub fn non_generic(kind: InstanceKind) -> Self {
        Self {
            args: GenericArgs::new(),
            kind,
        }
    }
    pub fn body_src(&self) -> BodySource {
        match self.kind {
            InstanceKind::Function(function) => BodySource::Function(function),
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
                args: GenericArgsRef<'unv>,
            }
            impl Visit for Collector<'_> {
                fn visit_constant(&mut self, _: Location, constant: &Constant) {
                    let new_instance = match constant.value {
                        ConstValue::Named(id, ref args) => Some(Instance {
                            args: args
                                .iter()
                                .cloned()
                                .map(|arg| Scheme::new(arg).bind(self.args))
                                .collect(),
                            kind: InstanceKind::Function(id),
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
            for (id, block) in body.block_info.blocks().iter_enumerated() {
                collector.visit_block(id, block);
            }
        }
        self.instances
    }
}
