use crate::{
    CtxtRef, Symbol,
    def_ids::DefId,
    index_vec::IndexVec,
    mir::{
        self, AggregateKind, ArithOp, BasicBlockId, BitwiseOp, Body, Comparison, Operation, Reg,
        Stmt, StmtKind, TerminatorKind, Value,
        passes::{
            BodyPass,
            dataflow::{self, Analysis, Domain},
            optimisation_enabled,
        },
        visitor::MutVisit,
    },
    typed_ast::FieldId,
    types::{CaseId, GenericArgs, SimpleScalar, Type, TypeKind},
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum KnownValue<'ctxt> {
    Variant(
        DefId,
        CaseId,
        GenericArgs<'ctxt>,
        Option<Box<KnownValue<'ctxt>>>,
    ),
    Tuple(IndexVec<FieldId, KnownValue<'ctxt>>),
    Unit,
    Bool(bool),
    Int(i64),
    Zeroed(Type<'ctxt>),
    String(Symbol),
    Function(DefId, GenericArgs<'ctxt>),
    Unknown,
}
impl KnownValue<'_> {
    fn as_scalar(&self) -> Option<i64> {
        Some(match self {
            Self::Bool(value) => *value as i64,
            Self::Int(value) => *value,
            _ => return None,
        })
    }
    fn as_simple_scalar(&self) -> Option<(SimpleScalar, i64)> {
        Some(match self {
            Self::Bool(value) => (SimpleScalar::Bool, *value as i64),
            Self::Int(value) => (SimpleScalar::Int, *value),
            _ => return None,
        })
    }
    fn from_simple_scalar(kind: SimpleScalar, value: i64) -> Option<Self> {
        match kind {
            SimpleScalar::Char => None,
            SimpleScalar::Bool => Some(KnownValue::Bool(match value {
                0 => false,
                1 => true,
                _ => return None,
            })),
            SimpleScalar::Int => Some(KnownValue::Int(value)),
        }
    }
}

impl<'ctxt> Domain for Values<'ctxt> {
    fn initial<'b>(body: &Body<'b>) -> Self {
        Values::from_value(body.registers.len(), None)
    }
    fn join(&mut self, other: &Self) -> bool {
        let mut changed = false;
        for (dst, src) in self.iter_mut().zip(other) {
            changed |= match (&mut *dst, src) {
                (None, None) => false,
                (Some(_), None) | (None, Some(_)) => {
                    dst.clone_from(src);
                    true
                }
                (Some(dst_value), Some(src)) => {
                    if *src == KnownValue::Unknown {
                        false
                    } else if dst_value != src {
                        dst.clone_from(&None);
                        true
                    } else {
                        false
                    }
                }
            };
        }
        changed
    }
}
struct ConstAnalysis<'a, 'ctxt> {
    ctxt: CtxtRef<'ctxt>,
    body: &'a Body<'ctxt>,
}

impl<'ctxt> ConstAnalysis<'_, 'ctxt> {
    fn visit_terminator(
        &self,
        state: &Values<'ctxt>,
        terminator: &mir::Terminator<'ctxt>,
        propagate: impl FnMut(BasicBlockId, &Values<'ctxt>),
    ) {
        if let TerminatorKind::Goto(block, args) = &terminator.kind
            && !args.is_empty()
        {
            let mut state = state.clone();
            let regs = self.body.block_info.blocks()[*block].args.clone();
            for (value, reg) in args.iter().zip(regs) {
                let value = simplify_value(&mut state, value);
                state[reg] = value;
            }
            dataflow::prop_uniform(self, &state, terminator, propagate);
            return;
        }

        dataflow::prop_uniform(self, state, terminator, propagate);
    }
}
impl<'ctxt> Analysis<'ctxt> for ConstAnalysis<'_, 'ctxt> {
    type Domain = Values<'ctxt>;
    fn apply_stmt_effect(&mut self, state: &mut Self::Domain, stmt: &Stmt<'ctxt>) {
        apply_stmt_effect(self.ctxt, state, stmt);
    }

    fn propagate_to_basic_blocks(
        &self,
        state: &Self::Domain,
        terminator: &mir::Terminator<'ctxt>,
        propagate: impl FnMut(BasicBlockId, &Self::Domain),
    ) {
        self.visit_terminator(state, terminator, propagate);
    }
}

pub struct ConstProp;
impl<'ctxt> BodyPass<'ctxt> for ConstProp {
    fn name(&self) -> &'static str {
        "const-prop"
    }
    fn enabled(&self, ctxt: crate::CtxtRef<'ctxt>) -> bool {
        optimisation_enabled(ctxt)
    }
    fn run(&self, ctxt: crate::CtxtRef<'ctxt>, body: &'_ mut crate::mir::Body<'ctxt>) {
        let mut states = ConstAnalysis { ctxt, body }.iterate_to_fixpoint(body);
        for (block_id, block) in body
            .block_info
            .blocks_mut_dont_dirty()
            .iter_mut_enumerated()
        {
            let state = &mut states[block_id];

            for (id, stmt) in &mut block.stmts.iter_mut_enumerated() {
                apply_stmt_effect(ctxt, state, stmt);
                OperandUpdater {
                    values: state,
                    ctxt,
                }
                .visit_stmt(mir::Location::stmt(block_id, id), stmt);
            }
            OperandUpdater {
                values: state,
                ctxt,
            }
            .visit_terminator(
                mir::Location::terminator(block_id),
                block.expect_terminator_mut(),
            );
        }
    }
}
fn apply_stmt_effect<'ctxt>(ctxt: CtxtRef<'ctxt>, values: &mut Values<'ctxt>, stmt: &Stmt<'ctxt>) {
    let StmtKind::Assign(reg, rvalue) = &stmt.kind else {
        return;
    };
    let value = eval_operation(ctxt, values, rvalue);
    values[*reg] = value;
}

