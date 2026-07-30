use std::{cell::Cell, collections::HashMap};

use crate::{
    CtxtRef,
    codegen::{
        backend_repr::{BackendRepr, backend_repr},
        locals::{LocalKind, ReturnSlot},
    },
    config::Feature,
    index_vec::IndexVec,
    layout::{self, Align, LayoutKind, Scalar, Size, TagEncoding},
    mir::{
        self, AggregateKind, AssertKind, BasicBlockId, BinaryOp, ConstValue, Constant, Locals,
        Operand, OverflowOp, Place, PlaceBase, traversal::reachable,
    },
    monomorph::collect::{Instance, InstanceKind},
    scheme::Scheme,
    typed_ast::FieldId,
    types::{CaseId, FunctionSig, GenericArgsRef, IntegerKind, Type},
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
mod locals;

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
}
impl PassMode {
    const fn new(repr: BackendRepr) -> PassMode {
        match repr {
            BackendRepr::Memory => PassMode::ByPtr,
            BackendRepr::Scalar(scalar) => PassMode::ByValue(scalar),
            BackendRepr::ZeroSized => PassMode::Void,
        }
    }
}
#[track_caller]
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
#[derive(Debug)]
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
    print_int: FuncId,
    print_string: FuncId,
    print_newline: FuncId,
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
    #[track_caller]
    fn make_function(
        &mut self,
        ctxt: &mut codegen::Context,
        f_ctxt: &mut frontend::FunctionBuilderContext,
        name: &str,
        linkage: cranelift_module::Linkage,
        sig: ir::Signature,
        build_function: impl FnOnce(&mut Self, &mut frontend::FunctionBuilder, ir::Block),
    ) -> FuncId {
        let function = self.declare_function(name, linkage, &sig);
        {
            ctxt.func.signature = sig.clone();
            let mut builder = frontend::FunctionBuilder::new(&mut ctxt.func, f_ctxt);
            let entry = builder.create_block();
            builder.switch_to_block(entry);
            build_function(self, &mut builder, entry);
            builder.finalize(self.module.target_config());
            if self.ctxt.config().has_feature(Feature::OutputBackendIr) {
                println!("{:?}", ctxt.func);
            }
            self.module.define_function(function, ctxt).unwrap();
            self.module.clear_context(ctxt);
        }
        function
    }
    pub fn codegen_functions(mut self, mir_ctxt: &mir::Context) -> cranelift_object::ObjectProduct {
        for (i, instance) in self.instances.iter().enumerate() {
            let (name, linkage) = if self
                .ctxt
                .main_function()
                .is_some_and(|(id, _)| id == instance.body_src().def_id())
            {
                ("main".to_string(), cranelift_module::Linkage::Export)
            } else {
                (
                    format!("f_{}_{i}", self.ctxt.display(instance.body_src().def_id())),
                    cranelift_module::Linkage::Local,
                )
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
                    id: self.module.declare_function(&name, linkage, &sig).unwrap(),
                    sig,
                    abi,
                },
            );
        }
        let mut ctxt = codegen::Context::new();
        let mut f_ctxt = frontend::FunctionBuilderContext::new();
        let mut constants = Constants::default();

        let print_int =
            self.declare_function("kli_print_int", cranelift_module::Linkage::Import, &{
                let mut sig = ir::Signature::new(self.module.target_config().default_call_conv);
                sig.params.push(AbiParam::new(ir::types::I64));
                sig
            });
        let print_string =
            self.declare_function("kli_print_string", cranelift_module::Linkage::Import, &{
                let mut sig = ir::Signature::new(self.module.target_config().default_call_conv);
                sig.params.push(AbiParam::new(PTR_IR_TYPE));
                sig.params.push(AbiParam::new(ir::types::I64));
                sig
            });
        let print_newline =
            self.declare_function("kli_print_newline", cranelift_module::Linkage::Import, &{
                ir::Signature::new(self.module.target_config().default_call_conv)
            });

        //Declare panic function
        let panic_function = self.make_function(
            &mut ctxt,
            &mut f_ctxt,
            "panic",
            cranelift_module::Linkage::Export,
            ir::Signature::new(self.module.target_config().default_call_conv),
            |_, builder, entry| {
                builder.ins().trap(TrapCode::user(1).unwrap());
                builder.seal_block(entry);
            },
        );
        let allocate_function = {
            let malloc = self.declare_function("malloc", cranelift_module::Linkage::Import, &{
                let mut sig = ir::Signature::new(self.module.target_config().default_call_conv);
                sig.params.push(AbiParam::new(ir::types::I64));
                sig.returns.push(AbiParam::new(PTR_IR_TYPE));
                sig
            });
            self.make_function(
                &mut ctxt,
                &mut f_ctxt,
                "kli_alloc",
                cranelift_module::Linkage::Hidden,
                {
                    let mut sig = ir::Signature::new(self.module.target_config().default_call_conv);
                    sig.params.push(AbiParam::new(ir::types::I64));
                    sig.params.push(AbiParam::new(ir::types::I64));
                    sig.returns.push(AbiParam::new(PTR_IR_TYPE));
                    sig
                },
                |this, builder, entry| {
                    /* Essentially this should be:
                        if size == 0 then return invalid_but_aligned_ptr;
                        let ptr = malloc(size);
                        if ptr == null then panic()
                        return ptr
                    */

                    builder.append_block_param(entry, ir::types::I64);
                    builder.append_block_param(entry, ir::types::I64);
                    let &[size_arg, align_arg] = builder.block_params(entry).as_array().unwrap();
                    let malloc = this.module.declare_func_in_func(malloc, builder.func);
                    let non_zero_size_block = builder.create_block();
                    let zero_size_block = builder.create_block();
                    let alloc_failed_block = builder.create_block();
                    let alloc_success_block = builder.create_block();

                    builder
                        .ins()
                        .brif(size_arg, non_zero_size_block, &[], zero_size_block, &[]);
                    builder.seal_block(entry);

                    builder.switch_to_block(zero_size_block);
                    builder.ins().return_(&[align_arg]);
                    builder.seal_block(zero_size_block);

                    builder.switch_to_block(non_zero_size_block);
                    let ptr = builder.ins().call(malloc, &[size_arg]);
                    let ptr = builder.inst_results(ptr)[0];
                    builder
                        .ins()
                        .brif(ptr, alloc_success_block, [], alloc_failed_block, []);
                    builder.seal_block(non_zero_size_block);

                    builder.switch_to_block(alloc_failed_block);
                    let print_string = this
                        .module
                        .declare_func_in_func(print_string, &mut builder.func);
                    let print_newline = this
                        .module
                        .declare_func_in_func(print_newline, &mut builder.func);
                    const MSG: &str = "failed to allocated";
                    let len = const { MSG.len() as i64 };
                    let msg = constants.constant_for(
                        &mut this.module,
                        String::from(MSG).into_bytes().into_boxed_slice(),
                    );
                    let msg = this.module.declare_data_in_func(msg, &mut builder.func);
                    let msg = builder.ins().symbol_value(PTR_IR_TYPE, msg);
                    let len = builder.ins().iconst(ir::types::I64, len);
                    builder.ins().call(print_string, &[msg, len]);
                    builder.ins().call(print_newline, &[]);
                    builder.ins().trap(TrapCode::unwrap_user(1));
                    builder.seal_block(alloc_failed_block);

                    builder.switch_to_block(alloc_success_block);
                    builder.ins().return_(&[ptr]);
                    builder.seal_block(alloc_success_block);
                },
            )
        };
        let runtime = RuntimeFunctions {
            panic: panic_function,
            alloc: allocate_function,
            print_int,
            print_string,
            print_newline,
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
            let should_print = self.ctxt.config().has_feature(Feature::OutputBackendIr);
            if should_print {
                println!(
                    "building {} {}",
                    self.ctxt.display_path_for(instance.body_src().def_id()),
                    instance.args
                );
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
                &mut constants,
            )
            .codgen(body);
            if should_print {
                println!("{:?}", ctxt.func);
            }
            self.module.define_function(id, &mut ctxt).unwrap();
            self.module.clear_context(&mut ctxt);
        }
        self.module.finish()
    }
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
            OperandValueKind::Indirect(ref place) => cg.load_place(place),
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
    Indirect(CodegenPlace),
    ZeroSized,
    Value(ScalarValue),
}
#[derive(Clone, Debug, PartialEq, Eq, Copy)]
enum ScalarType {
    Single(Scalar),
}

