use std::collections::{HashMap, hash_map::Entry};

use crate::{
    ast::{BinaryOp, IsResource},
    interpret::{
        functions::FunctionInfo,
        ints::Int,
        memory::{Byte, MemLocation, Memory},
        repr::{align_of, decode, encode, is_resource, offsets_of, size_of},
        values::{Pointer, StringValue, Value},
    },
    resolved_ast::{Builtin, FunctionId, VarId},
    typed_ast::{self, FieldId},
    types::{FunctionType, GenericArg, GenericKind, RecordField, Type},
};

mod functions;
mod ints;
mod memory;
mod repr;
mod values;
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Endianess {
    Little,
    Big,
}
#[derive(Debug)]
pub enum InterpretError {
    Panic,
    Overflow,
    DivideByZero,
    DoubleFree,
    FreeInvalid,
    BaseMismatch,
    DeallocMismatch {
        expected: MemLocation,
        got: MemLocation,
    },
    OutOfBounds {
        len: usize,
        offset: isize,
    },
    InvalidPointer,
    UsedDeallocatedMemory,
    NotEnoughBytes,
    InvalidValue,
    NotUtf8,
    ReadUninit,
    UninitByteInInt,
    UninitByteInPointer,
    UninitByteInChar,
    InvalidDiscriminant(Byte),
    UninitInPointer,
    CalledNonFunction,
    UseAfterMove,
    ReachedUnreachable,
}
pub const INT_SIZE: usize = 8;
pub const ADDR_SIZE: usize = 8;
fn simplify_ty(g: &HashMap<usize, Type>, ty: Type) -> Type {
    match ty {
        Type::Bool
        | Type::Char
        | Type::Int
        | Type::Unit
        | Type::String
        | Type::ClosureEnv
        | Type::Unknown => ty,
        Type::Infer(_) => unreachable!(),
        Type::List(element) => Type::List(Box::new(simplify_ty(g, *element))),
        Type::Option(inner) => Type::Option(Box::new(simplify_ty(g, *inner))),
        Type::Box(inner) => Type::Box(Box::new(simplify_ty(g, *inner))),
        Type::Mut(region, ty) => Type::Mut(region, Box::new(simplify_ty(g, *ty))),
        Type::Imm(region, ty) => Type::Imm(region, Box::new(simplify_ty(g, *ty))),
        Type::Function(FunctionType {
            resource,
            params,
            return_type,
        }) => Type::Function(FunctionType {
            resource,
            params: params
                .into_iter()
                .map(|param| simplify_ty(g, param))
                .collect(),
            return_type: Box::new(simplify_ty(g, *return_type)),
        }),
        Type::Record(fields) => Type::Record(
            fields
                .into_iter()
                .map(|field| RecordField {
                    name: field.name,
                    ty: simplify_ty(g, field.ty),
                })
                .collect(),
        ),
        Type::Param(_, i) => g.get(&i).cloned().expect("No generic arg"),
    }
}
struct Env {
    pointer: Pointer,
    fields: HashMap<VarId, (Type, bool, usize)>,
}
struct Frame<'f> {
    f: FunctionInfo<'f>,
    generic_args: Vec<Type>,
    vars: HashMap<VarId, (Type, bool, Pointer)>,
    locals: Vec<Pointer>,
    scope: Vec<Vec<VarId>>,
    captured_vars: Option<Env>,
}
pub struct CaptureInfo {
    pub size: usize,
    pub info: Vec<(VarId, Type, usize)>,
}
pub struct LambdaInfo<'f> {
    pub info: FunctionInfo<'f>,
    pub captures: Option<CaptureInfo>,
    pub generic_args: Vec<Type>,
}
pub struct Interpret<'f> {
    functions: HashMap<FunctionId, (FunctionInfo<'f>, HashMap<Vec<Type>, Pointer>)>,
    builtin_functions: HashMap<Builtin, HashMap<Vec<Type>, Pointer>>,
    lambdas: HashMap<Pointer, LambdaInfo<'f>>,
    entry: FunctionId,
    call_stack: Vec<Frame<'f>>,
    memory: Memory,
    endianness: Endianess,
}
impl<'f> Interpret<'f> {
    pub fn new(e: Endianess, functions: &'f [typed_ast::Function]) -> Self {
        let entry = functions
            .iter()
            .position(|f| &f.name.content as &str == "main")
            .map(FunctionId::new)
            .expect("Should have an entry point");
        let mut mem = Memory::new();
        let functions = functions
            .iter()
            .enumerate()
            .map(|(i, function)| {
                (
                    FunctionId::new(i),
                    (
                        FunctionInfo {
                            generics: &function.generics,
                            params: &function.params,
                            body: function.body.as_ref(),
                        },
                        if function.generics.is_empty() {
                            HashMap::from([(Vec::new(), mem.allocate(MemLocation::Function, 0))])
                        } else {
                            HashMap::new()
                        },
                    ),
                )
            })
            .collect();
        let mut builtin_functions = HashMap::new();
        builtin_functions.insert(Builtin::AllocBox, HashMap::new());
        builtin_functions.insert(Builtin::DeallocBox, HashMap::new());
        builtin_functions.insert(Builtin::DerefBox, HashMap::new());
        builtin_functions.insert(Builtin::DerefBoxMut, HashMap::new());
        builtin_functions.insert(Builtin::Swap, HashMap::new());
        builtin_functions.insert(Builtin::Replace, HashMap::new());
        builtin_functions.insert(Builtin::DestroyString, HashMap::new());
        builtin_functions.insert(Builtin::Freeze, HashMap::new());
        Self {
            functions,
            entry,
            call_stack: vec![],
            builtin_functions,
            memory: mem,
            lambdas: HashMap::new(),
            endianness: e,
        }
    }
    fn drop_closure_env(
        &mut self,
        env: Pointer,
        captures: Vec<(VarId, Type, usize)>,
    ) -> Result<(), InterpretError> {
        for (_, ref ty, offset) in captures {
            let pointer = self.memory.byte_offset_in_bounds(env, offset as isize)?;
            self.drop(ty, pointer)?;
        }
        self.memory.deallocate(MemLocation::Heap, env)?;
        Ok(())
    }
    fn drop(&mut self, ty: &Type, pointer_to_place: Pointer) -> Result<(), InterpretError> {
        match ty {
            Type::ClosureEnv => unreachable!("Cannot drop closure env"),
            Type::Bool | Type::Int | Type::Unit | Type::Imm(..) | Type::Mut(..) | Type::Char => {
                Ok(())
            }
            Type::Box(inner_ty) => {
                let inner = self.typed_read(pointer_to_place, ty)?;
                let inner = inner.as_pointer().unwrap();
                self.drop(inner_ty, inner)?;
                self.memory.deallocate(MemLocation::Heap, inner)
            }
            Type::String => {
                let StringValue {
                    pointer,
                    cap: _,
                    len: _,
                } = self
                    .typed_read(pointer_to_place, ty)?
                    .into_string()
                    .unwrap();
                self.memory.deallocate(MemLocation::Heap, pointer)?;
                Ok(())
            }
            Type::Unknown | Type::Infer(_) => unreachable!("Cannot have infer or unknown"),
            Type::Option(inner_ty) => {
                let is_some = self
                    .typed_read(pointer_to_place, &Type::Bool)?
                    .as_bool()
                    .unwrap();
                if is_some {
                    let inner_ty = self.simplify_ty((**inner_ty).clone());
                    let pointer_to_inner = self
                        .memory
                        .byte_offset_in_bounds(pointer_to_place, align_of(&inner_ty) as isize)?;
                    self.drop(&inner_ty, pointer_to_inner)
                } else {
                    Ok(())
                }
            }
            Type::Function(FunctionType { resource, .. }) => match resource {
                IsResource::Data => Ok(()),
                IsResource::Resource => {
                    let (env, code) = self.typed_read(pointer_to_place, ty)?.into_pair().unwrap();
                    let env = env.as_pointer().unwrap();
                    let code_ptr = code.as_pointer().unwrap();
                    let LambdaInfo {
                        info: _,
                        captures,
                        generic_args: _,
                    } = &self.lambdas[&code_ptr];
                    let CaptureInfo {
                        size: _,
                        info: captures,
                    } = captures.as_ref().unwrap();
                    self.drop_closure_env(env, captures.clone())
                }
            },
            Type::Record(fields) => {
                let tys = fields
                    .iter()
                    .map(|field| self.simplify_ty(field.ty.clone()))
                    .collect::<Vec<_>>();
                let (_, offsets) = offsets_of(&tys);
                for (ty, offset) in tys.into_iter().zip(offsets) {
                    let pointer = self
                        .memory
                        .byte_offset_in_bounds(pointer_to_place, offset as isize)?;
                    self.drop(&ty, pointer)?;
                }
                Ok(())
            }
            Type::Param(..) => unreachable!("Cant have params"),
            Type::List(element_type) => {
                let element_type = self.simplify_ty((**element_type).clone());
                let size = size_of(&element_type);
                let [ptr, cap, len] = self
                    .typed_read(pointer_to_place, ty)?
                    .into_tuple()
                    .expect("Should be a tuple")
                    .try_into()
                    .expect("Should be 3-tuple");
                let ptr = ptr.as_pointer().expect("Should be a ptr");
                let _ = cap.into_int().expect("Should be an int");
                let len = len.into_int().expect("Should be an int");
                for i in 0..len.into_size() {
                    self.drop(
                        &element_type,
                        self.memory
                            .byte_offset_in_bounds(ptr, (i * size).try_into().unwrap())?,
                    )?;
                }
                self.memory.deallocate(MemLocation::Heap, ptr)?;
                Ok(())
            }
        }
    }
    fn in_drop_scope<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, InterpretError>,
    ) -> Result<T, InterpretError> {
        self.call_stack.last_mut().unwrap().scope.push(Vec::new());
        let value = f(self);
        let vars = self.call_stack.last_mut().unwrap().scope.pop().unwrap();
        for var in vars {
            let &(ref ty, moved, pointer) = &self.call_stack.last_mut().unwrap().vars[&var];
            if !moved {
                let ty = ty.clone();
                self.drop(&ty, pointer)?;
                self.memory.deallocate(MemLocation::Stack, pointer)?;
            }
        }
        value
    }
    fn current_frame<'a>(&'a self) -> &'a Frame<'f> {
        self.call_stack.last().unwrap()
    }
    fn simplify_ty(&self, ty: Type) -> Type {
        let frame = self.current_frame();
        let params = frame.f.generics;
        let generic_arg_map = params
            .iter()
            .enumerate()
            .filter_map(|(i, param)| {
                let GenericKind::Type = param.kind else {
                    return None;
                };
                Some(i)
            })
            .enumerate()
            .map(|(generic_index, ty_index)| (generic_index, frame.generic_args[ty_index].clone()))
            .collect();
        simplify_ty(&generic_arg_map, ty)
    }
    fn allocate_local(&mut self, ty: &Type) -> Pointer {
        let ty = &self.simplify_ty(ty.clone());
        let pointer = self.memory.allocate(MemLocation::Stack, size_of(ty));
        self.call_stack.last_mut().unwrap().locals.push(pointer);
        pointer
    }
    fn generic_args_to_instance_args(args: Vec<GenericArg>) -> Vec<Type> {
        args.into_iter()
            .filter_map(|arg| match arg {
                GenericArg::Type(ty) => Some(ty),
                _ => None,
            })
            .collect()
    }
    fn typed_write(
        &mut self,
        pointer: Pointer,
        ty: &Type,
        value: Value,
    ) -> Result<(), InterpretError> {
        let ty = &self.simplify_ty(ty.clone());
        self.memory
            .write(pointer, encode(self.endianness, ty, value))
    }
    fn typed_read(&self, pointer: Pointer, ty: &Type) -> Result<Value, InterpretError> {
        let ty = &self.simplify_ty(ty.clone());
        let bytes = self.memory.read(pointer, size_of(ty))?;
        decode(self.endianness, ty, &bytes)
    }
    fn pointer_to_var(
        &mut self,
        as_move: bool,
        var: VarId,
    ) -> Result<(Pointer, Type), InterpretError> {
        for frame in self.call_stack.iter_mut().rev() {
            if let Some(Env {
                pointer: env,
                fields: ref mut captures,
            }) = frame.captured_vars
                && let Some(&mut (ref ty, ref mut moved, offset)) = captures.get_mut(&var)
                && let ty = ty.clone()
            {
                if as_move && is_resource(&ty) {
                    *moved = true;
                }
                return self
                    .memory
                    .byte_offset_in_bounds(env, offset as isize)
                    .map(|p| (p, ty));
            }
        }
        let frame = self.call_stack.last_mut().unwrap();
        let (ty, moved, pointer) = frame
            .vars
            .get_mut(&var)
            .unwrap_or_else(|| panic!("The var '{:?}' should be here ", var));
        if *moved && as_move {
            return Err(InterpretError::UseAfterMove);
        }
        if as_move && is_resource(ty) {
            *moved = true;
        }
        Ok((*pointer, ty.clone()))
    }
    fn pointer_to_place(
        &mut self,
        as_move: bool,
        place: &'f typed_ast::Place,
    ) -> Result<Pointer, InterpretError> {
        match &place.kind {
            typed_ast::PlaceKind::Var(var) => Ok(self.pointer_to_var(as_move, var.1)?.0),
            typed_ast::PlaceKind::Deref(value) => match &value.kind {
                typed_ast::ExprKind::Load(place) => {
                    let place_pointer = self.pointer_to_place(false, place)?;
                    let pointer = self
                        .typed_read(place_pointer, &place.ty)?
                        .as_pointer()
                        .unwrap();
                    Ok(pointer)
                }
                _ => {
                    let value = self.interpret_expr(value)?;
                    let pointer = value.as_pointer().unwrap();
                    Ok(pointer)
                }
            },
        }
    }
    fn alloc_var(&mut self, var: VarId, ty: &Type) -> Pointer {
        let pointer = self.allocate_local(ty);
        let frame = self.call_stack.last_mut().unwrap();
        frame.vars.insert(var, (ty.clone(), false, pointer));
        frame.scope.last_mut().unwrap().push(var);
        pointer
    }
    fn matches_pattern(
        &mut self,
        pattern: &typed_ast::Pattern,
        pointer_to_place: Pointer,
    ) -> Result<bool, InterpretError> {
        match &pattern.kind {
            typed_ast::PatternKind::Binding(..) => Ok(true),
            typed_ast::PatternKind::Ref(inner) => {
                let pointer = self
                    .typed_read(pointer_to_place, &pattern.ty)?
                    .as_pointer()
                    .unwrap();
                self.matches_pattern(inner, pointer)
            }
            &typed_ast::PatternKind::Int(matched_value) => {
                let value = self
                    .typed_read(pointer_to_place, &Type::Int)?
                    .into_int()
                    .unwrap();
                Ok(value == Int::new(matched_value))
            }
            &typed_ast::PatternKind::Bool(matched_value) => {
                let value = self
                    .typed_read(pointer_to_place, &Type::Bool)?
                    .as_bool()
                    .unwrap();
                Ok(value == matched_value)
            }
            typed_ast::PatternKind::Some(inner) => {
                let is_some = self
                    .typed_read(pointer_to_place, &Type::Bool)?
                    .as_bool()
                    .unwrap();
                if is_some {
                    let pointer = self.memory.byte_offset_in_bounds(
                        pointer_to_place,
                        align_of(&self.simplify_ty(inner.ty.clone())) as isize,
                    )?;
                    self.matches_pattern(inner, pointer)
                } else {
                    Ok(false)
                }
            }
            typed_ast::PatternKind::None => {
                let is_some = self
                    .typed_read(pointer_to_place, &Type::Bool)?
                    .as_bool()
                    .unwrap();
                if is_some { Ok(false) } else { Ok(true) }
            }
            typed_ast::PatternKind::Record(fields) => {
                let field_tys = fields
                    .iter()
                    .map(|field| self.simplify_ty(field.pattern.ty.clone()))
                    .collect::<Vec<_>>();
                let (_, offsets) = offsets_of(&field_tys);
                for (offset, field) in offsets.into_iter().zip(fields) {
                    let pointer = self
                        .memory
                        .byte_offset_in_bounds(pointer_to_place, offset as isize)?;
                    if !self.matches_pattern(&field.pattern, pointer)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }
    fn assign_to_pattern(
        &mut self,
        pattern: &typed_ast::Pattern,
        pointer_to_place: Pointer,
    ) -> Result<(), InterpretError> {
        match &pattern.kind {
            typed_ast::PatternKind::Binding(borrow, _, var, ty) => {
                let pointer = self.alloc_var(var.1, ty);
                if borrow.is_some() {
                    self.typed_write(pointer, ty, Value::Pointer(pointer_to_place))?;
                } else {
                    self.typed_write(pointer, ty, self.typed_read(pointer_to_place, ty)?)?;
                }
                Ok(())
            }
            typed_ast::PatternKind::Ref(inner) => {
                let pointer = self
                    .typed_read(pointer_to_place, &pattern.ty)?
                    .as_pointer()
                    .unwrap();
                self.assign_to_pattern(inner, pointer)
            }
            &typed_ast::PatternKind::Bool(_) | typed_ast::PatternKind::Int(_) => Ok(()),
            typed_ast::PatternKind::Some(inner) => {
                let is_some = self
                    .typed_read(pointer_to_place, &Type::Bool)?
                    .as_bool()
                    .unwrap();
                if is_some {
                    let pointer = self.memory.byte_offset_in_bounds(
                        pointer_to_place,
                        align_of(&self.simplify_ty(inner.ty.clone())) as isize,
                    )?;
                    self.assign_to_pattern(inner, pointer)
                } else {
                    Ok(())
                }
            }
            typed_ast::PatternKind::None => Ok(()),
            typed_ast::PatternKind::Record(fields) => {
                let field_tys = fields
                    .iter()
                    .map(|field| self.simplify_ty(field.pattern.ty.clone()))
                    .collect::<Vec<_>>();
                let (_, offsets) = offsets_of(&field_tys);
                for (offset, field) in offsets.into_iter().zip(fields) {
                    let pointer = self
                        .memory
                        .byte_offset_in_bounds(pointer_to_place, offset as isize)?;
                    self.assign_to_pattern(&field.pattern, pointer)?;
                }
                Ok(())
            }
        }
    }
    fn interpet_stmt(&mut self, stmt: &'f typed_ast::Stmt) -> Result<(), InterpretError> {
        match &stmt.kind {
            typed_ast::StmtKind::Expr(expr) => {
                let value = self.interpret_expr(expr)?;
                if is_resource(&expr.ty) {
                    let pointer = self.allocate_local(&expr.ty);
                    self.typed_write(pointer, &expr.ty, value)?;
                    self.drop(&expr.ty, pointer)?;
                }
            }
            typed_ast::StmtKind::Let(let_binding) => {
                let tmp_pointer = self.allocate_local(&let_binding.pattern.ty);
                let value = self.interpret_expr(&let_binding.value)?;
                self.typed_write(tmp_pointer, &let_binding.pattern.ty, value)?;
                self.assign_to_pattern(&let_binding.pattern, tmp_pointer)?;
            }
        }
        Ok(())
    }
    fn print_value(&self, value: Value, ty: &Type) -> Result<(), InterpretError> {
        match ty {
            Type::ClosureEnv => {
                let value = value.as_pointer().unwrap();
                println!("{}", value.address);
            }
            Type::Bool => {
                let value = value.as_bool().expect("Should be a bool");
                print!("{}", value)
            }
            Type::Int => {
                let value = value.into_int().expect("Should be an int");
                print!("{}", value)
            }
            Type::Unit => {
                print!("()")
            }
            Type::Option(ty) => {
                let value = value.into_option().expect("Should be an option");
                match value {
                    Some(value) => {
                        print!("Some(");
                        self.print_value(value, ty)?;
                        print!(")")
                    }
                    None => print!("None"),
                }
            }
            Type::Record(fields) => {
                let field_values = value.into_tuple().expect("Should be a record");
                print!("{{");
                let mut first = true;
                for (field, field_value) in fields.into_iter().zip(field_values) {
                    if !first {
                        print!(", ");
                    }
                    print!("{} = ", field.name);
                    self.print_value(field_value, &field.ty)?;
                    first = false;
                }
                print!("}}");
            }
            Type::Imm(.., pointee) | Type::Mut(.., pointee) | Type::Box(pointee) => {
                let pointer = value.as_pointer().unwrap();
                let value = self.typed_read(pointer, pointee)?;
                self.print_value(value, pointee)?;
            }
            Type::Function(FunctionType { resource, .. }) => match resource {
                IsResource::Data => print!("{}", value.as_pointer().unwrap().address),
                IsResource::Resource => {
                    let (env, code) = value.as_pair().unwrap();
                    let env = env.as_pointer().unwrap();
                    let code = code.as_pointer().unwrap();
                    print!("closure{{env = {:?},code = {:?}}}", env, code)
                }
            },
            Type::Char => {
                let char = value.as_char().unwrap();
                print!("{}", char)
            }
            Type::String => {
                let string = self.string_from(value.into_string().expect("Should be a string"))?;
                print!("{}", string);
            }
            Type::List(element_ty) => {
                let [ptr, cap, len] = value.into_tuple().unwrap().try_into().unwrap();
                let ptr = ptr.as_pointer().unwrap();
                let _ = cap.into_int().unwrap();
                let len = len.into_int().unwrap();
                let element_ty = self.simplify_ty((**element_ty).clone());
                let size = size_of(&element_ty);
                print!("[");
                let len = len.into_size();
                for i in 0..len {
                    self.print_value(
                        self.typed_read(
                            self.memory
                                .byte_offset_in_bounds(ptr, (i * size).try_into().unwrap())?,
                            &element_ty,
                        )?,
                        &element_ty,
                    )?;
                    if i < len - 1 {
                        print!(",")
                    }
                }
                print!("]");
            }

            Type::Param(..) => unreachable!("No generic params"),
            Type::Unknown | Type::Infer(_) => unreachable!("Cannot have this type"),
        }
        Ok(())
    }
    fn function_from_pointer(&self, p: Pointer) -> Option<(FunctionId, Vec<Type>)> {
        for (id, f) in self.functions.iter() {
            for (args, pointer) in f.1.iter() {
                if *pointer == p {
                    return Some((*id, args.clone()));
                }
            }
        }
        None
    }
    fn string_from(&self, string_value: StringValue) -> Result<String, InterpretError> {
        let StringValue {
            pointer: ptr,
            cap,
            len,
        } = string_value;
        let all_bytes = self.memory.read(ptr, len.into_size())?;
        let mut bytes = Vec::with_capacity(cap.into_size());
        for b in all_bytes {
            let value = match b {
                Byte::Init(b, _) => b,
                Byte::Uninit => {
                    return Err(InterpretError::ReadUninit);
                }
            };
            bytes.push(value);
        }
        String::from_utf8(bytes).map_err(|_| InterpretError::NotUtf8)
    }
    fn handle_iteration(
        &mut self,
        pattern: &typed_ast::Pattern,
        iter_ty: &Type,
        body: &'f typed_ast::Expr,
        iterator_value: Value,
    ) -> Result<(), InterpretError> {
        let mutable = matches!(iter_ty, Type::Mut(..));
        match &iter_ty {
            Type::Imm(region, ty) | Type::Mut(region, ty) => {
                let pointer = iterator_value.as_pointer().unwrap();
                let iterator_value = self.typed_read(pointer, ty)?;
                match &**ty {
                    Type::String => {
                        let string = self.string_from(iterator_value.into_string().unwrap())?;
                        self.in_drop_scope(|this| {
                            for c in string.chars() {
                                let local = this.allocate_local(&Type::Char);
                                this.typed_write(local, &Type::Char, Value::Char(c))?;
                                this.assign_to_pattern(pattern, local)?;
                                this.interpret_expr(body)?;
                            }
                            Ok(())
                        })
                    }
                    Type::List(ty) => {
                        let element_ty = self.simplify_ty((**ty).clone());
                        let iter_element_ty = if mutable {
                            Type::Mut(region.clone(), Box::new(element_ty))
                        } else {
                            Type::Imm(region.clone(), Box::new(element_ty))
                        };
                        let [ptr, cap, len] =
                            iterator_value.into_tuple().unwrap().try_into().unwrap();
                        let ptr = ptr.as_pointer().unwrap();
                        let _ = cap.into_int().unwrap();
                        let size = size_of(ty);
                        let len = len.into_int().unwrap();
                        for i in 0..len.into_size() {
                            self.in_drop_scope(|this| {
                                let local = this.allocate_local(&iter_element_ty);
                                this.typed_write(
                                    local,
                                    &iter_element_ty,
                                    Value::Pointer(this.memory.byte_offset_in_bounds(
                                        ptr,
                                        (i * size).try_into().unwrap(),
                                    )?),
                                )?;
                                this.assign_to_pattern(pattern, local)?;
                                this.interpret_expr(body)?;

                                Ok(())
                            })?;
                        }
                        Ok(())
                    }
                    ty => self.handle_iteration(pattern, ty, body, iterator_value),
                }
            }
            _ => unreachable!("Cant iterate these"),
        }
    }
    fn interpret_expr(&mut self, expr: &'f typed_ast::Expr) -> Result<Value, InterpretError> {
        match &expr.kind {
            typed_ast::ExprKind::Err => panic!("Cannot interpret err value"),
            typed_ast::ExprKind::Panic => Err(InterpretError::Panic),
            typed_ast::ExprKind::Int(value) => Ok(Value::Int(Int::new(*value))),
            typed_ast::ExprKind::Binary(op, left, right) => {
                let left = self.interpret_expr(left)?.into_int().unwrap();
                let right = self.interpret_expr(right)?.into_int().unwrap();
                let (res, overflow) = match op {
                    BinaryOp::Add => left.overflowing_add(right),
                    BinaryOp::Divide => {
                        if right == Int::ZERO {
                            return Err(InterpretError::DivideByZero);
                        } else {
                            left.overflowing_div(right)
                        }
                    }
                    BinaryOp::Subtract => left.overflowing_sub(right),
                    BinaryOp::Multiply => left.overflowing_mul(right),
                };
                if overflow {
                    Err(InterpretError::Overflow)
                } else {
                    Ok(Value::Int(res))
                }
            }
            typed_ast::ExprKind::Bool(value) => Ok(Value::Bool(*value)),
            typed_ast::ExprKind::Some(value) => {
                let value = self.interpret_expr(value)?;
                Ok(Value::Variant(Value::SOME_DISCRIMINANT, vec![value]))
            }
            typed_ast::ExprKind::None => Ok(Value::Variant(Value::NONE_DISCRIMINANT, vec![])),
            typed_ast::ExprKind::Block(block, _) => self.in_drop_scope(|this| {
                for stmt in &block.stmts {
                    this.interpet_stmt(stmt)?;
                }
                this.interpret_expr(&block.expr)
            }),
            typed_ast::ExprKind::Print(value) => {
                if let Some(arg) = value {
                    let value = self.interpret_expr(arg)?;
                    self.print_value(value, &arg.ty)?;
                }
                println!();
                Ok(Value::unit())
            }
            typed_ast::ExprKind::Unit => Ok(Value::unit()),
            typed_ast::ExprKind::Record(fields) => {
                let mut fields_values = fields
                    .iter()
                    .map(|field| {
                        self.interpret_expr(&field.value)
                            .map(|value| (field.index, value))
                    })
                    .collect::<Result<HashMap<_, _>, _>>()?;
                let fields = (0..fields.len())
                    .map(FieldId::new)
                    .map(|i| fields_values.remove(&i).expect("Should be a field here"))
                    .collect::<Vec<_>>();
                Ok(Value::Tuple(fields))
            }
            typed_ast::ExprKind::For {
                pattern,
                iterator,
                body,
            } => {
                let iterator_value = self.interpret_expr(iterator)?;
                self.handle_iteration(pattern, &iterator.ty, body, iterator_value)?;
                Ok(Value::unit())
            }
            typed_ast::ExprKind::Case(scrutinee, arms) => {
                let tmp = self.allocate_local(&scrutinee.ty);
                let value = self.interpret_expr(scrutinee)?;
                self.typed_write(tmp, &scrutinee.ty, value)?;
                for arm in arms {
                    let value = self.in_drop_scope(|this| {
                        let matched = this.matches_pattern(&arm.pattern, tmp)?;
                        if matched {
                            this.assign_to_pattern(&arm.pattern, tmp)?;
                            Ok(Some(this.interpret_expr(&arm.body)?))
                        } else {
                            Ok(None)
                        }
                    })?;
                    if let Some(value) = value {
                        return Ok(value);
                    }
                }
                Err(InterpretError::ReachedUnreachable)
            }
            typed_ast::ExprKind::String(value) => {
                let pointer = self.memory.allocate(MemLocation::Heap, value.len());
                self.memory.write(
                    pointer,
                    value.bytes().map(|b| Byte::Init(b, None)).collect(),
                )?;
                Ok(Value::Tuple(vec![
                    Value::Pointer(pointer),
                    Value::Int(Int::from_size(value.len())),
                    Value::Int(Int::from_size(value.len())),
                ]))
            }
            typed_ast::ExprKind::Function(_, id, args) => {
                let args = Self::generic_args_to_instance_args(args.to_vec());
                Ok(Value::Pointer(
                    match self.functions.get_mut(id).unwrap().1.entry(args) {
                        Entry::Occupied(occupied) => *occupied.get(),
                        Entry::Vacant(vacant) => *vacant
                            .insert_entry(self.memory.allocate(MemLocation::Function, 0))
                            .get(),
                    },
                ))
            }
            typed_ast::ExprKind::Call(callee, args) => {
                let callee_value = self.interpret_expr(callee)?;
                let args = args
                    .iter()
                    .map(|arg| self.interpret_expr(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let Type::Function(FunctionType { resource, .. }) = callee.ty else {
                    unreachable!("Can only call functions")
                };
                self.call_value(resource, callee_value, args)
            }
            typed_ast::ExprKind::Builtin(builtin, args) => self
                .builtin_functions
                .get_mut(builtin)
                .map(|instances| {
                    let args = Self::generic_args_to_instance_args(args.to_vec());
                    let function_ptr = *instances
                        .entry(args)
                        .or_insert_with(|| self.memory.allocate(MemLocation::Function, 0));
                    Ok(Value::Pointer(function_ptr))
                })
                .unwrap_or_else(|| panic!("{:?} should be supported", builtin)),
            typed_ast::ExprKind::Borrow {
                mutable: _,
                place,
                region: _,
            } => Ok(Value::Pointer(self.pointer_to_place(false, place)?)),
            typed_ast::ExprKind::Load(place) => {
                let pointer = self.pointer_to_place(true, place)?;
                self.typed_read(pointer, &place.ty)
            }
            typed_ast::ExprKind::Assign(place, value) => {
                let value = self.interpret_expr(value)?;
                let pointer = self.pointer_to_place(false, place)?;
                if is_resource(&place.ty) {
                    let var = self.call_stack.last_mut().unwrap().vars.iter().find_map(
                        |(id, (_, _, p))| {
                            if *p == pointer { Some(*id) } else { None }
                        },
                    );
                    if let Some(var) = var {
                        let &(_, moved, _) = &self.call_stack.last().unwrap().vars[&var];
                        if !moved {
                            self.drop(&place.ty, pointer)?;
                        }
                        let (_, moved, _) = self
                            .call_stack
                            .last_mut()
                            .unwrap()
                            .vars
                            .get_mut(&var)
                            .unwrap();
                        *moved = false;
                    }
                }
                self.typed_write(pointer, &place.ty, value)?;
                Ok(Value::unit())
            }
            typed_ast::ExprKind::Lambda(lambda) => {
                let code_ptr = self.memory.allocate(MemLocation::Function, 0);
                let generic_args = self.current_frame().generic_args.clone();
                match lambda.is_resource {
                    IsResource::Data => {
                        self.lambdas.insert(
                            code_ptr,
                            LambdaInfo {
                                info: FunctionInfo {
                                    generics: self.call_stack.last().unwrap().f.generics,
                                    params: &lambda.params,
                                    body: Some(&lambda.body),
                                },
                                captures: None,
                                generic_args,
                            },
                        );
                        Ok(Value::Pointer(code_ptr))
                    }
                    IsResource::Resource => {
                        let captures = lambda
                            .captures
                            .iter()
                            .map(|var| {
                                let (pointer, ty) = self.pointer_to_var(true, *var).unwrap();
                                let ty = self.simplify_ty(ty);
                                (*var, ty, pointer)
                            })
                            .collect::<Vec<_>>();
                        let tys = captures
                            .iter()
                            .map(|(_, ty, _)| ty.clone())
                            .collect::<Vec<_>>();
                        let (size, offsets) = offsets_of(&tys);
                        let env = self.memory.allocate(MemLocation::Heap, size);
                        for (capture, offset) in captures.iter().zip(offsets.iter().copied()) {
                            let &(_, ref ty, pointer_to_var) = capture;
                            let pointer =
                                self.memory.byte_offset_in_bounds(env, offset as isize)?;
                            self.typed_write(pointer, ty, self.typed_read(pointer_to_var, ty)?)?;
                        }
                        self.lambdas.insert(
                            code_ptr,
                            LambdaInfo {
                                info: FunctionInfo {
                                    generics: self.call_stack.last().unwrap().f.generics,
                                    params: &lambda.params,
                                    body: Some(&lambda.body),
                                },
                                captures: Some(CaptureInfo {
                                    size,
                                    info: captures
                                        .into_iter()
                                        .zip(offsets)
                                        .map(|((var, ty, _), offset)| (var, ty, offset))
                                        .collect(),
                                }),
                                generic_args,
                            },
                        );
                        Ok(Value::pair(Value::Pointer(env), Value::Pointer(code_ptr)))
                    }
                }
            }
            typed_ast::ExprKind::List(elements) => {
                let Type::List(ty) = &expr.ty else {
                    unreachable!("Should be a list")
                };
                let ty = self.simplify_ty((**ty).clone());
                let element_values = elements
                    .iter()
                    .map(|value| self.interpret_expr(value))
                    .collect::<Result<Vec<_>, _>>()?;
                let size = size_of(&ty);
                let pointer = self
                    .memory
                    .allocate(MemLocation::Heap, size * element_values.len());
                for (i, value) in element_values.into_iter().enumerate() {
                    self.typed_write(
                        self.memory
                            .byte_offset_in_bounds(pointer, (size * i).try_into().unwrap())?,
                        &ty,
                        value,
                    )?;
                }
                Ok(Value::Tuple(vec![
                    Value::Pointer(pointer),
                    Value::Int(Int::from_size(elements.len())),
                    Value::Int(Int::from_size(elements.len())),
                ]))
            }
        }
    }
    fn call_value(
        &mut self,
        resource: IsResource,
        callee: Value,
        args: Vec<Value>,
    ) -> Result<Value, InterpretError> {
        let IsResource::Data = resource else {
            let (env, code) = callee.into_pair().unwrap();
            let env = env.as_pointer().unwrap();
            let code = code.as_pointer().unwrap();
            let &LambdaInfo {
                info: function,
                ref captures,
                ref generic_args,
            } = &self.lambdas[&code];
            let generic_args = generic_args.clone();
            let CaptureInfo {
                size: _,
                info: captures,
            } = captures.as_ref().unwrap();
            let captures = captures.clone();
            let mut args = args;
            args.insert(0, Value::Pointer(env));
            let capture_map = captures
                .iter()
                .map(|(var, ty, offset)| (*var, (ty.clone(), *offset)))
                .collect();
            let value = self.interpret_function(function, generic_args, args, Some(capture_map))?;
            self.drop_closure_env(env, captures)?;
            return Ok(value);
        };
        let code = callee.as_pointer().unwrap();
        if let Some((b, tys)) = self.builtin_functions.iter().find_map(|(b, args_with_p)| {
            if let Some((args, _)) = args_with_p.iter().find(|&(_, &p)| p == code) {
                Some((*b, args.clone()))
            } else {
                None
            }
        }) {
            return self.handle_builtin_call(b, tys, args);
        }
        if let Some((f, inst_args)) = self.function_from_pointer(code) {
            self.interpret_function(self.functions[&f].0, inst_args, args, None)
        } else {
            let LambdaInfo {
                info: function,
                captures,
                generic_args,
            } = &self.lambdas[&code];
            let generic_args = generic_args.clone();
            debug_assert!(captures.is_none());
            self.interpret_function(*function, generic_args, args, None)
        }
    }
    fn handle_builtin_call(
        &mut self,
        b: Builtin,
        tys: Vec<Type>,
        args: Vec<Value>,
    ) -> Result<Value, InterpretError> {
        let tys = tys
            .into_iter()
            .map(|ty| self.simplify_ty(ty))
            .collect::<Vec<_>>();
        fn args_as_array<const N: usize>(args: Vec<Value>) -> Option<[Value; N]> {
            if args.len() != N {
                return None;
            }
            let mut values = [const { Value::Int(Int::ZERO) }; N];

            for (i, arg) in args.into_iter().enumerate() {
                values[i] = arg;
            }
            Some(values)
        }
        match b {
            Builtin::AllocBox => {
                let ty = &tys[0];
                let pointer = self.memory.allocate(MemLocation::Heap, size_of(ty));
                let [arg] = args_as_array(args).unwrap();
                self.typed_write(pointer, ty, arg)?;
                Ok(Value::Pointer(pointer))
            }
            Builtin::DeallocBox => {
                let [arg] = args_as_array(args).unwrap();
                let pointer = arg.as_pointer().unwrap();
                let ty = &tys[0];
                let value = self.typed_read(pointer, ty)?;
                self.memory.deallocate(MemLocation::Heap, pointer)?;
                Ok(value)
            }
            Builtin::DerefBoxMut => {
                let ty = &tys[0];

                let [arg] = args_as_array(args).unwrap();
                let pointer_to_box = arg.as_pointer().unwrap();
                let pointer = self
                    .typed_read(pointer_to_box, &Type::Box(Box::new(ty.clone())))?
                    .as_pointer()
                    .unwrap();
                Ok(Value::Pointer(pointer))
            }
            Builtin::DerefBox => {
                let ty = &tys[0];
                let [arg] = args_as_array(args).unwrap();
                let pointer_to_box = arg.as_pointer().unwrap();
                let pointer = self
                    .typed_read(pointer_to_box, &Type::Box(Box::new(ty.clone())))?
                    .as_pointer()
                    .unwrap();
                Ok(Value::Pointer(pointer))
            }
            Builtin::Freeze => {
                let [arg] = args_as_array(args).unwrap();
                let pointer = arg.as_pointer().unwrap();
                Ok(Value::Pointer(pointer))
            }
            Builtin::DestroyString => todo!("destroy string"),
            Builtin::Replace => {
                let ty = &tys[0];
                let [mut_ref, f] = args_as_array(args).unwrap();
                let mut_ref = mut_ref.as_pointer().unwrap();
                let old_value = self.typed_read(mut_ref, ty)?;
                let result = self.call_value(IsResource::Resource, f, vec![old_value])?;
                self.typed_write(mut_ref, ty, result)?;
                Ok(Value::Pointer(mut_ref))
            }
            Builtin::Swap => {
                let [dest, src] = args_as_array(args).unwrap();
                let dest = dest.as_pointer().unwrap();
                let ty = &tys[0];
                let result = self.typed_read(dest, ty)?;
                self.typed_write(dest, ty, src)?;
                Ok(result)
            }
        }
    }
    fn interpret_function(
        &mut self,
        function: FunctionInfo<'f>,
        generic_args: Vec<Type>,
        mut args: Vec<Value>,
        captures: Option<HashMap<VarId, (Type, usize)>>,
    ) -> Result<Value, InterpretError> {
        let Some(body) = function.body else {
            unreachable!("Cannot call functions without body")
        };
        self.call_stack.push(Frame {
            f: function,
            vars: HashMap::new(),
            locals: Vec::new(),
            scope: Vec::new(),
            generic_args,
            captured_vars: captures.map(|captures| {
                let env = args.remove(0).as_pointer().unwrap();
                Env {
                    pointer: env,
                    fields: captures
                        .into_iter()
                        .map(|(var, (ty, field))| (var, (ty, false, field)))
                        .collect(),
                }
            }),
        });
        let result = self.in_drop_scope(|this| {
            let param_tys = function
                .params
                .iter()
                .map(|param| (param.var, param.ty.clone()))
                .collect::<Vec<_>>();
            for ((var, param), arg) in param_tys.into_iter().zip(args) {
                let pointer = this.alloc_var(var, &param);
                this.typed_write(pointer, &param, arg)?;
            }
            this.interpret_expr(body)
        });
        self.call_stack.pop();
        result
    }
    pub fn interpret(mut self) -> Result<(), InterpretError> {
        let value =
            self.interpret_function(self.functions[&self.entry].0, Vec::new(), Vec::new(), None)?;
        assert!(value.is_unit());
        for alloc in self.memory.leaked_allocations() {
            println!("Leaked {:?} at {}", alloc.bytes, alloc.base_address);
        }
        Ok(())
    }
}
