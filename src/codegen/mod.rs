use std::collections::HashMap;

use crate::{
    CtxtRef,
    codegen::backend_repr::{BackendRepr, backend_repr},
    index_vec::IndexVec,
    layout::{self, Align, LayoutKind, Scalar, Size},
    mir::{
        self, BasicBlockId, BinaryOp, ConstValue, Constant, Locals, Operand, OverflowOp, Place,
        PlaceBase, traversal::reachable,
    },
    monomorph::collect::{Instance, InstanceKind},
    scheme::Scheme,
    types::{self, FunctionSig, GenericArgsRef, Type},
};
use cranelift::{
    codegen::{
        self,
        ir::{
            self, AbiParam, InstBuilder, InstBuilderBase, MemFlagsData, Signature, TrapCode,
            immediates::Offset32,
        },
    },
    frontend,
};
use cranelift_module::{FuncId, Module};
mod backend_repr;

fn scalar_to_cranelift_type(scalar: layout::Scalar) -> codegen::ir::Type {
    match scalar {
        layout::Scalar::Bool | layout::Scalar::Byte => codegen::ir::types::I8,
        layout::Scalar::Int64(_) => codegen::ir::types::I64,
        layout::Scalar::Uint32 => codegen::ir::types::I32,
        layout::Scalar::Pointer { non_null: _ } => codegen::ir::types::I64,
    }
}
#[derive(PartialEq, Eq, Clone, Copy)]
enum ReturnMode {
    Void,
    ByPtr,
    ByValue(Scalar),
    ByPair(Scalar, Scalar),
}
fn signature(
    ctxt: CtxtRef<'_>,
    function_sig: &FunctionSig,
) -> (ReturnMode, codegen::ir::Signature) {
    let mut sig = codegen::ir::Signature::new(codegen::isa::CallConv::Fast);

    let return_ty_layout = ctxt.layout_of(&function_sig.return_type).unwrap();
    let return_mode = match return_ty_layout.kind {
        _ if return_ty_layout.size == Size::ZERO => ReturnMode::Void,
        LayoutKind::ScalarPair(first, second) => ReturnMode::ByPair(first, second),
        LayoutKind::Scalar(first) => {
            sig.returns
                .push(AbiParam::new(scalar_to_cranelift_type(first)));
            ReturnMode::ByValue(first)
        }
        _ => {
            sig.params.push(AbiParam::new(ir::types::I64));
            ReturnMode::ByPtr
        }
    };
    sig.params.extend(
        function_sig
            .params
            .iter()
            .flat_map(
                |param| match backend_repr(&ctxt.layout_of(param).unwrap()) {
                    BackendRepr::Scalar(scalar) => [Some(scalar_to_cranelift_type(scalar)), None],
                    BackendRepr::ScalarPair(first, second) => [
                        Some(scalar_to_cranelift_type(first)),
                        Some(scalar_to_cranelift_type(second)),
                    ],
                    BackendRepr::ZeroSized => [None, None],
                },
            )
            .flatten()
            .map(codegen::ir::AbiParam::new),
    );
    (return_mode, sig)
}
struct FunctionInfo {
    sig: Signature,
    mode: ReturnMode,
    id: FuncId,
}
struct FunctionMap {
    functions: HashMap<Instance, FunctionInfo>,
}
pub struct CodegenRoot<'c> {
    ctxt: CtxtRef<'c>,
    module: cranelift_object::ObjectModule,
    map: FunctionMap,
    instances: Vec<Instance>,
}