fn eval_operation<'ctxt>(
    ctxt: CtxtRef<'ctxt>,
    values: &Values<'ctxt>,
    operation: &Operation<'ctxt>,
) -> Option<KnownValue<'ctxt>> {
    match operation {
        Operation::Copy(value) => simplify_value(values, value),
        Operation::Cmp(op, left, right) => {
            let left = simplify_value(values, left)?.as_scalar()?;
            let right = simplify_value(values, right)?.as_scalar()?;
            match op {
                Comparison::Equals => Some(KnownValue::Bool(left == right)),
                Comparison::Greater => Some(KnownValue::Bool(left > right)),
                Comparison::Lesser => Some(KnownValue::Bool(left < right)),
            }
        }
        Operation::Zeroed(ty) => Some(match ty.kind() {
            TypeKind::Int => KnownValue::Int(0),
            TypeKind::Bool => KnownValue::Bool(false),
            TypeKind::Tuple(fields) if fields.is_empty() => KnownValue::Unit,
            _ => KnownValue::Zeroed(*ty),
        }),
        Operation::Arith(op, left, right) => {
            let left = simplify_value(values, left)?.as_scalar()?;
            let right = simplify_value(values, right)?.as_scalar()?;
            match op {
                ArithOp::AddOverflow => {
                    let (result, overflow) = left.overflowing_add(right);
                    Some(KnownValue::Tuple(IndexVec::from([
                        KnownValue::Int(result),
                        KnownValue::Bool(overflow),
                    ])))
                }
                ArithOp::SubOverflow => {
                    let (result, overflow) = left.overflowing_sub(right);
                    Some(KnownValue::Tuple(IndexVec::from([
                        KnownValue::Int(result),
                        KnownValue::Bool(overflow),
                    ])))
                }
                ArithOp::MulOverflow => {
                    let (result, overflow) = left.overflowing_mul(right);
                    Some(KnownValue::Tuple(IndexVec::from([
                        KnownValue::Int(result),
                        KnownValue::Bool(overflow),
                    ])))
                }
                ArithOp::Add => None,
                ArithOp::Sub => None,
                ArithOp::Mul => None,
                ArithOp::Divide => {
                    let result = left.checked_div(right)?;
                    Some(KnownValue::Int(result))
                }
            }
        }
        Operation::Bitwise(op, left, right) => {
            let (kind, left) = simplify_value(values, left)?.as_simple_scalar()?;
            let (_, right) = simplify_value(values, right)?.as_simple_scalar()?;
            match op {
                BitwiseOp::And => KnownValue::from_simple_scalar(kind, left & right),
                BitwiseOp::Or => KnownValue::from_simple_scalar(kind, left | right),
                BitwiseOp::ShiftLeft => KnownValue::from_simple_scalar(kind, left << right),
                BitwiseOp::ShiftRight => KnownValue::from_simple_scalar(kind, left >> right),
            }
        }
        Operation::ExtractField(value, field) => {
            let KnownValue::Tuple(fields) = simplify_value(values, value)? else {
                return None;
            };
            Some(fields[*field].clone())
        }
        Operation::Aggregate(kind, fields) => match kind {
            AggregateKind::Variant(id, case, args) => Some(KnownValue::Variant(
                *id,
                *case,
                args.clone(),
                if let Some(value) = fields
                    .iter()
                    .next()
                    .map(|value| simplify_value(values, value))
                {
                    Some(Box::new(value?))
                } else {
                    None
                },
            )),
            AggregateKind::Tuple => None,
            AggregateKind::NamedRecord(..) => None,
        },
        Operation::Discriminant(value) => {
            let KnownValue::Variant(ty_id, id, ..) = simplify_value(values, value)? else {
                return None;
            };
            Some(KnownValue::Int(
                ctxt.type_def(ty_id).case_value(id).1.into(),
            ))
        }
        Operation::ExtractPayload(value, case_id) => {
            let KnownValue::Variant(_, id, _, value) = simplify_value(values, value)? else {
                return None;
            };
            if id != *case_id {
                return None;
            }
            let Some(value) = value else {
                return None;
            };
            Some(KnownValue::Tuple(IndexVec::from([*value])))
        }
        _ => None,
    }
}

