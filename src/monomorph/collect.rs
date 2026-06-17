use std::collections::{HashSet, VecDeque};

use crate::{
    mir::{BodySource, Constant, ConstantValue, Context, Operand, Rvalue, Stmt, Terminator},
    resolved_ast::{FunctionId, LambdaId},
    scheme::Scheme,
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
    fn add_new_instances_from_operand(
        &mut self,
        unvisited: &mut VecDeque<Instance>,
        operand: &Operand,
        current_args: Vec<GenericArg>,
    ) {
        let new_instance = if let Operand::Constant(Constant {
            ty: _,
            value: ConstantValue::Function(function, args),
        }) = operand
        {
            let args = args
                .clone()
                .into_iter()
                .map(|arg| Scheme::new(arg).bind(&current_args))
                .collect();
            Some(Instance {
                args,
                kind: InstanceKind::Function(*function),
            })
        } else if let Operand::Constant(Constant {
            ty: _,
            value: ConstantValue::Lambda(lambda, _),
        }) = operand
        {
            Some(Instance {
                args: current_args,
                kind: InstanceKind::Lambda(*lambda),
            })
        } else {
            None
        };
        if let Some(instance) = new_instance {
            unvisited.push_back(instance);
        }
    }
    pub fn collect(mut self, entry: Instance) -> Vec<Instance> {
        let mut unvisited = VecDeque::new();
        unvisited.push_back(entry);
        while let Some(instance) = unvisited.pop_front() {
            if !self.seen_instances.insert(instance.clone()) {
                continue;
            }
            self.instances.push(instance.clone());
            let args = instance.args.clone();
            let body = self.ctxt.expect_body(instance.body_src());
            for block in body.blocks.iter() {
                for stmt in block.stmts.iter() {
                    match stmt {
                        Stmt::Noop | Stmt::Print(None) => (),
                        Stmt::Print(Some(operand)) | Stmt::Assert(operand, _) => {
                            self.add_new_instances_from_operand(
                                &mut unvisited,
                                operand,
                                args.clone(),
                            );
                        }
                        Stmt::Assign(_, rvalue) => match rvalue {
                            Rvalue::Use(operand) => self.add_new_instances_from_operand(
                                &mut unvisited,
                                operand,
                                args.clone(),
                            ),
                            Rvalue::Aggregate(_, fields) => {
                                for field in fields {
                                    self.add_new_instances_from_operand(
                                        &mut unvisited,
                                        field,
                                        args.clone(),
                                    )
                                }
                            }
                            Rvalue::Allocate { ty: _, count } => {
                                self.add_new_instances_from_operand(
                                    &mut unvisited,
                                    count,
                                    args.clone(),
                                );
                            }
                            Rvalue::Call(operand, operands) => {
                                self.add_new_instances_from_operand(
                                    &mut unvisited,
                                    operand,
                                    args.clone(),
                                );
                                for operand in operands {
                                    self.add_new_instances_from_operand(
                                        &mut unvisited,
                                        operand,
                                        args.clone(),
                                    )
                                }
                            }
                            Rvalue::Binary(_, left_and_right) => {
                                let (left, right) = left_and_right.as_ref();
                                self.add_new_instances_from_operand(
                                    &mut unvisited,
                                    left,
                                    args.clone(),
                                );
                                self.add_new_instances_from_operand(
                                    &mut unvisited,
                                    right,
                                    args.clone(),
                                );
                            }
                            Rvalue::PointerCast(operand) => {
                                self.add_new_instances_from_operand(
                                    &mut unvisited,
                                    operand,
                                    args.clone(),
                                );
                            }
                            Rvalue::Ref(_, _) | Rvalue::Len(..) => (),
                        },
                    }
                }
                match block.expect_terminator() {
                    Terminator::Panic
                    | Terminator::Goto(_)
                    | Terminator::Return
                    | Terminator::Unreachable => (),
                    Terminator::Switch(operand, _) => {
                        self.add_new_instances_from_operand(&mut unvisited, operand, args.clone());
                    }
                }
            }
        }
        self.instances
    }
}
