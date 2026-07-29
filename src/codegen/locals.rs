use std::collections::{HashMap, hash_map::Entry};

use cranelift::{codegen, frontend};

use crate::{
    CtxtRef,
    codegen::{
        PassMode,
        backend_repr::{BackendRepr, backend_repr},
        scalar_to_cranelift_type,
    },
    index_vec::IndexVec,
    mir::{self, Local, PlaceBase, visitor::Visit},
    scheme::Scheme,
    types::{self, GenericArgsRef},
};
#[derive(Clone, Copy)]
pub enum ReturnSlot {
    Arg,
    Scalar(cranelift::frontend::Variable),
    Local(cranelift::codegen::ir::StackSlot),
    Void,
}
#[derive(Clone, Copy)]
pub enum LocalKind {
    ZeroSized,
    Scalar(cranelift::frontend::Variable),
    Memory(cranelift::codegen::ir::StackSlot),
}

pub struct Locals {
    return_slot: ReturnSlot,
    info: IndexVec<Local, CodegenLocalInfo>,
}
impl Locals {
    pub fn return_slot(&self) -> ReturnSlot {
        self.return_slot
    }
    pub fn new(
        body: &mir::Body,
        args: GenericArgsRef,
        ctxt: CtxtRef<'_>,
        builder: &mut frontend::FunctionBuilder,
        ret_mode: PassMode,
    ) -> Self {
        let ssa = Ssa::new(body);
        Self {
            return_slot: match ret_mode {
                PassMode::ByValue(single) if ssa.is_local_ssa(PlaceBase::ReturnPlace) => {
                    ReturnSlot::Scalar(
                        builder.declare_var(scalar_to_cranelift_type(single)),
                    )
                }
                PassMode::ByValue(_) => {
                    let layout = ctxt
                        .layout_of(&Scheme::new(body.return_type.clone()).bind(args))
                        .unwrap();
                    let slot = builder.create_sized_stack_slot(codegen::ir::StackSlotData::new(
                        codegen::ir::StackSlotKind::ExplicitSlot,
                        layout.size.in_bytes().try_into().unwrap(),
                        layout.alignment.pow_of_2(),
                    ));
                    ReturnSlot::Local(slot)
                }
                PassMode::ByPtr => ReturnSlot::Arg,
                PassMode::Void => ReturnSlot::Void,
            },
            info: body
                .locals
                .iter_enumerated()
                .map(|(id, local)| {
                    let ty = Scheme::new(local.ty.clone()).bind(args);
                    let layout = ctxt.layout_of(&ty).unwrap();
                    let repr = backend_repr(&layout);
                    CodegenLocalInfo {
                        ty,
                        kind: match repr {
                            BackendRepr::ZeroSized => LocalKind::ZeroSized,
                            BackendRepr::Scalar(scalar)
                                if ssa.is_local_ssa(PlaceBase::Local(id)) =>
                            {
                                LocalKind::Scalar(
                                    builder.declare_var(scalar_to_cranelift_type(scalar)),
                                )
                            }
                            _ => LocalKind::Memory(builder.create_sized_stack_slot(
                                codegen::ir::StackSlotData::new(
                                    codegen::ir::StackSlotKind::ExplicitSlot,
                                    layout.size.in_bytes().try_into().unwrap(),
                                    layout.alignment.pow_of_2(),
                                ),
                            )),
                        },
                    }
                })
                .collect(),
        }
    }
    pub fn info_for(&self, local: Local) -> &CodegenLocalInfo {
        &self.info[local]
    }
}

pub struct Ssa {
    is_ssa: IndexVec<Local, bool>,
    is_return_slot_ssa: bool,
}
impl Ssa {
    pub fn new(body: &mir::Body) -> Self {
        enum AssignCount {
            Once,
            Many,
        }
        #[derive(Default)]
        struct SsaVisitor {
            assignments: HashMap<PlaceBase, AssignCount>,
        }
        impl Visit for SsaVisitor {
            fn visit_assign(&mut self, _: mir::Location, place: &mir::Place, _: &mir::Rvalue) {
                let base = place.base;
                if !place.projections.is_empty() {
                    self.assignments.insert(base, AssignCount::Many);
                    return;
                }
                match self.assignments.entry(base) {
                    Entry::Occupied(mut occupied) => {
                        occupied.insert(AssignCount::Many);
                    }
                    Entry::Vacant(vacant) => {
                        vacant.insert(AssignCount::Once);
                    }
                }
            }
        }
        let mut visit = SsaVisitor::default();
        visit.visit_body(body);
        Self {
            is_return_slot_ssa: visit
                .assignments
                .get(&PlaceBase::ReturnPlace)
                .is_none_or(|count| matches!(count, AssignCount::Once)),
            is_ssa: body
                .locals
                .indices()
                .map(|local| {
                    let Some(AssignCount::Many) = visit.assignments.get(&PlaceBase::Local(local))
                    else {
                        return true;
                    };
                    false
                })
                .collect(),
        }
    }

    pub fn is_local_ssa(&self, base: PlaceBase) -> bool {
        match base {
            PlaceBase::Local(local) => self.is_ssa[local],
            PlaceBase::ReturnPlace => self.is_return_slot_ssa,
        }
    }
}

pub struct CodegenLocalInfo {
    pub ty: types::Type,
    pub kind: LocalKind,
}
