use std::collections::HashMap;

use crate::{
    ast::BinaryOp,
    interpret::{
        functions::FunctionInfo,
        memory::{Byte, MemLocation, Memory},
        repr::{align_of, decode, encode, is_resource, offsets_of, size_of},
        values::{Int, Pointer, StringValue, Value},
    },
    resolved_ast::{Builtin, FunctionId, VarId},
    typed_ast::{self, FieldId},
    types::{GenericArg, Type},
};

mod functions;
mod memory;
mod repr;
mod values;
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
pub const INT_SIZE: usize = std::mem::size_of::<Int>();
pub const ADDR_SIZE: usize = 8;
struct Frame {
    vars: HashMap<VarId, (Type, bool, Pointer)>,
    locals: Vec<Pointer>,
    scope: Vec<Vec<VarId>>,
}
pub struct Interpret<'f> {
    functions: HashMap<FunctionId, FunctionInfo<'f>>,
    builtin_functions: HashMap<Builtin, HashMap<Vec<Type>, Pointer>>,
    entry: FunctionId,
    call_stack: Vec<Frame>,
    memory: Memory,
}
impl<'f> Interpret<'f> {
    pub fn new(functions: &'f [typed_ast::Function]) -> Self {
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
                    FunctionInfo {
                        code: function,
                        pointer: mem.allocate(MemLocation::Function, 0),
                    },
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
        builtin_functions.insert(Builtin::DestroyList, HashMap::new());
        builtin_functions.insert(Builtin::DestroyString, HashMap::new());
        builtin_functions.insert(Builtin::Freeze, HashMap::new());
        Self {
            functions,
            entry,
            call_stack: vec![],
            builtin_functions,
            memory: mem,
        }
    }