#[derive(Debug, Clone)]
enum CodegenPlace {
    Ssa(Type, frontend::Variable),
    MemPlace(MemPlace),
}
impl CodegenPlace {
    fn type_of(&self) -> Type {
        match &self {
            CodegenPlace::MemPlace(place) => place.ty.clone(),
            CodegenPlace::Ssa(ty, ..) => ty.clone(),
        }
    }
}
#[derive(Clone, Debug)]
struct MemPlace {
    ty: Type,
    layout: layout::Layout,
    base_ptr: codegen::ir::Value,
    offset: i32,
    scalar: Option<ScalarType>,
}
impl MemPlace {
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
    #[track_caller]
    fn project_field(self, ctxt: CtxtRef<'_>, field: FieldId) -> Self {
        let (ty, offset, layout) = match self.layout.kind {
            LayoutKind::Aggregate(field_layouts, ref field_positions) => (
                self.ty.field_info(field, ctxt).unwrap().0,
                field_positions[field].offset.in_bytes_i32() + self.offset,
                { field_layouts }
                    .swap_remove(field_positions[field].index_in_memory)
                    .layout,
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
            kind => unreachable!(
                "invalid field layout at {field:?} {:?} for {}",
                kind, self.ty
            ),
        };
        Self::new_with_offset(self.base_ptr, layout, ty, offset)
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
        builder.builder.ins().uadd_overflow_trap(
            self.base_ptr,
            src_offset,
            TrapCode::unwrap_user(1),
        )
    }
    fn offset_in_bytes(&self, value: i32) -> Self {
        Self {
            ty: self.ty.clone(),
            layout: self.layout.clone(),
            base_ptr: self.base_ptr,
            offset: self.offset + value,
            scalar: self.scalar,
        }
    }
}
pub struct FunctionCodegen<'r, M: Module> {
    ctxt: CtxtRef<'r>,
    builder: cranelift::frontend::FunctionBuilder<'r>,
    local_info: locals::Locals,
    return_ty: Type,
    abi: &'r CallAbi,
    constants: &'r mut Constants,
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
        constants: &'a mut Constants,
    ) -> Self {
        Self {
            runtime_functions,
            block_map,
            target_config: module.target_config(),
            module,
            functions,
            ctxt,
            panic_block: Cell::default(),
            local_info: locals::Locals::new(body, args, ctxt, &mut builder, abi.ret),
            return_ty: Scheme::new(body.return_type.clone()).bind(args),
            abi,
            args,
            constants,
            builder,
        }
    }
    fn store_immediate(&mut self, dst_place: CodegenPlace, value: ir::Value) {
        match dst_place {
            CodegenPlace::MemPlace(place) => self.store_immediate_mem(place, value),
            CodegenPlace::Ssa(.., var) => self.store_var_imm(var, value),
        }
    }

    fn store_immediate_pair(
        &mut self,
        dst_place: CodegenPlace,
        first: ir::Value,
        second: ir::Value,
        offset: Size,
    ) {
        match dst_place {
            CodegenPlace::MemPlace(place) => {
                self.store_immediate_pair_mem(place, first, second, offset)
            }
            CodegenPlace::Ssa(..) => {
                unreachable!()
            }
        }
    }
    fn store_immediate_mem(&mut self, dst_place: MemPlace, value: ir::Value) {
        self.builder.ins().store(
            MemFlagsData::new(),
            value,
            dst_place.base_ptr,
            Offset32::new(dst_place.offset),
        );
    }
    fn store_immediate_pair_mem(
        &mut self,
        dst_place: MemPlace,
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
        let second_offset = offset.in_bytes_i32();
        let second_offset = dst_place.offset + second_offset;
        self.builder.ins().store(
            MemFlagsData::new(),
            second,
            dst_place.base_ptr,
            Offset32::new(second_offset),
        );
    }
    fn copy(&mut self, place: CodegenPlace, dst_place: CodegenPlace) {
        match (place, dst_place) {
            (CodegenPlace::MemPlace(place), CodegenPlace::MemPlace(dst_place)) => {
                self.memcopy(place, dst_place)
            }

            (ref place, dst_place) => {
                let Some(value) = self.load_place(place) else {
                    return;
                };
                self.store_scalar(dst_place, value);
            }
        }
    }
    fn memcopy(&mut self, place: MemPlace, dst_place: MemPlace) {
        let size = self.layout_for(&dst_place.ty).size;
        let src = place.ptr(self);
        let dst = dst_place.ptr(self);
        self.builder.emit_small_memory_copy(
            self.target_config,
            dst,
            src,
            size.in_bytes(),
            dst_place.align().pow_of_2(),
            place.align().pow_of_2(),
            false,
            MemFlagsData::new(),
        );
    }
    fn store_scalar(&mut self, place: CodegenPlace, value: ScalarValue) {
        match place {
            CodegenPlace::MemPlace(place) => self.store_scalar_mem(place, value),
            CodegenPlace::Ssa(.., var) => {
                self.store_var_imm(var, value.first_value());
            }
        }
    }
    fn store_scalar_mem(&mut self, place: MemPlace, value: ScalarValue) {
        match value {
            ScalarValue::Pair([first, second], offset) => {
                self.store_immediate_pair_mem(place, first, second, offset);
            }
            ScalarValue::Single(value) => {
                self.store_immediate_mem(place, value);
            }
        }
    }
    fn store_operand_with_mem_place(&mut self, dst_place: MemPlace, value: OperandValue) {
        match value.kind {
            OperandValueKind::ZeroSized => (),
            OperandValueKind::Indirect(place) => {
                self.copy(place, CodegenPlace::MemPlace(dst_place));
            }
            OperandValueKind::Value(value) => self.store_scalar_mem(dst_place, value),
        }
    }
    fn store_var_imm(&mut self, var: frontend::Variable, value: codegen::ir::Value) {
        self.builder.def_var(var, value);
    }
    fn store_value(&mut self, place: &mir::Place, value: OperandValue) {
        let Ok(dst_place) = self.eval_place(place) else {
            return;
        };
        let first_var = match dst_place {
            CodegenPlace::MemPlace(place) => {
                self.store_operand_with_mem_place(place, value);
                return;
            }
            CodegenPlace::Ssa(.., variable) => variable,
        };
        let value = value.force_immediate_value(self).unwrap();
        self.store_var_imm(first_var, value.first_value());
    }
    fn store_operand(&mut self, place: &mir::Place, operand: &mir::Operand) {
        let operand = self.eval_operand(operand);
        self.store_value(place, operand);
    }
    fn load_place(&mut self, place: &CodegenPlace) -> Option<ScalarValue> {
        match &place {
            CodegenPlace::MemPlace(place) => self.load_place_mem(place),
            CodegenPlace::Ssa(.., var) => Some(ScalarValue::Single(self.builder.use_var(*var))),
        }
    }
    fn load_place_mem(&mut self, place: &MemPlace) -> Option<ScalarValue> {
        let (base_ptr, offset) = place.ptr_and_offset();
        let scalar = place.scalar?;
        match scalar {
            ScalarType::Single(single) => Some(ScalarValue::Single(self.builder.ins().load(
                scalar_to_cranelift_type(single),
                ir::MemFlagsData::new(),
                base_ptr,
                Offset32::new(offset),
            ))),
        }
    }
    #[track_caller]
    fn layout_for(&self, ty: &Type) -> layout::Layout {
        self.ctxt.layout_of(ty).expect("should be monoed enough")
    }
    fn load_array_ptr(&mut self, place: MemPlace) -> codegen::ir::Value {
        let (ptr, offset) = place.ptr_and_offset();
        self.builder
            .ins()
            .load(PTR_IR_TYPE, MemFlagsData::new(), ptr, offset)
    }
    fn load_array_len(&mut self, place: MemPlace) -> codegen::ir::Value {
        let place = place.offset_in_bytes(layout::POINTER_SIZE.in_bytes_i32());
        let (ptr, offset) = place.ptr_and_offset();
        self.builder
            .ins()
            .load(ir::types::I64, MemFlagsData::new(), ptr, offset)
    }
    fn eval_place(&mut self, place: &mir::Place) -> Result<CodegenPlace, Type> {
        let (ty, ptr, projections) = match place.base {
            PlaceBase::Local(local) => {
                let local_info = &self.local_info.info_for(local);
                let mut ty = local_info.ty.clone();
                let base_kind = local_info.kind;
                let mut projections = &place.projections[..];
                let ptr = match base_kind {
                    LocalKind::Memory(addr) => {
                        self.builder
                            .ins()
                            .stack_addr(PTR_IR_TYPE, addr, Offset32::new(0))
                    }

                    LocalKind::ZeroSized => return Err(ty),
                    LocalKind::Scalar(var) => 'a: {
                        if !projections.is_empty() {
                            for projection in place.projections.iter() {
                                projections = &projections[1..];
                                ty = projection.apply_projection_to_type(ty, self.ctxt);
                                match projection {
                                    mir::PlaceProjection::ConstantIndex(_)
                                    | mir::PlaceProjection::Index(_) => {
                                        unreachable!("shouldn't be scalar then")
                                    }
                                    mir::PlaceProjection::CaseDowncast(..)
                                    | mir::PlaceProjection::Field(_) => (),
                                    mir::PlaceProjection::Deref => {
                                        let value = self
                                            .load_place(&CodegenPlace::Ssa(ty.clone(), var))
                                            .unwrap()
                                            .first_value();
                                        break 'a value;
                                    }
                                }
                            }
                        }
                        return Ok(CodegenPlace::Ssa(ty, var));
                    }
                };
                (ty, ptr, projections)
            }
            PlaceBase::ReturnPlace => {
                let ty = self.return_ty.clone();
                let ptr = match self.local_info.return_slot() {
                    ReturnSlot::Scalar(variable) => {
                        return Ok(CodegenPlace::Ssa(ty, variable));
                    }
                    ReturnSlot::Arg => self.builder.block_params(self.block_map.entry())[0],
                    ReturnSlot::Local(return_slot) => {
                        self.builder
                            .ins()
                            .stack_addr(PTR_IR_TYPE, return_slot, Offset32::new(0))
                    }
                    ReturnSlot::Void => return Err(ty),
                };
                (ty, ptr, &place.projections[..])
            }
        };
        let layout = self.layout_for(&ty);
        let mut place_value = MemPlace::new(ptr, layout, ty);
        for projection in projections {
            place_value = match *projection {
                mir::PlaceProjection::Field(field_id) => {
                    place_value.project_field(self.ctxt, field_id)
                }
                mir::PlaceProjection::ConstantIndex(index) => {
                    let ty = projection.apply_projection_to_type(place_value.ty.clone(), self.ctxt);
                    let layout = self.layout_for(&ty);
                    let ptr_value = self.load_array_ptr(place_value.clone());
                    let index: u64 = index.into();
                    let offset: i32 = (index * layout.size.in_bytes()).try_into().unwrap();
                    MemPlace::new_with_offset(ptr_value, layout, ty, offset)
                }
                mir::PlaceProjection::Index(local) => {
                    let ty = projection.apply_projection_to_type(place_value.ty.clone(), self.ctxt);
                    let layout = self.layout_for(&ty);
                    let ptr_value = self.load_array_ptr(place_value);
                    let index_place = self.eval_place(&mir::Place::local(local)).unwrap();
                    let index_value = self.load_place(&index_place).unwrap().first_value();
                    let size_value =
                        self.build_int_const(&index_place.type_of(), layout.size.in_bytes().into());
                    let (offset, overflow) =
                        self.builder.ins().umul_overflow(index_value, size_value);
                    self.builder
                        .ins()
                        .trapnz(overflow, TrapCode::unwrap_user(1));
                    let offset_ptr = self.builder.ins().uadd_overflow_trap(
                        ptr_value,
                        offset,
                        TrapCode::unwrap_user(1),
                    );
                    MemPlace::new(offset_ptr, layout, ty)
                }
                mir::PlaceProjection::CaseDowncast(case, ..) => {
                    place_value.project_downcast(self.ctxt, case)
                }
                mir::PlaceProjection::Deref => {
                    let ty = projection.apply_projection_to_type(place_value.ty.clone(), self.ctxt);
                    let layout = self.layout_for(&ty);
                    let box_ptr = self
                        .load_place_mem(&MemPlace {
                            ty: ty.clone(),
                            layout: layout.clone(),
                            base_ptr: ptr,
                            offset: 0,
                            scalar: Some(ScalarType::Single(Scalar::Pointer { non_null: true })),
                        })
                        .unwrap()
                        .first_value();
                    MemPlace::new(box_ptr, layout, ty)
                }
            };
        }

        Ok(CodegenPlace::MemPlace(place_value))
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
        let ty = Scheme::new((*constant.ty).clone()).bind(self.args);
        let kind = match constant.value {
            mir::ConstValue::ZeroSized => OperandValueKind::ZeroSized,
            mir::ConstValue::Named(id, ref args) => {
                let kind = InstanceKind::Function(id);
                let function = Instance {
                    args: args.clone(),
                    kind,
                };
                let func_id = self
                    .functions
                    .functions
                    .get(&function)
                    .unwrap_or_else(|| panic!("not found {:?}", function))
                    .id;
                let function = self.module.declare_func_in_func(func_id, self.builder.func);
                OperandValueKind::Value(ScalarValue::Single(
                    self.builder.ins().func_addr(PTR_IR_TYPE, function),
                ))
            }
            mir::ConstValue::Scalar(value) => OperandValueKind::Value(ScalarValue::Single(
                self.build_int_const(&constant.ty, value),
            )),
            mir::ConstValue::Variant(case, ref data) => {
                if data.is_some() {
                    todo!("Handle constant data variants")
                }
                let (id, _, _) = constant.ty.as_named().unwrap();
                let (tag_ty, value) = self.ctxt.type_def(id).case_value(case);
                let layout = self.layout_for(&ty);
                match backend_repr(&layout) {
                    BackendRepr::Scalar(_) => OperandValueKind::Value(ScalarValue::Single(
                        self.build_int_const(&tag_ty.into_type(), value.into()),
                    )),
                    BackendRepr::ZeroSized => unreachable!(),
                    BackendRepr::Memory => {
                        let mut bytes: Box<[u8]> =
                            std::iter::repeat_n(0, layout.size.in_bytes_usize()).collect();
                        if value < 256 {
                            bytes[0] = value as u8;
                        } else {
                            todo!("really big")
                        };
                        let ptr = self.alloc_constant(bytes);
                        OperandValueKind::Indirect(CodegenPlace::MemPlace(MemPlace::new(
                            ptr,
                            layout,
                            ty.clone(),
                        )))
                    }
                }
            }
            mir::ConstValue::Record(..) => todo!(),
            mir::ConstValue::String(name) => {
                let string = name.to_string();
                let (first, second) = self.build_string_value(string);
                OperandValueKind::Value(ScalarValue::pair(first, second, layout::POINTER_SIZE))
            }
        };
        OperandValue { ty, kind }
    }
    fn alloc_constant(&mut self, bytes: Box<[u8]>) -> ir::Value {
        let data = self.constants.constant_for(self.module, bytes);
        let data = self.module.declare_data_in_func(data, self.builder.func);
        self.builder.ins().symbol_value(PTR_IR_TYPE, data)
    }
    fn build_string_value(&mut self, string: String) -> (ir::Value, ir::Value) {
        let len: u64 = string.len().try_into().unwrap();
        let first = self.alloc_constant(string.into_bytes().into_boxed_slice());
        let second = self.build_int_const(&Type::UINT, len.into());
        (first, second)
    }
    fn eval_operand(&mut self, operand: &mir::Operand) -> OperandValue {
        match operand {
            Operand::Constant(constant) => self.build_const(constant),
            Operand::Load(place) => match self.eval_place(place) {
                Ok(place) => OperandValue {
                    ty: place.type_of().clone(),
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
        return_place: Option<CodegenPlace>,
        abi: &CallAbi,
        args: Vec<OperandValue>,
    ) -> Vec<ir::Value> {
        let mut all_args = Vec::new();
        if let Some(CodegenPlace::MemPlace(place)) = return_place {
            all_args.push(place.ptr(self));
        }
        for (arg, &param) in args.into_iter().zip(abi.params.iter()) {
            match (arg.kind, param) {
                (_, PassMode::Void) => (),
                (OperandValueKind::Indirect(codegen_place), PassMode::ByPtr) => {
                    let CodegenPlace::MemPlace(mem_place) = codegen_place else {
                        unreachable!()
                    };
                    all_args.push(mem_place.ptr(self));
                }
                (OperandValueKind::Indirect(codegen_place), PassMode::ByValue(_)) => {
                    let value = self.load_place(&codegen_place).unwrap();
                    all_args.extend(value.into_iter());
                }
                (OperandValueKind::ZeroSized, _) | (_, PassMode::ByPtr) => unreachable!(),
                (OperandValueKind::Value(scalar_value), PassMode::ByValue(_)) => {
                    all_args.extend(scalar_value.into_iter());
                }
            }
        }
        all_args
    }
    fn store_return_value(&mut self, values: Vec<ir::Value>, ret_place: Option<CodegenPlace>) {
        let Some(ret_place) = ret_place else { return };
        match values.as_slice() {
            [] => (),
            [value] => self.store_immediate(ret_place, *value),
            _ => unreachable!(),
        }
    }
    fn codegen_alloc_call(
        &mut self,
        size_in_bytes: ir::Value,
        align_in_bytes: ir::Value,
    ) -> ir::Value {
        self.codegen_direct_call_single_scalar_return(
            self.runtime_functions.alloc,
            &[size_in_bytes, align_in_bytes],
        )
    }
    fn codgen_static_size_alloc_call(&mut self, ty: &Type, count: u64) -> ir::Value {
        let ty_layout = self.layout_for(ty);
        let total_size = ty_layout.size.mul(count).in_bytes();
        let size_val = self.build_int_const(&Type::UINT, total_size.into());
        let align = self.build_int_const(&Type::UINT, ty_layout.alignment.in_bytes().into());
        self.codegen_alloc_call(size_val, align)
    }
    fn print_string_newline(&mut self, ptr: ir::Value, len: ir::Value) {
        let print_string = self.runtime_functions.print_string;
        let print_newline = self.runtime_functions.print_newline;
        self.codegen_direct_void_call(print_string, &[ptr, len]);
        self.codegen_direct_void_call(print_newline, &[]);
    }
    fn codegen_direct_void_call(&mut self, id: FuncId, args: &[ir::Value]) {
        let func = self.module.declare_func_in_func(id, self.builder.func);
        let call = self.builder.ins().call(func, args);
        self.builder.inst_results(call);
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
        ret_place: Option<CodegenPlace>,
        id: FuncId,
        args: &[ir::Value],
    ) {
        let func = self.module.declare_func_in_func(id, self.builder.func);
        let call = self.builder.ins().call(func, args);
        let values = self.builder.inst_results(call).to_vec();
        self.store_return_value(values, ret_place);
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
                params: sig.params.clone(),
                return_type: (*sig.return_type).clone(),
            },
        );
        let ret_place = self.eval_place(place).ok();
        match callee {
            Ok((def_id, generic_args)) => {
                let function = Instance {
                    args: Scheme::new(generic_args.clone()).bind(self.args),
                    kind: InstanceKind::Function(def_id),
                };
                let id = self
                    .functions
                    .functions
                    .get(&function)
                    .unwrap_or_else(|| panic!("not found {:?}", function))
                    .id;
                let flattened_args = self.explode_args(
                    (abi.ret == PassMode::ByPtr)
                        .then(|| ret_place.clone())
                        .flatten(),
                    &abi,
                    args,
                );
                debug_assert_eq!(
                    flattened_args.len(),
                    signature(&abi, self.target_config).params.len(),
                    "{}",
                    self.ctxt.display_path_for(def_id)
                );
                self.codegen_direct_call(ret_place, id, &flattened_args);
            }
            Err(value) => {
                let callee = value.force_immediate_value(self).unwrap().first_value();
                let sig = signature(&abi, self.module.target_config());
                let sig = self.builder.func.import_signature(sig);

                let flattened_args = self.explode_args(
                    (abi.ret == PassMode::ByPtr)
                        .then(|| ret_place.clone())
                        .flatten(),
                    &abi,
                    args,
                );
                let results = self
                    .builder
                    .ins()
                    .call_indirect(sig, callee, &flattened_args);
                let values = self.builder.inst_results(results).to_vec();
                self.store_return_value(values, ret_place);
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
        let place = self.eval_place(place).unwrap();
        if let Some(right) = right {
            let Type::Tuple(fields) = place.type_of().clone() else {
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
    fn codgen_box(&mut self, place: &mir::Place, ty: &Type, operand: &mir::Operand) {
        let ty = Scheme::new(ty.clone()).bind(self.args);
        let ptr = self.codgen_static_size_alloc_call(&ty, 1);
        let value = self.eval_operand(operand);
        let box_inner_ptr = MemPlace::new(ptr, self.layout_for(&ty), ty);
        self.store_operand_with_mem_place(box_inner_ptr.clone(), value.clone());

        let place = self.eval_place(place).unwrap();
        let box_ptr = box_inner_ptr.ptr(self);
        self.store_immediate(place, box_ptr);
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
                    let LayoutKind::Variant { tag, .. } = self.layout_for(&ty).kind else {
                        unreachable!()
                    };
                    let Some(dst_place) = self.eval_place(place).ok() else {
                        return;
                    };
                    match tag {
                        layout::TagEncoding::Field { .. } => {
                            let (id, ..) = dst_place.type_of().as_named().unwrap();
                            let (_, value) = self.ctxt.type_def(id).case_value(*case);
                            match dst_place {
                                CodegenPlace::MemPlace(dst_place) => {
                                    let tag =
                                        dst_place.project_field(self.ctxt, FieldId::FIRST_FIELD);
                                    let discr = self.build_int_const(&tag.ty, value.into());
                                    self.store_immediate_mem(tag, discr);
                                }
                                CodegenPlace::Ssa(..) => {
                                    todo!("Ssa variants")
                                }
                            }
                        }
                        layout::TagEncoding::Uninhabited => (),
                    }
                }
            },
            mir::Rvalue::Repeat { ty, value, count } => {
                let ty = Scheme::new(ty.clone()).bind(self.args);
                let ty_layout = self.layout_for(&ty);

                let byte_size = self.build_int_const(&Type::UINT, ty_layout.size.in_bytes().into());
                let byte_align =
                    self.build_int_const(&Type::UINT, ty_layout.alignment.in_bytes().into());
                let value = self.eval_operand(value);
                let count = self
                    .eval_operand(count)
                    .force_immediate_value(self)
                    .unwrap()
                    .first_value();
                let byte_size = self.builder.ins().imul(byte_size, count);

                let ptr = self.codegen_alloc_call(byte_size, byte_align);
                if ty_layout.is_zst() {
                    let place = self.eval_place(place).unwrap();
                    self.store_immediate_pair(place, ptr, count, layout::POINTER_SIZE);
                    return;
                }
                if ty_layout.size == Size::BYTE {
                    let value = value.force_immediate_value(self).unwrap().first_value();
                    self.builder
                        .call_memset(self.target_config, ptr, value, byte_size);
                    let place = self.eval_place(place).unwrap();
                    self.store_immediate_pair(place, ptr, count, layout::POINTER_SIZE);
                    return;
                }
                /*
                   let ptr = kli_alloc(size,align);
                   let mut i = 0;
                   while i < count{
                       offset(ptr,i)^ = value;
                       i = i + 1;
                   }
                */
                let loop_condition = self.builder.create_block();
                let loop_body = self.builder.create_block();
                let loop_end = self.builder.create_block();
                let loop_count = self.builder.declare_var(ir::types::I64);
                {
                    let zero_value = self.build_int_const(&Type::UINT, 0);
                    self.builder.def_var(loop_count, zero_value);
                    self.builder.ins().jump(loop_condition, &[]);
                }

                {
                    self.builder.switch_to_block(loop_condition);
                    let current_loop_count = self.builder.use_var(loop_count);
                    let loop_count_lesser_than_len = self.builder.ins().icmp(
                        ir::condcodes::IntCC::UnsignedLessThan,
                        current_loop_count,
                        count,
                    );
                    self.builder.ins().brif(
                        loop_count_lesser_than_len,
                        loop_body,
                        [],
                        loop_end,
                        [],
                    );
                }
                {
                    self.builder.switch_to_block(loop_body);
                    let var_use = self.builder.use_var(loop_count);
                    let element_size: i64 = ty_layout.size.in_bytes_i64();
                    let new_offset = self.builder.ins().imul_imm_u(var_use, element_size);
                    let ptr = self.builder.ins().iadd(ptr, new_offset);
                    let place_value = MemPlace::new(ptr, ty_layout.clone(), ty.clone());
                    self.store_operand_with_mem_place(place_value, value);

                    let one_value = self.build_int_const(&Type::UINT, 1);
                    let new_val = self.builder.ins().iadd(var_use, one_value);
                    self.builder.def_var(loop_count, new_val);

                    self.builder.ins().jump(loop_condition, []);
                }
                self.builder.switch_to_block(loop_end);
                let place = self.eval_place(place).unwrap();
                self.store_immediate_pair(place, ptr, count, layout::POINTER_SIZE);
            }
            mir::Rvalue::AllocateArray(ty, fields) => {
                let ty = Scheme::new(ty.clone()).bind(self.args);
                let ty_layout = self.layout_for(&ty);
                let len: u64 = fields.len().try_into().unwrap();
                let ptr = self.codgen_static_size_alloc_call(&ty, len);
                if ty_layout.is_zst() || fields.is_empty() {
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
                    return;
                }
                let place_value = MemPlace::new(ptr, ty_layout.clone(), ty.clone());
                for (i, operand) in fields.iter().enumerate() {
                    let i: u64 = i.try_into().unwrap();
                    let offset: u64 = ty_layout.size.in_bytes() * i;
                    let offset: i32 = offset.try_into().unwrap();
                    let value = self.eval_operand(operand);
                    self.store_operand_with_mem_place(place_value.offset_in_bytes(offset), value);
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
                self.codgen_box(place, ty, operand);
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
                let place = self.eval_place(place).unwrap();
                let array_place = self.eval_place(array_place).unwrap();
                let CodegenPlace::MemPlace(array_place) = array_place else {
                    unreachable!()
                };
                let addr = self.load_array_ptr(array_place);
                self.store_immediate(place, addr);
            }
            mir::Rvalue::Cast(kind, operand) => match kind {
                mir::CastKind::Transmute(to) => {
                    //Zero sized transmutes are no ops
                    let Ok(dst_place) = self.eval_place(place) else {
                        return;
                    };
                    /*
                        We need to reinterpret out 'from' value based of its byte representation,
                        so we store it in memory and then copy that value as a 'to'
                    */
                    let operand = self.eval_operand(operand);
                    let to_ty = Scheme::new(to.clone()).bind(self.args);
                    let to_layout = self.layout_for(&to_ty);

                    let from_layout = self.layout_for(&operand.ty);
                    match (backend_repr(&from_layout), backend_repr(&to_layout)) {
                        (BackendRepr::Scalar(from), BackendRepr::Scalar(to)) => {
                            let from = scalar_to_cranelift_type(from);
                            let to = scalar_to_cranelift_type(to);
                            let mut value =
                                operand.force_immediate_value(self).unwrap().first_value();
                            if from != to {
                                value = self.builder.ins().bitcast(to, MemFlagsData::new(), value);
                            }
                            self.store_immediate(dst_place, value);
                            return;
                        }
                        _ if from_layout == to_layout => {
                            self.store_value(place, operand);
                            return;
                        }
                        _ => (),
                    }

                    let intermidiate_slot =
                        self.builder.create_sized_stack_slot(ir::StackSlotData::new(
                            ir::StackSlotKind::ExplicitSlot,
                            to_layout.size.in_bytes_u32(),
                            to_layout.alignment.pow_of_2(),
                        ));
                    let intermidiate_slot = self.builder.ins().stack_addr(
                        PTR_IR_TYPE,
                        intermidiate_slot,
                        Offset32::new(0),
                    );

                    let transmuted_place = MemPlace::new(intermidiate_slot, to_layout, to_ty);
                    self.store_operand_with_mem_place(transmuted_place.clone(), operand);
                    self.copy(CodegenPlace::MemPlace(transmuted_place), dst_place);
                }
                mir::CastKind::IntegerCast(cast) => match cast {
                    mir::IntegerCast::ZeroExtendByteTo(kind) => {
                        let ty = ir::types::I64;
                        let value = self
                            .eval_operand(operand)
                            .force_immediate_value(self)
                            .unwrap()
                            .first_value();
                        let value = match kind {
                            IntegerKind::Unsigned => self.builder.ins().uextend(ty, value),
                            IntegerKind::Signed => self.builder.ins().uextend(ty, value),
                        };
                        let place = self.eval_place(place).unwrap();
                        self.store_immediate(place, value);
                    }
                },
            },
            mir::Rvalue::Len(array_place) => {
                let place = self.eval_place(place).unwrap();
                let array_place = self.eval_place(array_place).unwrap();
                let CodegenPlace::MemPlace(array_place) = array_place else {
                    unreachable!("arrays never ssa")
                };
                let len_value = self.load_array_len(array_place);
                self.store_immediate(place, len_value);
            }
            mir::Rvalue::Discriminant(variant_place) => {
                let Ok(dst_place) = self.eval_place(place) else {
                    unreachable!()
                };
                let (tag_value, extend) = match self.eval_place(variant_place) {
                    Ok(place) => match place {
                        CodegenPlace::MemPlace(place) => {
                            let tag_place = place.project_field(self.ctxt, FieldId::FIRST_FIELD);
                            let value = self.load_place_mem(&tag_place).unwrap().first_value();
                            (value, !tag_place.ty.is_integer())
                        }
                        CodegenPlace::Ssa(ty, .., var) => {
                            let (id, ..) = ty.as_named().unwrap();
                            let tag_ty = self.ctxt.type_def(id).tag_type().into_type();
                            (self.builder.use_var(var), !tag_ty.is_integer())
                        }
                    },
                    Err(_) => {
                        return;
                    }
                };
                let value = if extend {
                    self.builder.ins().uextend(ir::types::I64, tag_value)
                } else {
                    tag_value
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
            let local_kind = self.local_info.info_for(local).kind;
            let ty = self.local_info.info_for(local).ty.clone();
            if matches!(local_kind, LocalKind::Memory(_) | LocalKind::Scalar(..))
                && let Ok(place) = self.eval_place(&Place::local(local))
            {
                let (param_index, scalar_ty) = param_values[i];
                let Some(scalar_ty) = scalar_ty else {
                    continue;
                };
                let value = match scalar_ty {
                    ScalarType::Single(_) => ScalarValue::Single(
                        self.builder.block_params(self.block_map.entry())[param_index],
                    ),
                };
                match local_kind {
                    LocalKind::Memory(_) => {
                        let layout = self.layout_for(&ty);
                        if self.ctxt.config().has_feature(Feature::OutputBackendIr) {
                            println!("{} {} {:?}", param_index, ty, backend_repr(&layout));
                        }
                        if let BackendRepr::Memory = backend_repr(&layout) {
                            let ptr = value.first_value();
                            let src_place = MemPlace::new(ptr, layout, ty);
                            self.copy(CodegenPlace::MemPlace(src_place), place);
                        } else {
                            self.store_scalar(place, value);
                        }
                    }
                    LocalKind::Scalar(_) => {
                        self.store_scalar(place, value);
                    }
                    _ => (),
                }
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
            self.codegen_direct_call(None, self.runtime_functions.panic, &[]);
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
                    mir::StmtKind::Print(operand) => {
                        if let Some(operand) = operand {
                            let operand = self.eval_operand(operand);
                            match operand.ty {
                                Type::Int(_) => {
                                    let value = operand
                                        .force_immediate_value(&mut self)
                                        .unwrap()
                                        .first_value();
                                    self.codegen_direct_void_call(
                                        self.runtime_functions.print_int,
                                        &[value],
                                    );
                                }
                                Type::Bool => {
                                    let value = operand
                                        .force_immediate_value(&mut self)
                                        .unwrap()
                                        .first_value();
                                    let true_value = self.build_string_value("true".to_string());
                                    let false_value = self.build_string_value("false".to_string());

                                    let (first_value, second_value) = (
                                        self.builder.ins().select(
                                            value,
                                            true_value.0,
                                            false_value.0,
                                        ),
                                        self.builder.ins().select(
                                            value,
                                            true_value.1,
                                            false_value.1,
                                        ),
                                    );
                                    self.print_string_newline(first_value, second_value);
                                }
                                Type::String => {
                                    let (first_val, second_val) = match operand.kind {
                                        OperandValueKind::Indirect(CodegenPlace::MemPlace(
                                            place,
                                        )) => {
                                            let first_val = self.load_array_ptr(place.clone());
                                            let second_val = self.load_array_len(place);
                                            (first_val, second_val)
                                        }
                                        OperandValueKind::Value(ScalarValue::Pair(
                                            [first, second],
                                            _,
                                        )) => (first, second),
                                        _ => unreachable!("{:?}", operand.kind),
                                    };
                                    self.print_string_newline(first_val, second_val);
                                }
                                ref ty => todo!("print for {} {:?}", ty, ty),
                            }
                        } else {
                            let values = self.build_string_value("".to_string());
                            self.print_string_newline(values.0, values.1);
                        }
                    }
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
                    //If it negates than we are asserting !operand instead of operand
                    let assert_success = if assert_kind.negate() {
                        let cond_value = self.build_int_const(&Type::Bool, 0);
                        self.builder
                            .ins()
                            .icmp(ir::condcodes::IntCC::Equal, value, cond_value)
                    } else {
                        let cond_value = self.build_int_const(&Type::Bool, 0);
                        self.builder
                            .ins()
                            .icmp(ir::condcodes::IntCC::NotEqual, value, cond_value)
                    };
                    let fail_block = self.builder.create_block();
                    self.builder.ins().brif(
                        assert_success,
                        block_map.blocks[*basic_block_id].unwrap(),
                        &[],
                        fail_block,
                        &[],
                    );

                    self.builder.switch_to_block(fail_block);
                    let (first, second) = self.build_string_value(match *assert_kind {
                        AssertKind::DivideByZero => {
                            "attempted to compute division by 0".to_string()
                        }
                        AssertKind::InBounds => {
                            "attempted to access out of bounds index".to_string()
                        }
                        AssertKind::Overflow(op) => format!(
                            "failed to compute '{}' due to overflow",
                            match op {
                                OverflowOp::Add => '+',
                                OverflowOp::Multiply => '*',
                                OverflowOp::Subtract => '-',
                            }
                        ),
                        AssertKind::DivideOverflow => {
                            "attempted to compute overflowing division".to_string()
                        }
                    });
                    self.print_string_newline(first, second);
                    self.builder.ins().trap(code);
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
                    let rvalue = self.eval_place(&Place::return_place()).ok();
                    let mode = self.functions.functions[&Instance {
                        args: self.args.iter().cloned().collect(),
                        kind: body.src.as_instance(),
                    }]
                        .abi
                        .ret;

                    match mode {
                        //Don't do anything
                        PassMode::Void | PassMode::ByPtr => {
                            self.builder.ins().return_(&[]);
                        }
                        //Write return value to pointer
                        //Just return
                        PassMode::ByValue(_) => {
                            let rvalue = rvalue.unwrap();
                            let value = self.load_place(&rvalue).unwrap();
                            self.builder.ins().return_(value.as_slice());
                        }
                    }
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
#[derive(Default)]
struct Constants {
    next_constant: usize,
    constants: HashMap<Box<[u8]>, cranelift_module::DataId>,
}
impl Constants {
    pub fn constant_for(
        &mut self,
        module: &mut impl Module,
        bytes: Box<[u8]>,
    ) -> cranelift_module::DataId {
        if let Some(constant) = self.constants.get(&bytes) {
            return *constant;
        }
        let name = format!("const_{}", self.next_constant);
        let data_id = module
            .declare_data(&name, cranelift_module::Linkage::Local, false, false)
            .unwrap();
        let mut data = cranelift_module::DataDescription::new();
        data.define(bytes);
        module.define_data(data_id, &data).unwrap();
        self.next_constant += 1;
        data_id
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
