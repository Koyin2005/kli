use std::{cell::Cell, collections::HashMap};

use crate::{
    CtxtRef,
    codegen::backend_repr::{BackendRepr, backend_repr},
    index_vec::IndexVec,
    layout::{self, Align, LayoutKind, Scalar, Size, TagEncoding},
    mir::{
        self, AggregateKind, BasicBlockId, BinaryOp, ConstValue, Constant, Locals, Operand,
        OverflowOp, Place, PlaceBase, traversal::reachable,
    },
    monomorph::collect::{Instance, InstanceKind},
    scheme::Scheme,
    typed_ast::FieldId,
    types::{self, CaseId, FunctionSig, GenericArgsRef, IntegerKind, Type},
};
use cranelift::{
    codegen::{
        self,
        ir::{
            self, AbiParam, InstBuilder, InstBuilderBase, MemFlagsData, Signature, TrapCode,
            immediates::Offset32,
        },
        settings::Configurable,
    },
    frontend,
};
use cranelift_module::{FuncId, Module};
mod backend_repr;

fn scalar_to_cranelift_type(scalar: layout::Scalar) -> codegen::ir::Type {
    match scalar {
        layout::Scalar::Bool | layout::Scalar::Byte => codegen::ir::types::I8,
        layout::Scalar::Int64(_) => PTR_IR_TYPE,
        layout::Scalar::Uint32 => codegen::ir::types::I32,
        layout::Scalar::Pointer { non_null: _ } => codegen::ir::types::I64,
    }
}
fn pass_mode_to_cranelift_types(mode: PassMode) -> impl Iterator<Item = codegen::ir::Type> {
    match mode {
        PassMode::ByPair(first, second, _) => [
            Some(scalar_to_cranelift_type(first)),
            Some(scalar_to_cranelift_type(second)),
        ],
        PassMode::Void => [None, None],
        PassMode::ByPtr => [Some(PTR_IR_TYPE), None],
        PassMode::ByValue(scalar) => [Some(scalar_to_cranelift_type(scalar)), None],
    }
    .into_iter()
    .flatten()
}
const PTR_IR_TYPE: codegen::ir::Type = codegen::ir::types::I64;
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum PassMode {
    Void,
    ByPtr,
    ByValue(Scalar),
    ByPair(Scalar, Scalar, Size),
}
impl PassMode {
    const fn new(repr: BackendRepr) -> PassMode {
        match repr {
            BackendRepr::Memory => PassMode::ByPtr,
            BackendRepr::Scalar(scalar) => PassMode::ByValue(scalar),
            BackendRepr::ZeroSized => PassMode::Void,
            BackendRepr::ScalarPair {
                first,
                second,
                second_offset,
            } => PassMode::ByPair(first, second, second_offset),
        }
    }
}
fn call_abi(ctxt: CtxtRef<'_>, function_sig: &FunctionSig) -> CallAbi {
    let params = function_sig
        .params
        .iter()
        .map(|param| PassMode::new(backend_repr(&ctxt.layout_of(param).unwrap())))
        .collect();
    let ret = PassMode::new(backend_repr(
        &ctxt.layout_of(&function_sig.return_type).unwrap(),
    ));
    CallAbi { params, ret }
}
fn signature(
    abi: &CallAbi,
    target_config: codegen::isa::TargetFrontendConfig,
) -> codegen::ir::Signature {
    let mut sig = codegen::ir::Signature::new(target_config.default_call_conv);
    if matches!(abi.ret, PassMode::ByPtr) {
        sig.params.push(AbiParam::special(
            PTR_IR_TYPE,
            ir::ArgumentPurpose::StructReturn,
        ));
    }
    for param in abi.params.iter() {
        sig.params
            .extend(pass_mode_to_cranelift_types(*param).map(AbiParam::new));
    }
    if !matches!(abi.ret, PassMode::ByPtr) {
        sig.returns
            .extend(pass_mode_to_cranelift_types(abi.ret).map(AbiParam::new));
    }
    sig
}
struct CallAbi {
    params: Vec<PassMode>,
    ret: PassMode,
}
struct FunctionInfo {
    sig: Signature,
    abi: CallAbi,