impl<'a> CodegenRoot<'a> {
    pub fn new<'b>(
        ctxt: CtxtRef<'a>,
        instances: impl IntoIterator<Item = Instance>,
    ) -> CodegenRoot<'a> {
        let triple = target_lexicon::Triple::host();
        let isa = codegen::isa::lookup(triple)
            .unwrap()
            .finish(codegen::settings::Flags::new(codegen::settings::builder()))
            .unwrap();
        let obj_builder = cranelift_object::ObjectBuilder::new(
            isa,
            "code",
            cranelift_module::default_libcall_names(),
        )
        .unwrap();
        let module = cranelift_object::ObjectModule::new(obj_builder);
        Self {
            ctxt: ctxt,
            map: FunctionMap {
                functions: HashMap::new(),
            },
            module,
            instances: instances.into_iter().collect(),
        }
    }

    pub fn codegen_functions(mut self, mir_ctxt: &mir::Context) -> cranelift_object::ObjectProduct {
        for (i, instance) in self.instances.iter().enumerate() {
            let name = if self
                .ctxt
                .main_function()
                .is_some_and(|(id, _)| id == instance.body_src().def_id())
            {
                "main".to_string()
            } else {
                format!("f_{i}")
            };
            let function = &mir_ctxt.bodies[&instance.body_src()];
            let sig = Scheme::new(FunctionSig::new(
                function.param_types().map(|param| param).collect(),
                function.return_type.clone(),
            ))
            .bind(&instance.args);
            let (mode, sig) = signature(self.ctxt, &sig);
            self.map.functions.insert(
                instance.clone(),
                FunctionInfo {
                    id: self
                        .module
                        .declare_function(&name, cranelift_module::Linkage::Local, &sig)
                        .unwrap(),
                    sig,
                    mode,
                },
            );
        }
        let mut ctxt = codegen::Context::new();
        let mut f_ctxt = frontend::FunctionBuilderContext::new();
        for instance in self.instances {
            let FunctionInfo {
                ref sig,
                mode: _,
                id,
                ..
            } = self.map.functions[&instance];
            let sig = sig.clone();
            ctxt.func.signature = sig;
            let body = &mir_ctxt.bodies[&instance.body_src()];
            let mut builder = frontend::FunctionBuilder::new(&mut ctxt.func, &mut f_ctxt);
            let block_map = BlockMap::new(body, &mut builder);
            FunctionCodegen::new(
                self.ctxt,
                builder,
                body,
                &instance.args,
                &mut self.module,
                &self.map,
            )
            .codgen(body, &block_map);
            println!("{:?}", ctxt.func);
            let _ = self.module.define_function(id, &mut ctxt).unwrap();
            self.module.clear_context(&mut ctxt);
        }
        self.module.finish()
    }
}
#[derive(Clone, Copy)]
enum LocalKind {
    ZeroSized,
    Memory(codegen::ir::StackSlot),
}
#[derive(Debug)]
struct OperandValue {
    ty: Type,
    kind: OperandValueKind,
}
impl OperandValue {
    pub fn force_immediate_value(
        &self,
        cg: &mut FunctionCodegen<'_, impl Module>,
    ) -> Option<codegen::ir::Value> {
        match self.kind {
            OperandValueKind::Indirect(ref place) => cg.load_place_value(place),
            OperandValueKind::ZeroSized => None,
            OperandValueKind::Value(value) => Some(value),
        }
    }
}
#[derive(Debug)]
enum OperandValueKind {
    Indirect(PlaceValue),
    ZeroSized,
    Value(codegen::ir::Value),
}
#[derive(Clone, Debug)]
struct PlaceValue {
    ty: Type,
    align: Align,
    base_ptr: codegen::ir::Value,
    offset: i32,
    scalar: Option<Scalar>,
}
struct CodegenLocalInfo {
    ty: types::Type,
    kind: LocalKind,
}
pub struct FunctionCodegen<'r, M: Module> {
    ctxt: CtxtRef<'r>,
    builder: cranelift::frontend::FunctionBuilder<'r>,
    local_info: IndexVec<mir::Local, CodegenLocalInfo>,
    return_value_info: CodegenLocalInfo,
    functions: &'r FunctionMap,
    args: GenericArgsRef<'r>,
    target_config: codegen::isa::TargetFrontendConfig,
    module: &'r mut M,
}

