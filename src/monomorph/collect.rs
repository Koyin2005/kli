use std::collections::{HashSet, VecDeque};

use crate::{
    CtxtRef,
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
pub struct Instance<'ctxt> {
    pub args: GenericArgs<'ctxt>,
    pub kind: InstanceKind,
}
impl<'ctxt> Instance<'ctxt> {
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
    seen_instances: HashSet<Instance<'ctxt>>,
    instances: Vec<Instance<'ctxt>>,
    ctxt: &'ctxt Context<'ctxt>,
}
impl<'ctxt> InstanceCollector<'ctxt> {
    pub fn new(context: &'ctxt Context) -> Self {
        Self {
            seen_instances: HashSet::new(),
            instances: Vec::new(),
            ctxt: context,
        }
    }
    pub fn collect(mut self, ctxt: CtxtRef<'ctxt>, entry: Instance<'ctxt>) -> Vec<Instance<'ctxt>> {
        let mut unvisited = VecDeque::new();
        unvisited.push_back(entry);
        while let Some(instance) = unvisited.pop_front() {
            struct Collector<'unv, 'ctxt> {
                ctxt: CtxtRef<'ctxt>,
                v: &'unv mut VecDeque<Instance<'ctxt>>,
                args: GenericArgsRef<'unv, 'ctxt>,
            }
            impl<'ctxt> Visit<'ctxt> for Collector<'_, 'ctxt> {
                fn ctxt(&self) -> crate::CtxtRef<'ctxt> {
                    self.ctxt
                }
                fn visit_constant(&mut self, _: Location, constant: &Constant<'ctxt>) {
                    let new_instance = match constant.value {
                        ConstValue::Named(id, ref args) => Some(Instance {
                            args: args
                                .iter()
                                .cloned()
                                .map(|arg| Scheme::new(arg).bind(self.ctxt(), self.args))
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
                ctxt,
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