    fn drop(&mut self, ty: &Type, pointer_to_place: Pointer) -> Result<(), InterpretError> {
        match ty {
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
                } = self.typed_read(pointer_to_place, ty)?.as_string().unwrap();
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
                    let pointer_to_inner = self
                        .memory
                        .byte_offset_in_bounds(pointer_to_place, align_of(inner_ty) as isize)?;
                    self.drop(inner_ty, pointer_to_inner)
                } else {
                    Ok(())
                }
            }
            Type::Function(..) => todo!("Drop functions"),
            Type::Record(fields) => {
                let tys = fields
                    .iter()
                    .map(|field| field.ty.clone())
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
            Type::Param(..) => todo!("Handle params"),
            Type::List(..) => todo!("Drop lists"),
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
    fn dangling_pointer(ty: &Type) -> Pointer {
        Pointer {
            address: align_of(ty),
            alloc: None,
        }
    }
    fn allocate_local(&mut self, ty: &Type) -> Pointer {
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
        self.memory.write(pointer, encode(ty, value))
    }
    fn typed_read(&self, pointer: Pointer, ty: &Type) -> Result<Value, InterpretError> {
        let bytes = self.memory.read(pointer, size_of(ty))?;
        decode(ty, &bytes)
    }
    fn pointer_to_place(
        &mut self,
        as_move: bool,
        place: &typed_ast::Place,
    ) -> Result<Pointer, InterpretError> {
        match &place.kind {
            typed_ast::PlaceKind::Var(var) => {
                let (_, moved, pointer) = self
                    .call_stack
                    .last_mut()
                    .unwrap()
                    .vars
                    .get_mut(&var.1)
                    .unwrap();
                if *moved && as_move {
                    return Err(InterpretError::UseAfterMove);
                }
                if as_move && is_resource(&place.ty) {
                    *moved = true;
                }
                Ok(*pointer)
            }
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
                    .as_int()
                    .unwrap();
                Ok(value == matched_value)
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
                    let pointer = self
                        .memory
                        .byte_offset_in_bounds(pointer_to_place, align_of(&inner.ty) as isize)?;
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
                    .map(|field| field.pattern.ty.clone())
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
                    let pointer = self
                        .memory
                        .byte_offset_in_bounds(pointer_to_place, align_of(&inner.ty) as isize)?;
                    self.assign_to_pattern(inner, pointer)
                } else {
                    Ok(())
                }
            }
            typed_ast::PatternKind::None => Ok(()),
            typed_ast::PatternKind::Record(fields) => {
                let field_tys = fields
                    .iter()
                    .map(|field| field.pattern.ty.clone())
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
    fn interpet_stmt(&mut self, stmt: &typed_ast::Stmt) -> Result<(), InterpretError> {
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
    fn print_value(&self, value: &Value, ty: &Type) -> Result<(), InterpretError> {
        match ty {
            Type::Bool => {
                let value = value.as_bool().expect("Should be a bool");
                print!("{}", value)
            }
            Type::Int => {
                let value = value.as_int().expect("Should be an int");
                print!("{}", value)
            }
            Type::Unit => {
                print!("()")
            }
            Type::Option(ty) => {
                let value = value.as_option_ref().expect("Should be an option");
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
                let field_values = value.as_tuple().expect("Should be a record");
                print!("{{");
                let mut first = true;
                for (field, field_value) in fields.iter().zip(field_values) {
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
                self.print_value(&value, pointee)?;
            }
            Type::Function(_) => todo!("Functions"),
            Type::Char => {
                let char = value.as_char().unwrap();
                print!("{}", char)
            }
            Type::String => {
                let string = self.string_from(value.as_string().expect("Should be a string"))?;
                print!("{}", string);
            }
            Type::List(_) => todo!("boxing"),

            Type::Param(..) => todo!("Type Param"),
            Type::Unknown | Type::Infer(_) => unreachable!("Cannot have this type"),
        }
        Ok(())
    }
    fn function_from_pointer(&self, p: Pointer) -> Result<FunctionId, InterpretError> {
        for (id, f) in self.functions.iter() {
            if f.pointer == p {
                return Ok(*id);
            }
        }
        Err(InterpretError::CalledNonFunction)
    }
    fn string_from(&self, string_value: StringValue) -> Result<String, InterpretError> {
        let StringValue {
            pointer: ptr,
            cap,
            len,
        } = string_value;
        let all_bytes = self.memory.read(ptr, len as usize)?;
        let mut bytes = Vec::with_capacity(cap as usize);
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
        body: &typed_ast::Expr,
        iterator_value: Value,
    ) -> Result<(), InterpretError> {
        match &iter_ty {
            Type::Imm(_, ty) | Type::Mut(_, ty) => {
                let pointer = iterator_value.as_pointer().unwrap();
                let iterator_value = self.typed_read(pointer, ty)?;
                match &**ty {
                    Type::String => {
                        let string = self.string_from(iterator_value.as_string().unwrap())?;
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
                    Type::List(_) => {
                        todo!("Handle list")
                    }
                    ty => self.handle_iteration(pattern, ty, body, iterator_value),
                }
            }
            _ => unreachable!("Cant iterate these"),
        }
    }
    fn interpret_expr(&mut self, expr: &typed_ast::Expr) -> Result<Value, InterpretError> {
        match &expr.kind {
            typed_ast::ExprKind::Err => panic!("Cannot interpret err value"),
            typed_ast::ExprKind::Panic => Err(InterpretError::Panic),
            typed_ast::ExprKind::Int(value) => Ok(Value::Int(*value)),
            typed_ast::ExprKind::Binary(op, left, right) => {
                let left = self.interpret_expr(left)?.as_int().unwrap();
                let right = self.interpret_expr(right)?.as_int().unwrap();
                let (res, overflow) = match op {
                    BinaryOp::Add => left.overflowing_add(right),
                    BinaryOp::Divide => {
                        if right == 0 {
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
                    self.print_value(&value, &arg.ty)?;
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
                    Value::Int(Int::try_from(value.len()).unwrap()),
                    Value::Int(Int::try_from(value.len()).unwrap()),
                ]))
            }
            typed_ast::ExprKind::Function(_, id, args) => {
                let args = Self::generic_args_to_instance_args(args.to_vec());
                assert!(args.is_empty(), "Generic functions not supported atm");
                Ok(Value::pair(
                    Value::Pointer(Self::dangling_pointer(&Type::Unit)),
                    Value::Pointer(self.functions[id].pointer),
                ))
            }
            typed_ast::ExprKind::Call(callee, args) => {
                let callee = self.interpret_expr(callee)?;
                let args = args
                    .iter()
                    .map(|arg| self.interpret_expr(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let (_, code) = callee.into_pair().unwrap();
                let code = code.as_pointer().unwrap();
                if let Some((b, tys)) =
                    self.builtin_functions.iter().find_map(|(b, args_with_p)| {
                        if let Some((args, _)) = args_with_p.iter().find(|&(_, &p)| p == code) {
                            Some((*b, args.clone()))
                        } else {
                            None
                        }
                    })
                {
                    return self.handle_builtin_call(b, tys, args);
                }
                let f = self.function_from_pointer(code)?;
                self.interpret_function(f, args)
            }
            typed_ast::ExprKind::Builtin(builtin, args) => self
                .builtin_functions
                .get_mut(builtin)
                .map(|instances| {
                    let args = Self::generic_args_to_instance_args(args.to_vec());
                    let function_ptr = *instances
                        .entry(args)
                        .or_insert_with(|| self.memory.allocate(MemLocation::Function, 0));
                    Ok(Value::pair(
                        Value::Pointer(Self::dangling_pointer(&Type::Unit)),
                        Value::Pointer(function_ptr),
                    ))
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
            typed_ast::ExprKind::Lambda(..) => todo!("Lambda"),
            typed_ast::ExprKind::List(..) => todo!("List"),
        }
    }
    fn handle_builtin_call(
        &mut self,
        b: Builtin,
        tys: Vec<Type>,
        args: Vec<Value>,
    ) -> Result<Value, InterpretError> {
        fn args_as_array<const N: usize>(args: Vec<Value>) -> Option<[Value; N]> {
            if args.len() != N {
                return None;
            }
            let mut values = [const { Value::Int(0) }; N];

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
            Builtin::Replace => todo!("Replace"),
            Builtin::DestroyList => todo!("Destroy list"),
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
        f: FunctionId,
        args: Vec<Value>,
    ) -> Result<Value, InterpretError> {
        self.call_stack.push(Frame {
            vars: HashMap::new(),
            locals: Vec::new(),
            scope: Vec::new(),
        });
        let result = self.in_drop_scope(|this| {
            let function = this.functions[&f];
            let param_tys = function
                .code
                .params
                .iter()
                .map(|param| (param.var, param.ty.clone()))
                .collect::<Vec<_>>();
            for ((var, param), arg) in param_tys.into_iter().zip(args) {
                let pointer = this.alloc_var(var, &param);
                this.typed_write(pointer, &param, arg)?;
            }
            this.interpret_expr(&function.code.body)
        });
        self.call_stack.pop();
        result
    }
    pub fn interpret(mut self) -> Result<(), InterpretError> {
        let value = self.interpret_function(self.entry, Vec::new())?;
        assert!(value.is_unit());
        for alloc in self.memory.leaked_allocations() {
            println!("Leaked {:?} at {}", alloc.bytes, alloc.base_address);
        }
        Ok(())
    }
}