fn simplify_value<'ctxt>(
    values: &Values<'ctxt>,
    value: &Value<'ctxt>,
) -> Option<KnownValue<'ctxt>> {
    match value {
        Value::Bool(value) => Some(KnownValue::Bool(*value)),
        Value::Reg(reg) => values[*reg].clone(),
        Value::Int(value) => Some(KnownValue::Int(*value)),
        Value::Char(_) => todo!("handle chars"),
        Value::String(symbol) => Some(KnownValue::String(*symbol)),
        Value::Function(def_id, generic_args) => {
            Some(KnownValue::Function(*def_id, generic_args.clone()))
        }
        Value::Lambda(..) => todo!("lambda"),
        Value::Unit => Some(KnownValue::Unit),
        Value::Unknown(_) => Some(KnownValue::Unknown),
    }
}
fn load_value<'ctxt>(values: &Values<'ctxt>, reg: Reg) -> Option<Value<'ctxt>> {
    let value = values[reg].as_ref()?;
    match value {
        KnownValue::Unit => Some(Value::Unit),
        KnownValue::Int(value) => Some(Value::Int(*value)),
        KnownValue::Bool(value) => Some(Value::Bool(*value)),
        KnownValue::Function(def_id, args) => Some(Value::Function(*def_id, args.clone())),
        KnownValue::String(string) => Some(Value::String(*string)),
        KnownValue::Tuple(..)
        | KnownValue::Variant(..)
        | KnownValue::Zeroed(..)
        | KnownValue::Unknown => None,
    }
}
type Values<'ctxt> = IndexVec<Reg, Option<KnownValue<'ctxt>>>;

struct OperandUpdater<'a, 'ctxt> {
    values: &'a Values<'ctxt>,
    ctxt: CtxtRef<'ctxt>,
}
impl<'ctxt> MutVisit<'ctxt> for OperandUpdater<'_, 'ctxt> {
    fn visit_value(&mut self, _: mir::Location, value: &mut mir::Value<'ctxt>) {
        let Value::Reg(reg) = value else {
            return;
        };
        if let Some(result) = load_value(self.values, *reg) {
            *value = result;
        }
    }
}