    id: FuncId,
}
struct RuntimeFunctions {
    panic: FuncId,
    alloc: FuncId,
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
    pub fn new(
        ctxt: CtxtRef<'a>,
        instances: impl IntoIterator<Item = Instance>,
    ) -> CodegenRoot<'a> {
        let triple = target_lexicon::Triple::host();
        let mut builder = codegen::settings::builder();
        builder.set("opt_level", "speed_and_size").unwrap();
        let isa = codegen::isa::lookup(triple)
            .unwrap()
            .finish(codegen::settings::Flags::new(builder))
            .unwrap();
        let obj_builder = cranelift_object::ObjectBuilder::new(
            isa,
            "code",
            cranelift_module::default_libcall_names(),
        )
        .unwrap();
        let module = cranelift_object::ObjectModule::new(obj_builder);
        Self {
            ctxt,
            map: FunctionMap {
                functions: HashMap::new(),
            },
            module,
            instances: instances.into_iter().collect(),
        }
    }
    fn declare_function(
        &mut self,
        name: &str,
        linkage: cranelift_module::Linkage,
        sig: &ir::Signature,
    ) -> FuncId {
        self.module.declare_function(name, linkage, sig).unwrap()
    }
    fn make_function(
        &mut self,
        ctxt: &mut codegen::Context,
        f_ctxt: &mut frontend::FunctionBuilderContext,
        name: &str,
        linkage: cranelift_module::Linkage,
        sig: &ir::Signature,
        build_function: impl FnOnce(&mut frontend::FunctionBuilder, ir::Block),
    ) -> FuncId {
        let function = self.declare_function(name, linkage, sig);
        {
            let mut builder = frontend::FunctionBuilder::new(&mut ctxt.func, f_ctxt);
            let entry = builder.create_block();
            builder.switch_to_block(entry);
            build_function(&mut builder, entry);
            builder.finalize(self.module.target_config());
            println!("{:?}", ctxt.func);
            self.module.define_function(function, ctxt).unwrap();
            self.module.clear_context(ctxt);
        }
        function
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
                format!("f_{}_{i}", self.ctxt.display(instance.body_src().def_id()))
            };
            let function = &mir_ctxt.bodies[&instance.body_src()];
            let sig = Scheme::new(FunctionSig::new(
                function.param_types().collect(),
                function.return_type.clone(),
            ))
            .bind(&instance.args);
            let abi = call_abi(self.ctxt, &sig);
            let sig = signature(&abi, self.module.target_config());
            self.map.functions.insert(
                instance.clone(),
                FunctionInfo {
                    id: self
                        .module
                        .declare_function(&name, cranelift_module::Linkage::Local, &sig)
                        .unwrap(),
                    sig,
                    abi,
                },
            );
        }
        let mut ctxt = codegen::Context::new();
        let mut f_ctxt = frontend::FunctionBuilderContext::new();
        //Declare panic function
        let panic_function = self.make_function(
            &mut ctxt,
            &mut f_ctxt,
            "panic",
            cranelift_module::Linkage::Export,
            &ir::Signature::new(self.module.target_config().default_call_conv),
            |builder, entry| {
                builder.ins().trap(TrapCode::user(1).unwrap());
                builder.seal_block(entry);
            },
        );
        let allocate_function =
            self.declare_function("malloc", cranelift_module::Linkage::Import, &{
                let mut sig = ir::Signature::new(self.module.target_config().default_call_conv);
                sig.params.push(AbiParam::new(ir::types::I64));
                sig.returns.push(AbiParam::new(PTR_IR_TYPE));
                sig
            });

        let runtime = RuntimeFunctions {
            panic: panic_function,
            alloc: allocate_function,
        };

        for instance in self.instances {
            let FunctionInfo {
                ref sig,
                ref abi,
                id,
                ..
            } = self.map.functions[&instance];
            {
                let sig = sig.clone();
                ctxt.func.signature = sig;
            }
            let body = &mir_ctxt.bodies[&instance.body_src()];
            let mut builder = frontend::FunctionBuilder::new(&mut ctxt.func, &mut f_ctxt);
            let block_map = BlockMap::new(body, &mut builder);
            for param in sig.params.iter() {
                builder.append_block_param(block_map.entry(), param.value_type);
            }
            FunctionCodegen::new(
                self.ctxt,
                builder,
                body,
                &instance.args,
                &mut self.module,
                &self.map,
                abi,
                &block_map,
                &runtime,
            )
            .codgen(body);
            println!("{:?}", ctxt.func);
            self.module.define_function(id, &mut ctxt).unwrap();
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
#[derive(Debug, Clone)]
struct OperandValue {
    ty: Type,
    kind: OperandValueKind,
}
impl OperandValue {
    pub fn force_immediate_value(
        &self,
        cg: &mut FunctionCodegen<'_, impl Module>,
    ) -> Option<ScalarValue> {
        match self.kind {
            OperandValueKind::Indirect(ref place) => cg.load_place_value(place),
            OperandValueKind::ZeroSized => None,
            OperandValueKind::Value(value) => Some(value),
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub(super) enum ScalarValue {
    Single(codegen::ir::Value),
    Pair([codegen::ir::Value; 2], Size),
}
impl ScalarValue {
    pub const fn pair(first: codegen::ir::Value, second: codegen::ir::Value, offset: Size) -> Self {
        Self::Pair([first, second], offset)
    }
    pub fn first_value(&self) -> codegen::ir::Value {
        match *self {
            Self::Pair([first, _], ..) => first,
            Self::Single(value) => value,
        }
    }
    pub fn into_iter(self) -> impl Iterator<Item = codegen::ir::Value> {
        let (first, second) = match self {
            Self::Pair([first, second], _) => (first, Some(second)),
            Self::Single(value) => (value, None),
        };
        std::iter::once(first).chain(second)
    }
    pub fn as_slice(&self) -> &[codegen::ir::Value] {
        match self {
            Self::Pair(values, _) => values,
            ScalarValue::Single(value) => std::slice::from_ref(value),
        }
    }
}
#[derive(Debug, Clone)]
enum OperandValueKind {
    Indirect(PlaceValue),
    ZeroSized,
    Value(ScalarValue),
}
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
enum ScalarType {
    Single(Scalar),
    Pair(Scalar, Scalar, Size),
}
#[derive(Clone, Debug)]
struct PlaceValue {
    ty: Type,
    layout: layout::Layout,
    base_ptr: codegen::ir::Value,
    offset: i32,
    scalar: Option<ScalarType>,
}
impl PlaceValue {
    fn new(ptr: codegen::ir::Value, layout: layout::Layout, ty: Type) -> Self {
        Self::new_with_offset(ptr, layout, ty, 0)
    }
    fn new_with_offset(
        ptr: codegen::ir::Value,
        layout: layout::Layout,
        ty: Type,
        offset: i32,
    ) -> Self {
        Self {
            scalar: match backend_repr(&layout) {
                BackendRepr::Scalar(scalar) => Some(ScalarType::Single(scalar)),
                BackendRepr::ScalarPair {
                    first,
                    second,
                    second_offset,
                } => Some(ScalarType::Pair(first, second, second_offset)),
                _ => None,
            },
            layout,
            ty,
            base_ptr: ptr,
            offset,
        }
    }
    fn align(&self) -> Align {
        self.layout.alignment
    }
    fn ptr_and_offset(&self) -> (codegen::ir::Value, i32) {
        (self.base_ptr, self.offset)
    }
    fn project_downcast(self, ctxt: CtxtRef<'_>, case: CaseId) -> Self {
        let Type::Named(id, _, args) = self.ty else {
            unreachable!("Should be named")
        };

        let ty = ctxt.type_def(id).case(case).payload_type(&args, ctxt);
        let layout = match self.layout.kind {
            LayoutKind::Variant { tag, ref cases } => {
                let (size, align) = match tag {
                    layout::TagEncoding::Field { scalar } => (scalar.size(), scalar.align()),
                    layout::TagEncoding::Uninhabited => (Size::ZERO, Align::BYTE),
                };
                layout::Layout::prefixed_by(size, align, cases[case].clone())
            }
            _ => unreachable!(),
        };
        Self::new_with_offset(self.base_ptr, layout, ty, self.offset)
    }
    fn project_field(self, ctxt: CtxtRef<'_>, field: FieldId) -> Self {
        let (ty, offset, layout) = match self.layout.kind {
            LayoutKind::Aggregate(field_layouts, ref field_positions) => (
                self.ty.field_info(field, ctxt).unwrap().0,
                {
                    let offset = field_positions[field].in_bytes();
                    let offset: i32 = offset.try_into().unwrap();
                    offset
                } + self.offset,
                { field_layouts }.swap_remove(0).layout,
            ),
            LayoutKind::Variant { tag, cases } if field == FieldId::FIRST_FIELD => (
                if cases.len() < 256 {
                    Type::Byte
                } else {
                    Type::UINT
                },
                0,
                match tag {
                    TagEncoding::Field { scalar } => layout::Layout::from_scalar(scalar),
                    TagEncoding::Uninhabited => layout::Layout::zst(),
                },
            ),
            _ => unreachable!("invalid field layout"),
        };
        let scalar = match layout.kind {
            LayoutKind::Scalar(scalar) => Some(ScalarType::Single(scalar)),
            LayoutKind::ScalarPair(first, second, offset) => {
                Some(ScalarType::Pair(first, second, offset))
            }
            _ => None,
        };
        Self {
            ty,
            layout,
            base_ptr: self.base_ptr,
            offset,
            scalar,
        }
    }
    fn ptr(&self, builder: &mut FunctionCodegen<'_, impl Module>) -> ir::Value {
        if self.offset == 0 {
            return self.base_ptr;
        }
        let src_offset = builder.builder.ins().build_imm_const(
            ir::types::I64,
            ir::immediates::Imm64::new(self.offset as i64),
            false,
        );
        let src = builder.builder.ins().uadd_overflow_trap(
            self.base_ptr,
            src_offset,
            TrapCode::unwrap_user(1),
        );
        src
    }
    fn offset_by(&self, value: i32) -> Self {
        Self {
            ty: self.ty.clone(),
            layout: self.layout.clone(),
            base_ptr: self.base_ptr,
            offset: self.offset + value,
            scalar: self.scalar,
        }
    }
}
struct CodegenLocalInfo {
    ty: types::Type,
    kind: LocalKind,
}
enum ReturnSlot {
    Arg,
    Local(ir::StackSlot),
    Void,
}
pub struct FunctionCodegen<'r, M: Module> {
    ctxt: CtxtRef<'r>,
    builder: cranelift::frontend::FunctionBuilder<'r>,
    local_info: IndexVec<mir::Local, CodegenLocalInfo>,
    return_ty: Type,
    abi: &'r CallAbi,
    return_slot: ReturnSlot,
    functions: &'r FunctionMap,
    args: GenericArgsRef<'r>,
    target_config: codegen::isa::TargetFrontendConfig,
    module: &'r mut M,
    block_map: &'r BlockMap,
    runtime_functions: &'r RuntimeFunctions,
    panic_block: Cell<Option<ir::Block>>,
}

impl<'a, M: Module> FunctionCodegen<'a, M> {
    fn new(
        ctxt: CtxtRef<'a>,
        mut builder: cranelift::frontend::FunctionBuilder<'a>,
        body: &'a mir::Body,
        args: GenericArgsRef<'a>,
        module: &'a mut M,
        functions: &'a FunctionMap,
        abi: &'a CallAbi,
        block_map: &'a BlockMap,
        runtime_functions: &'a RuntimeFunctions,
    ) -> Self {
        Self {
            runtime_functions,
            block_map,
            target_config: module.target_config(),
            module,
            functions,
            ctxt,
            panic_block: Cell::default(),
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
                                    layout.alignment.pow_of_2(),
                                ),
                            )),
                        },
                    }
                })
                .collect(),
            return_ty: Scheme::new(body.return_type.clone()).bind(args),
            abi,
            return_slot: match abi.ret {
                PassMode::ByValue(_) | PassMode::ByPair(..) => {
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
    fn store_immediate_pair(
        &mut self,
        dst_place: PlaceValue,
        first: ir::Value,
        second: ir::Value,
        offset: Size,
    ) {
        self.builder.ins().store(
            MemFlagsData::new(),
            first,
            dst_place.base_ptr,
            Offset32::new(dst_place.offset),
        );
        let second_offset: i32 = offset.in_bytes().try_into().unwrap();
        let second_offset = dst_place.offset + second_offset;
        self.builder.ins().store(
            MemFlagsData::new(),
            second,
            dst_place.base_ptr,
            Offset32::new(second_offset),
        );
    }
    fn copy(&mut self, place: PlaceValue, dst_place: PlaceValue) {
        let size = self.ctxt.layout_of(&dst_place.ty).unwrap().size;
        let src = place.ptr(self);
        let dst = dst_place.ptr(self);
        self.builder.emit_small_memory_copy(
            self.target_config,
            dst,
            src,
            size.in_bytes(),
            dst_place.align().in_bytes() as u8,
            place.align().in_bytes() as u8,
            false,
            MemFlagsData::new(),
        );
    }
    fn store_scalar(&mut self, place: PlaceValue, value: ScalarValue) {
        match value {
            ScalarValue::Pair([first, second], offset) => {
                self.store_immediate_pair(place, first, second, offset);
            }
            ScalarValue::Single(value) => {
                self.store_immediate(place, value);
            }
        }
    }
    fn store_operand_with_place(&mut self, dst_place: PlaceValue, value: OperandValue) {
        match value.kind {
            OperandValueKind::ZeroSized => (),
            OperandValueKind::Indirect(place) => {
                self.copy(place, dst_place);
            }
            OperandValueKind::Value(value) => self.store_scalar(dst_place, value),
        }
    }
    fn store_value(&mut self, place: &mir::Place, value: OperandValue) {
        let Ok(dst_place) = self.eval_addr_of_place(place) else {
            return;
        };
        self.store_operand_with_place(dst_place, value);
    }
    fn store_operand(&mut self, place: &mir::Place, operand: &mir::Operand) {
        let operand = self.eval_operand(operand);
        self.store_value(place, operand);
    }
    fn load_place_value(&mut self, place: &PlaceValue) -> Option<ScalarValue> {
        let (base_ptr, offset) = place.ptr_and_offset();
        let scalar = place.scalar?;
        match scalar {
            ScalarType::Single(single) => Some(ScalarValue::Single(self.builder.ins().load(
                scalar_to_cranelift_type(single),
                ir::MemFlagsData::new(),
                base_ptr,
                Offset32::new(offset),
            ))),
            ScalarType::Pair(first, second, second_offset) => {
                let first_value = self.builder.ins().load(
                    scalar_to_cranelift_type(first),
                    ir::MemFlagsData::new(),
                    base_ptr,
                    Offset32::new(offset),
                );
                let second_offset_in_bytes: i32 = second_offset.in_bytes().try_into().unwrap();
                let second_value = self.builder.ins().load(
                    scalar_to_cranelift_type(second),
                    ir::MemFlagsData::new(),
                    base_ptr,
                    Offset32::new(offset + second_offset_in_bytes),
                );
                Some(ScalarValue::pair(first_value, second_value, second_offset))
            }
        }
    }
    fn eval_addr_of_place(&mut self, place: &mir::Place) -> Result<PlaceValue, Type> {
        let (ty, ptr) = match place.base {
            PlaceBase::Local(local) => {
                let local_info = &self.local_info[local];
                let ty = local_info.ty.clone();
                let base_kind = local_info.kind;
                let ptr = match base_kind {
                    LocalKind::Memory(addr) => {
                        self.builder
                            .ins()
                            .stack_addr(PTR_IR_TYPE, addr, Offset32::new(0))
                    }

                    LocalKind::ZeroSized => return Err(ty),
                };
                (ty, ptr)
            }
            PlaceBase::ReturnPlace => {
                let ty = self.return_ty.clone();
                let ptr = match self.return_slot {
                    ReturnSlot::Arg => self.builder.block_params(self.block_map.entry())[0],
                    ReturnSlot::Local(return_slot) => {
                        self.builder
                            .ins()
                            .stack_addr(PTR_IR_TYPE, return_slot, Offset32::new(0))
                    }
                    ReturnSlot::Void => return Err(ty),
                };
                (ty, ptr)
            }
        };
        let layout = self.ctxt.layout_of(&ty).unwrap();
        let mut place_value = PlaceValue::new(ptr, layout, ty);
        for projection in place.projections.iter() {
            place_value = match *projection {
                mir::PlaceProjection::Field(field_id) => {
                    place_value.project_field(self.ctxt, field_id)
                }
                mir::PlaceProjection::ConstantIndex(index) => {
                    let ptr_value = self.load_place_value(&place_value).unwrap().first_value();
                    PlaceValue {
                        ty: place_value.ty,
                        layout: place_value.layout,
                        base_ptr: ptr_value,
                        offset: index.try_into().unwrap(),
                        scalar: place_value.scalar,
                    }
                }
                mir::PlaceProjection::Index(local) => {
                    let ptr_value = self.load_place_value(&place_value).unwrap().first_value();
                    let index_place = self.eval_addr_of_place(&mir::Place::local(local)).unwrap();
                    let index_value = self.load_place_value(&index_place).unwrap().first_value();
                    let offset_ptr = self.builder.ins().uadd_overflow_trap(
                        ptr_value,
                        index_value,
                        TrapCode::unwrap_user(1),
                    );
                    PlaceValue {
                        ty: place_value.ty,
                        layout: place_value.layout,
                        base_ptr: offset_ptr,
                        offset: 0,
                        scalar: place_value.scalar,
                    }
                }
                mir::PlaceProjection::CaseDowncast(case, ..) => {
                    place_value.project_downcast(self.ctxt, case)
                }
                mir::PlaceProjection::Deref => {
                    let ty = projection.apply_projection_to_type(place_value.ty.clone(), self.ctxt);
                    let layout = self.ctxt.layout_of(&ty).unwrap();
                    PlaceValue::new(
                        self.load_place_value(&PlaceValue {
                            ty: ty.clone(),
                            layout: layout.clone(),
                            base_ptr: ptr,
                            offset: 0,
                            scalar: Some(ScalarType::Single(Scalar::Pointer { non_null: true })),
                        })
                        .unwrap()
                        .first_value(),
                        layout,
                        ty,
                    )
                }
            };
        }

        Ok(place_value)
    }
    fn build_int_const(&mut self, ty: &Type, value: i128) -> codegen::ir::Value {
        let (ty, value, signed) = match ty {
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
        self.builder.ins().build_imm_const(ty, value, signed)
    }
    fn build_const(&mut self, constant: &Constant) -> OperandValue {
        OperandValue {
            ty: (*constant.ty).clone(),
            kind: match constant.value {
                mir::ConstValue::ZeroSized => OperandValueKind::ZeroSized,
                mir::ConstValue::Named(..) => todo!(),
                mir::ConstValue::Scalar(value) => OperandValueKind::Value(ScalarValue::Single(
                    self.build_int_const(&constant.ty, value),
                )),
                mir::ConstValue::Variant(case, ref data) => {
                    if data.is_some() {
                        todo!("Handle constant data variants")
                    }
                    let (id, _, _) = constant.ty.as_named().unwrap();
                    let (ty, value) = self.ctxt.type_def(id).case_value(case);
                    OperandValueKind::Value(ScalarValue::Single(
                        self.build_int_const(&ty.into_type(), value.into()),
                    ))
                }
                mir::ConstValue::Record(..) => todo!(),
                mir::ConstValue::String(..) => todo!(),
            },
        }
    }
    fn eval_operand(&mut self, operand: &mir::Operand) -> OperandValue {
        match operand {
            Operand::Constant(constant) => self.build_const(constant),
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
        return_place: Option<PlaceValue>,
        abi: &CallAbi,
        args: Vec<OperandValue>,
    ) -> Vec<ir::Value> {
        let args = args
            .into_iter()
            .enumerate()
            .filter_map(|(i, arg)| {
                let OperandValueKind::Indirect(ref place) = arg.kind else {
                    return Some(arg);
                };
                let Some(_) = place.scalar else {
                    return Some(arg);
                };
                let pass_mode = abi.params[i];
                let (PassMode::ByValue(_) | PassMode::ByPair(..)) = pass_mode else {
                    return Some(arg);
                };
                arg.force_immediate_value(self).map(|value| OperandValue {
                    ty: arg.ty.clone(),
                    kind: OperandValueKind::Value(value),
                })
            })
            .collect::<Vec<_>>();
        return_place
            .map(|place| OperandValue {
                ty: place.ty.clone(),
                kind: OperandValueKind::Indirect(place),
            })
            .into_iter()
            .chain(args)
            .flat_map(|arg| match arg.kind {
                OperandValueKind::ZeroSized => None,
                OperandValueKind::Value(value) => Some(value),
                OperandValueKind::Indirect(value) => Some(ScalarValue::Single(value.ptr(self))),
            })
            .flat_map(ScalarValue::into_iter)
            .collect()
    }
    fn store_return_value(
        &mut self,
        values: Vec<ir::Value>,
        ret_place: Option<PlaceValue>,
        abi: &CallAbi,
    ) {
        let Some(ret_place) = ret_place else { return };
        match values.as_slice() {
            [] => (),
            [value] => self.store_immediate(ret_place, *value),
            &[first, second] => {
                let PassMode::ByPair(_, _, offset) = abi.ret else {
                    unreachable!("can only have two values with scalar pair ret")
                };
                self.store_immediate_pair(ret_place, first, second, offset);
            }
            _ => unreachable!(),
        }
    }
    fn codgen_alloc_call(&mut self, ty: &Type, count: u64) -> ir::Value {
        let ty_layout = self.ctxt.layout_of(&ty).unwrap();

        let total_size = ty_layout.size.mul(count).in_bytes();
        let size_val = self.build_int_const(&Type::UINT, total_size.into());
        self.codegen_direct_call_single_scalar_return(self.runtime_functions.alloc, &[size_val])
    }
    fn codegen_direct_call_single_scalar_return(
        &mut self,
        id: FuncId,
        args: &[ir::Value],
    ) -> ir::Value {
        let func = self.module.declare_func_in_func(id, self.builder.func);
        let call = self.builder.ins().call(func, args);
        self.builder.inst_results(call)[0]
    }
    fn codegen_direct_call(
        &mut self,
        ret_place: Option<PlaceValue>,
        abi: &CallAbi,
        id: FuncId,
        args: &[ir::Value],
    ) {
        let func = self.module.declare_func_in_func(id, self.builder.func);
        let call = self.builder.ins().call(func, args);
        let values = self.builder.inst_results(call).to_vec();
        self.store_return_value(values, ret_place, abi);
    }
    fn codegen_call(
        &mut self,
        place: &mir::Place,
        callee: &mir::Operand,
        locals: &Locals,
        args: &[Operand],
    ) {
        let Type::Function(sig) =
            Scheme::new(callee.type_of(self.ctxt, locals, &self.return_ty)).bind(self.args)
        else {
            unreachable!()
        };
        let callee = match callee {
            Operand::Constant(Constant {
                ty: _,
                value: ConstValue::Named(id, args),
            }) => Ok((*id, args)),
            _ => Err(self.eval_operand(callee)),
        };
        let args: Vec<OperandValue> = args
            .iter()
            .map(|operand| self.eval_operand(operand))
            .collect();
        let abi = call_abi(
            self.ctxt,
            &FunctionSig {
                params: sig.params,
                return_type: *sig.return_type,
            },
        );
        let ret_place = self.eval_addr_of_place(place).ok();
        let args = self.explode_args(
            (abi.ret == PassMode::ByPtr)
                .then(|| ret_place.clone())
                .flatten(),
            &abi,
            args,
        );
        match callee {
            Ok((id, generic_args)) => {
                let function = Instance {
                    args: generic_args.clone(),
                    kind: InstanceKind::Function(id),
                };
                let id = self.functions.functions[&function].id;
                self.codegen_direct_call(ret_place, &abi, id, &args);
            }
            Err(value) => {
                let callee = value.force_immediate_value(self).unwrap().first_value();
                let sig = signature(&abi, self.module.target_config());
                let sig = self.builder.func.import_signature(sig);
                let results = self.builder.ins().call_indirect(sig, callee, &args);
                let values = self.builder.inst_results(results).to_vec();
                self.store_return_value(values, ret_place, &abi);
            }
        };
    }
    fn build_overflow_op(
        &mut self,
        op: OverflowOp,
        kind: IntegerKind,
        left: codegen::ir::Value,
        right: codegen::ir::Value,
    ) -> (codegen::ir::Value, codegen::ir::Value) {
        let signed = matches!(kind, IntegerKind::Signed);
        match op {
            OverflowOp::Add => {
                if signed {
                    self.builder.ins().sadd_overflow(left, right)
                } else {
                    self.builder.ins().uadd_overflow(left, right)
                }
            }
            OverflowOp::Multiply => {
                if signed {
                    self.builder.ins().smul_overflow(left, right)
                } else {
                    self.builder.ins().umul_overflow(left, right)
                }
            }
            OverflowOp::Subtract => {
                if signed {
                    self.builder.ins().ssub_overflow(left, right)
                } else {
                    self.builder.ins().usub_overflow(left, right)
                }
            }
        }
    }
    fn codgen_binary_op(
        &mut self,
        place: &mir::Place,
        binary_op: mir::BinaryOp,
        left: &Operand,
        right: &Operand,
    ) {
        let left_operand = self.eval_operand(left);
        let left_value = left_operand
            .force_immediate_value(self)
            .unwrap()
            .first_value();
        let right_operand = self.eval_operand(right);
        let right_value = right_operand
            .force_immediate_value(self)
            .unwrap()
            .first_value();
        let (left, right) = match binary_op {
            BinaryOp::Overflow(op) => {
                let Type::Int(kind) = left_operand.ty else {
                    unreachable!()
                };
                let (left, right) = self.build_overflow_op(op, kind, left_value, right_value);
                (left, Some(right))
            }
            BinaryOp::Wrapping(op) => {
                let Type::Int(kind) = left_operand.ty else {
                    unreachable!()
                };
                let (left, _) = self.build_overflow_op(op, kind, left_value, right_value);
                (left, None)
            }
            BinaryOp::Divide => {
                let Type::Int(kind) = left_operand.ty else {
                    unreachable!()
                };
                (
                    match kind {
                        IntegerKind::Signed => self.builder.ins().sdiv(left_value, right_value),
                        IntegerKind::Unsigned => self.builder.ins().udiv(left_value, right_value),
                    },
                    None,
                )
            }
            BinaryOp::Greater => {
                let Type::Int(kind) = left_operand.ty else {
                    unreachable!()
                };
                (
                    self.builder.ins().icmp(
                        match kind {
                            IntegerKind::Signed => ir::condcodes::IntCC::SignedGreaterThan,
                            IntegerKind::Unsigned => ir::condcodes::IntCC::UnsignedGreaterThan,
                        },
                        left_value,
                        right_value,
                    ),
                    None,
                )
            }
            BinaryOp::Lesser => {
                let Type::Int(kind) = left_operand.ty else {
                    unreachable!()
                };
                (
                    self.builder.ins().icmp(
                        match kind {
                            IntegerKind::Signed => ir::condcodes::IntCC::SignedLessThan,
                            IntegerKind::Unsigned => ir::condcodes::IntCC::UnsignedLessThan,
                        },
                        left_value,
                        right_value,
                    ),
                    None,
                )
            }
            BinaryOp::Equals => (
                self.builder
                    .ins()
                    .icmp(ir::condcodes::IntCC::Equal, left_value, right_value),
                None,
            ),
            BinaryOp::BitwiseAnd => (self.builder.ins().band(left_value, right_value), None),
        };
        let place = self.eval_addr_of_place(place).unwrap();
        if let Some(right) = right {
            let Type::Tuple(fields) = place.ty.clone() else {
                unreachable!()
            };
            let Some(&Type::Int(_)) = fields.first() else {
                unreachable!()
            };
            let offset = layout::INT_SIZE;
            self.store_immediate_pair(place, left, right, offset);
        } else {
            self.store_immediate(place, left);
        }
    }
    fn assign(&mut self, place: &mir::Place, value: &mir::Rvalue, locals: &Locals) {
        match value {
            mir::Rvalue::Aggregate(kind, fields) => match kind {
                AggregateKind::Tuple
                | AggregateKind::NamedRecord(..)
                | AggregateKind::Record { field_names: _ } => {
                    for (id, field) in fields.iter_enumerated() {
                        self.store_operand(&place.clone().with_field(id), field);
                    }
                }
                AggregateKind::Variant(id, case, args) => {
                    let type_def = self.ctxt.type_def(*id);
                    let ty =
                        Scheme::new(Type::Named(*id, type_def.name, args.clone())).bind(self.args);

                    let name = type_def.case(*case).name;
                    let payload_place = place.clone().with_case_downcast(*case, name);
                    for (id, field) in fields.iter_enumerated() {
                        self.store_operand(&payload_place.clone().with_field(id), field);
                    }
                    let LayoutKind::Variant { tag, .. } = self.ctxt.layout_of(&ty).unwrap().kind
                    else {
                        unreachable!()
                    };
                    let Some(dst_place) = self.eval_addr_of_place(place).ok() else {
                        return;
                    };
                    match tag {
                        layout::TagEncoding::Field { .. } => {
                            let (id, ..) = dst_place.ty.as_named().unwrap();
                            let (_, value) = self.ctxt.type_def(id).case_value(*case);
                            let tag = dst_place.project_field(self.ctxt, FieldId::FIRST_FIELD);
                            let discr = self.build_int_const(&tag.ty, value.into());
                            self.store_immediate(tag, discr);
                        }
                        layout::TagEncoding::Uninhabited => (),
                    }
                }
            },
            mir::Rvalue::AllocateArray(ty, fields) => {
                let ty = Scheme::new(ty.clone()).bind(self.args);
                let ty_layout = self.ctxt.layout_of(&ty).unwrap();
                let len: u64 = fields.len().try_into().unwrap();
                let ptr = self.codgen_alloc_call(&ty, len);
                let place_value = PlaceValue::new(ptr, ty_layout.clone(), ty.clone());
                for (i, operand) in fields.iter().enumerate() {
                    let i: u64 = i.try_into().unwrap();
                    let offset: u64 = ty_layout.size.in_bytes() * i;
                    let offset: i32 = offset.try_into().unwrap();
                    let value = self.eval_operand(operand);
                    self.store_operand_with_place(place_value.offset_by(offset), value);
                }
                let len_value = self.build_int_const(&Type::UINT, len.into());
                self.store_value(
                    place,
                    OperandValue {
                        ty: Type::array(ty),
                        kind: OperandValueKind::Value(ScalarValue::pair(
                            ptr,
                            len_value,
                            layout::POINTER_SIZE,
                        )),
                    },
                );
            }
            mir::Rvalue::AllocateBox(ty, operand) => {
                let ty = Scheme::new(ty.clone()).bind(self.args);
                let ptr = self.codgen_alloc_call(&ty, 1);
                let value = self.eval_operand(operand);
                let box_inner_ptr = PlaceValue::new(ptr, self.ctxt.layout_of(&ty).unwrap(), ty);
                self.store_operand_with_place(box_inner_ptr.clone(), value.clone());
                let box_inner_ptr_value = self.load_place_value(&box_inner_ptr).unwrap();
                let place = self.eval_addr_of_place(place).unwrap();
                self.store_scalar(place, box_inner_ptr_value);
            }
            mir::Rvalue::Use(operand) => {
                self.store_operand(place, operand);
            }
            mir::Rvalue::Call(callee, args) => {
                self.codegen_call(place, callee, locals, args);
            }
            mir::Rvalue::Binary(binary_op, operands) => {
                let (left, right) = &**operands;
                self.codgen_binary_op(place, *binary_op, left, right);
            }
            mir::Rvalue::AddrOf(array_place) => {
                let place = self.eval_addr_of_place(place).unwrap();
                let array_place = self.eval_addr_of_place(array_place).unwrap();
                let ptr = self.load_place_value(&array_place).unwrap().first_value();
                self.store_immediate(place, ptr);
            }
            mir::Rvalue::Cast(..) => todo!("Transmutation"),
            mir::Rvalue::Len(array_place) => {
                let place = self.eval_addr_of_place(place).unwrap();
                let len_place = self.eval_addr_of_place(array_place).unwrap();
                let len_place =
                    len_place.offset_by(layout::POINTER_SIZE.in_bytes().try_into().unwrap());
                self.copy(len_place, place);
            }
            mir::Rvalue::Discriminant(variant_place) => {
                let Ok(dst_place) = self.eval_addr_of_place(place) else {
                    unreachable!()
                };
                let tag_value = match self.eval_addr_of_place(variant_place) {
                    Ok(place) => place.project_field(self.ctxt, FieldId::FIRST_FIELD),
                    Err(_) => {
                        return;
                    }
                };
                let extend = !tag_value.ty.is_integer();
                let value = self.load_place_value(&tag_value).unwrap().first_value();
                let value = if extend {
                    self.builder.ins().uextend(ir::types::I64, value)
                } else {
                    value
                };
                self.store_immediate(dst_place, value);
            }
        }
    }
    fn store_params(&mut self, body: &'_ mir::Body) {
        let abi = self.abi;
        let param_values = {
            let mut param_values = Vec::new();
            let mut param_index = if abi.ret != PassMode::ByPtr {
                0usize
            } else {
                1usize
            };
            for &mode in abi.params.iter() {
                let curr_param = param_index;
                param_values.push((
                    curr_param,
                    match mode {
                        PassMode::Void => None,
                        PassMode::ByPair(first, second, offset) => {
                            param_index += 2;
                            Some(ScalarType::Pair(first, second, offset))
                        }
                        PassMode::ByPtr => {
                            param_index += 1;
                            Some(ScalarType::Single(Scalar::Pointer { non_null: true }))
                        }
                        PassMode::ByValue(scalar) => {
                            param_index += 1;
                            Some(ScalarType::Single(scalar))
                        }
                    },
                ));
            }
            param_values
        };
        for (i, local) in body.params_iter().enumerate() {
            if matches!(self.local_info[local].kind, LocalKind::Memory(_))
                && let Ok(place) = self.eval_addr_of_place(&Place::local(local))
            {
                let (param_index, scalar_ty) = param_values[i];
                let Some(scalar_ty) = scalar_ty else {
                    continue;
                };
                let value = match scalar_ty {
                    ScalarType::Single(_) => ScalarValue::Single(
                        self.builder.block_params(self.block_map.entry())[param_index],
                    ),
                    ScalarType::Pair(.., offset) => ScalarValue::pair(
                        self.builder.block_params(self.block_map.entry())[param_index],
                        self.builder.block_params(self.block_map.entry())[param_index + 1],
                        offset,
                    ),
                };
                self.store_scalar(place, value);
            }
        }
    }
    fn codegen_panic_terminator(&mut self) {
        if let Some(panic_block) = &self.panic_block.get() {
            self.builder.ins().jump(*panic_block, &[]);
        } else {
            let panic_block = self.builder.create_block();
            self.builder.set_cold_block(panic_block);
            self.builder.ins().jump(panic_block, &[]);
            self.builder.switch_to_block(panic_block);
            self.codegen_direct_call(
                None,
                &CallAbi {
                    params: Vec::new(),
                    ret: PassMode::Void,
                },
                self.runtime_functions.panic,
                &[],
            );
            self.builder.ins().trap(TrapCode::user(1).unwrap());
            self.panic_block.set(Some(panic_block));
        };
    }
    fn codgen(mut self, body: &'_ mir::Body) {
        let block_map = self.block_map;

        for (id, &block) in block_map.blocks.iter_enumerated() {
            let Some(bb) = block else {
                continue;
            };
            let block = &body.block_info.blocks()[id];
            self.builder.switch_to_block(bb);
            if id == BasicBlockId::ENTRY {
                self.store_params(body);
            }
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
                    let value = value
                        .force_immediate_value(&mut self)
                        .unwrap()
                        .first_value();
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
                mir::TerminatorKind::Switch(operand, targets) => {
                    let operand = self.eval_operand(operand);
                    let value = operand
                        .force_immediate_value(&mut self)
                        .unwrap()
                        .first_value();
                    let otherwise_block = block_map.blocks[targets.otherwise].unwrap();
                    let mut switch = frontend::Switch::new();
                    for target in targets.targets.iter() {
                        switch.set_entry(
                            u128::from_ne_bytes(target.value.to_ne_bytes()),
                            block_map.blocks[target.target].unwrap(),
                        );
                    }
                    switch.emit(&mut self.builder, value, otherwise_block);
                }
                mir::TerminatorKind::Unreachable => {
                    self.builder.ins().trap(TrapCode::user(1).unwrap());
                }
                mir::TerminatorKind::Return => {
                    let rvalue = self.eval_addr_of_place(&Place::return_place()).ok();
                    let mode = self.functions.functions[&Instance {
                        args: self.args.iter().cloned().collect(),
                        kind: InstanceKind::Function(body.src.def_id()),
                    }]
                        .abi
                        .ret;
                    let rvals = match mode {
                        PassMode::Void | PassMode::ByPtr => None,
                        PassMode::ByPair(..) | PassMode::ByValue(_) => {
                            rvalue.and_then(|ref place| self.load_place_value(place))
                        }
                    };
                    if let Some(return_value) = rvals {
                        self.builder.ins().return_(return_value.as_slice());
                    } else {
                        self.builder.ins().return_(&[]);
                    };
                }
                mir::TerminatorKind::Goto(basic_block_id) => {
                    self.builder
                        .ins()
                        .jump(block_map.blocks[*basic_block_id].unwrap(), &[]);
                }
                mir::TerminatorKind::Panic => {
                    self.codegen_panic_terminator();
                }
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
    fn entry(&self) -> codegen::ir::Block {
        self.blocks[BasicBlockId::ENTRY].unwrap()
    }
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