impl<'a, M: Module> FunctionCodegen<'a, M> {
    fn new(
        ctxt: CtxtRef<'a>,
        mut builder: cranelift::frontend::FunctionBuilder<'a>,
        body: &'a mir::Body,
        args: GenericArgsRef<'a>,
        module: &'a mut M,
        functions: &'a FunctionMap,
    ) -> Self {
        Self {
            target_config: module.target_config(),
            module,
            functions,
            ctxt: ctxt,
            local_info: body
                .locals
                .iter()
                .map(|local| {
                    let ty = Scheme::new(local.ty.clone()).bind(args);
                    let layout = ctxt.layout_of(&ty).unwrap();
                    let repr = backend_repr(&layout);
                    CodegenLocalInfo {
                        ty,
                        kind: match repr {
                            BackendRepr::ZeroSized => LocalKind::ZeroSized,
                            _ => LocalKind::Memory(builder.create_sized_stack_slot(
                                codegen::ir::StackSlotData::new(
                                    codegen::ir::StackSlotKind::ExplicitSlot,
                                    layout.size.in_bytes().try_into().unwrap(),
                                    layout.alignment.in_bytes().try_into().unwrap(),
                                ),
                            )),
                        },
                    }
                })
                .collect(),
            return_value_info: {
                let ty = Scheme::new(body.return_type.clone()).bind(args);
                let layout = ctxt.layout_of(&ty).unwrap();
                let repr = backend_repr(&layout);
                CodegenLocalInfo {
                    ty,
                    kind: match repr {
                        BackendRepr::ZeroSized => LocalKind::ZeroSized,
                        _ => LocalKind::Memory(builder.create_sized_stack_slot(
                            codegen::ir::StackSlotData::new(
                                codegen::ir::StackSlotKind::ExplicitSlot,
                                layout.size.in_bytes().try_into().unwrap(),
                                layout.alignment.in_bytes().try_into().unwrap(),
                            ),
                        )),
                    },
                }
            },
            args,
            builder,
        }
    }
    fn store_immediate(&mut self, dst_place: PlaceValue, value: ir::Value) {
        self.builder.ins().store(
            MemFlagsData::new(),
            value,
            dst_place.base_ptr,
            Offset32::new(dst_place.offset),
        );
    }
    fn store_value(&mut self, place: &mir::Place, value: OperandValue) {
        let Ok(dst_place) = self.eval_addr_of_place(place) else {
            return;
        };
        let size = self.ctxt.layout_of(&value.ty).unwrap().size;
        match value.kind {
            OperandValueKind::ZeroSized => (),
            OperandValueKind::Indirect(place) => {
                let src_offset = self.builder.ins().build_imm_const(
                    ir::types::I64,
                    ir::immediates::Imm64::new(place.offset as i64),
                    false,
                );
                let (src, _) = self.builder.ins().uadd_overflow(place.base_ptr, src_offset);
                let dst_offset = self.builder.ins().build_imm_const(
                    ir::types::I64,
                    ir::immediates::Imm64::new(dst_place.offset as i64),
                    false,
                );
                let (dst, _) = self
                    .builder
                    .ins()
                    .uadd_overflow(dst_place.base_ptr, dst_offset);
                self.builder.emit_small_memory_copy(
                    self.target_config,
                    dst,
                    src,
                    size.in_bytes(),
                    dst_place.align.in_bytes() as u8,
                    place.align.in_bytes() as u8,
                    false,
                    MemFlagsData::new(),
                );
            }
            OperandValueKind::Value(value) => {
                self.store_immediate(dst_place, value);
            }
        }
    }
    fn store_operand(&mut self, place: &mir::Place, operand: &mir::Operand) {
        let operand = self.eval_operand(operand);
        self.store_value(place, operand);
    }
    fn load_place_value(&mut self, place: &PlaceValue) -> Option<ir::Value> {
        let &PlaceValue {
            ty: _,
            align: _,
            base_ptr,
            offset,
            scalar,
        } = place;
        Some(self.builder.ins().load(
            scalar_to_cranelift_type(scalar?),
            ir::MemFlagsData::new(),
            base_ptr,
            Offset32::new(offset),
        ))
    }
    fn eval_addr_of_place(&mut self, place: &mir::Place) -> Result<PlaceValue, Type> {
        let local_info = match place.base {
            PlaceBase::Local(local) => &self.local_info[local],
            PlaceBase::ReturnPlace => &self.return_value_info,
        };
        let ty = local_info.ty.clone();
        let base_kind = local_info.kind;
        let mut ptr = match base_kind {
            LocalKind::Memory(addr) => {
                self.builder
                    .ins()
                    .stack_addr(ir::types::I64, addr, Offset32::new(0))
            }
            LocalKind::ZeroSized => return Err(ty),
        };
        let mut offset = 0;
        let mut ty = ty;
        for projection in place.projections.iter() {
            let layout = self.ctxt.layout_of(&ty).unwrap();
            (ptr, offset) = match *projection {
                mir::PlaceProjection::Field(field_id) => {
                    let offset = match layout.kind {
                        LayoutKind::Aggregate(fields) => fields[field_id].offset.in_bytes() as i32,
                        kind => unreachable!("{:?}", kind),
                    };
                    (ptr, offset)
                }
                mir::PlaceProjection::ConstantIndex(_) => todo!("Constant index"),
                mir::PlaceProjection::Index(_) => todo!("Array index"),
                mir::PlaceProjection::CaseDowncast(..) => (ptr, offset),
                mir::PlaceProjection::Deref => (
                    self.load_place_value(&PlaceValue {
                        ty: projection.apply_projection_to_type(ty.clone(), self.ctxt),
                        align: layout.alignment,
                        base_ptr: ptr,
                        offset: 0,
                        scalar: Some(Scalar::Pointer { non_null: true }),
                    })
                    .unwrap(),
                    0,
                ),
            };
            ty = projection.apply_projection_to_type(ty, self.ctxt);
        }

        Ok(PlaceValue {
            scalar: if let BackendRepr::Scalar(scalar) =
                backend_repr(&self.ctxt.layout_of(&ty).unwrap())
            {
                Some(scalar)
            } else {
                None
            },
            align: self.ctxt.layout_of(&ty).unwrap().alignment,
            ty,
            base_ptr: ptr,
            offset: offset,
        })
    }
    fn eval_operand(&mut self, operand: &mir::Operand) -> OperandValue {
        match operand {
            Operand::Constant(constant) => OperandValue {
                ty: *constant.ty.clone(),
                kind: match constant.value {
                    mir::ConstValue::ZeroSized => OperandValueKind::ZeroSized,
                    mir::ConstValue::Named(..) => todo!(),
                    mir::ConstValue::ClosureShim(..) => todo!(),
                    mir::ConstValue::Scalar(value) => {
                        let (ty, value, signed) = match &*constant.ty {
                            Type::Bool | Type::Byte => (
                                codegen::ir::types::I8,
                                codegen::ir::immediates::Imm64::new(value as i64),
                                false,
                            ),
                            Type::Int(kind) => (
                                codegen::ir::types::I64,
                                codegen::ir::immediates::Imm64::new(value as i64),
                                kind.is_signed(),
                            ),
                            _ => unreachable!(),
                        };
                        OperandValueKind::Value(
                            self.builder.ins().build_imm_const(ty, value, signed),
                        )
                    }
                    mir::ConstValue::Variant(..) => todo!(),
                    mir::ConstValue::Record(..) => todo!(),
                    mir::ConstValue::String(..) => todo!(),
                },
            },
            Operand::Load(place) => match self.eval_addr_of_place(place) {
                Ok(place) => OperandValue {
                    ty: place.ty.clone(),
                    kind: OperandValueKind::Indirect(place),
                },
                Err(ty) => OperandValue {
                    ty,
                    kind: OperandValueKind::ZeroSized,
                },
            },
        }
    }
    fn explode_args(
        &mut self,
        return_place: Option<&Place>,
        args: Vec<OperandValue>,
    ) -> Vec<ir::Value> {
        args.into_iter()
            .chain(return_place.map_or(None, |place| {
                let place = self.eval_addr_of_place(place).ok()?;
                Some(OperandValue {
                    ty: place.ty.clone(),
                    kind: OperandValueKind::Indirect(place),
                })
            }))
            .flat_map(|arg| match arg.kind {
                OperandValueKind::ZeroSized => None,
                OperandValueKind::Value(value) => Some(value),
                OperandValueKind::Indirect(value) => self.load_place_value(&value),
            })
            .collect()
    }
    fn assign(&mut self, place: &mir::Place, value: &mir::Rvalue, locals: &Locals) {
        match value {
            mir::Rvalue::Aggregate(..) => todo!("aggr"),
            mir::Rvalue::AllocateArray(_, _) => todo!("alloc array"),
            mir::Rvalue::AllocateBox(_, _) => todo!("alloc box"),
            mir::Rvalue::Use(operand) => {
                self.store_operand(place, operand);
            }
            mir::Rvalue::Call(operand, operands) => {
                let Type::Function(sig) =
                    Scheme::new(operand.type_of(self.ctxt, locals, &self.return_value_info.ty))
                        .bind(self.args)
                else {
                    unreachable!()
                };
                let callee = match operand {
                    Operand::Constant(Constant {
                        ty: _,
                        value: ConstValue::Named(id, args),
                    }) => Ok((*id, args)),
                    _ => Err(self.eval_operand(operand)),
                };
                let args: Vec<OperandValue> = operands
                    .iter()
                    .map(|operand| self.eval_operand(operand))
                    .collect();
                let (return_mode, sig) = signature(
                    self.ctxt,
                    &FunctionSig {
                        params: sig.params,
                        return_type: *sig.return_type,
                    },
                );
                let args =
                    self.explode_args((return_mode == ReturnMode::ByPtr).then(|| place), args);
                let values = match callee {
                    Ok((id, generic_args)) => {
                        let function = Instance {
                            args: generic_args.clone(),
                            kind: InstanceKind::Function(id),
                        };
                        let func = self.module.declare_func_in_func(
                            self.functions.functions[&function].id,
                            self.builder.func,
                        );
                        let call = self.builder.ins().call(func, &args);
                        self.builder.inst_results(call).to_vec()
                    }
                    Err(value) => {
                        let callee = value.force_immediate_value(self).unwrap();
                        let sig = self.builder.func.import_signature(sig);
                        let results = self.builder.ins().call_indirect(sig, callee, &args);
                        self.builder.inst_results(results).to_vec()
                    }
                };
                let Ok(place) = self.eval_addr_of_place(place) else {
                    return;
                };
                match return_mode {
                    ReturnMode::ByPtr | ReturnMode::Void => (),
                    ReturnMode::ByPair(..) => {
                        todo!("scalar pair")
                    }
                    ReturnMode::ByValue(_) => {
                        self.store_immediate(place, values[0]);
                    }
                }
            }
            mir::Rvalue::Binary(binary_op, operands) => {
                let (left, right) = &**operands;
                let left_value = self.eval_operand(left);
                let left_value = left_value.force_immediate_value(self).unwrap();
                let right_value = self.eval_operand(right);
                let right_value = right_value.force_immediate_value(self).unwrap();
                let (left, right) = match binary_op {
                    BinaryOp::Overflow(op) => {
                        let (left, right) = match op {
                            OverflowOp::Add => {
                                self.builder.ins().uadd_overflow(left_value, right_value)
                            }
                            _ => todo!("{:?}", op),
                        };
                        (left, Some(right))
                    }
                    _ => todo!("{:?}", binary_op),
                };
                if let Some(_) = right {
                    todo!("Store properly")
                } else {
                    let Type::Tuple(fields) =
                        place.type_of(self.ctxt, locals, &self.return_value_info.ty)
                    else {
                        unreachable!()
                    };
                    self.store_value(
                        place,
                        OperandValue {
                            ty: { fields }.swap_remove(0),
                            kind: OperandValueKind::Value(left),
                        },
                    );
                }
            }
            mir::Rvalue::AddrOf(_) => todo!("Get addr of"),
            mir::Rvalue::Cast(..) => todo!("Transmutation"),
            mir::Rvalue::Len(_) => todo!("Len"),
            mir::Rvalue::Discriminant(_) => todo!("Discriminant"),
        }
    }
    fn codgen(mut self, body: &'_ mir::Body, block_map: &'_ BlockMap) {
        for (id, block) in block_map.blocks.iter_enumerated() {
            let Some(bb) = *block else {
                continue;
            };
            let block = &body.block_info.blocks()[id];
            self.builder.switch_to_block(bb);
            for stmt in block.stmts.iter() {
                match &stmt.kind {
                    mir::StmtKind::Noop => (),
                    mir::StmtKind::Assign(place, rvalue) => {
                        self.assign(place, rvalue, &body.locals);
                    }
                    mir::StmtKind::Print(_) => todo!("print"),
                }
            }
            match &block.expect_terminator().kind {
                mir::TerminatorKind::Assert(operand, assert_kind, basic_block_id) => {
                    let value = self.eval_operand(operand);
                    let value = value.force_immediate_value(&mut self).unwrap();
                    let code = TrapCode::user(1).unwrap();
                    if assert_kind.negate() {
                        self.builder.ins().trapnz(value, code);
                    } else {
                        self.builder.ins().trapz(value, code);
                    }
                    self.builder
                        .ins()
                        .jump(block_map.blocks[*basic_block_id].unwrap(), &[]);
                }
                mir::TerminatorKind::Switch(..) => todo!(),
                mir::TerminatorKind::Unreachable => {
                    self.builder.ins().trap(TrapCode::user(1).unwrap());
                }
                mir::TerminatorKind::Return => {
                    let rvalue = self
                        .eval_addr_of_place(&Place::return_place())
                        .ok()
                        .and_then(|ref place| self.load_place_value(place));
                    let mode = self.functions.functions[&Instance {
                        args: self.args.iter().cloned().collect(),
                        kind: InstanceKind::Function(body.src.def_id()),
                    }]
                        .mode;
                    let rvals = match mode {
                        ReturnMode::Void => Vec::new(),
                        ReturnMode::ByPair(..) => todo!("by pair"),
                        ReturnMode::ByValue(_) => {
                            vec![rvalue.unwrap()]
                        }
                        ReturnMode::ByPtr => {
                            todo!("by ptr")
                        }
                    };
                    self.builder.ins().return_(rvals.as_slice());
                }
                mir::TerminatorKind::Goto(basic_block_id) => {
                    self.builder
                        .ins()
                        .jump(block_map.blocks[*basic_block_id].unwrap(), &[]);
                }
                mir::TerminatorKind::Panic => todo!(),
            }
        }

        self.builder.seal_all_blocks();
        self.builder.finalize(self.module.target_config());
    }
}

struct BlockMap {
    blocks: IndexVec<BasicBlockId, Option<codegen::ir::Block>>,
}
impl BlockMap {
    fn new(body: &'_ mir::Body, builder: &mut frontend::FunctionBuilder) -> Self {
        let reachable = reachable(&body.block_info);
        let block_map = IndexVec::<BasicBlockId, _>::from_iter(
            body.block_info.blocks().indices().map(|block| {
                if reachable.contains(&block) {
                    Some(builder.create_block())
                } else {
                    None
                }
            }),
        );
        Self { blocks: block_map }
    }
}
