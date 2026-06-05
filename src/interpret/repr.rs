use crate::{
    ast::IsResource,
    interpret::{
        ADDR_SIZE, InterpretError,
        memory::Byte,
        values::{Int, Pointer, StringValue, Value},
    },
    types::{FunctionType, Type},
};
pub fn offsets_of(fields: &[Type]) -> (usize, Vec<usize>) {
    let mut offset = 0usize;
    let mut offsets = Vec::new();
    let mut max_align = 1;
    for field in fields {
        let align = align_of(field);
        max_align = max_align.max(align);
        offset = offset.next_multiple_of(align);
        offsets.push(offset);
        offset += size_of(field);
    }
    (offset.next_multiple_of(max_align), offsets)
}
pub fn align_of(ty: &Type) -> usize {
    match ty {
        Type::Bool => 1,
        Type::Char => 4,
        Type::Imm(..) | Type::Box(..) | Type::Mut(..) => ADDR_SIZE,
        Type::Record(fields) => fields
            .iter()
            .map(|field| align_of(&field.ty))
            .max()
            .unwrap_or(1),
        Type::Unit => 1,
        Type::String => ADDR_SIZE,
        Type::Int => 8,
        Type::Unknown | Type::Infer(_) => unreachable!(),
        Type::Option(ty) => align_of(ty).max(1),
        Type::Function(_) => ADDR_SIZE,
        Type::List(_) => ADDR_SIZE,
        Type::Param(..) => todo!("Params"),
    }
}
pub fn size_of(ty: &Type) -> usize {
    match ty {
        Type::Bool => 1,
        Type::String | Type::List(_) => ADDR_SIZE * 3,
        Type::Unit => 0,
        Type::Int => 8,
        Type::Char => 4,
        Type::Box(_) | Type::Imm(..) | Type::Mut(..) => ADDR_SIZE,
        Type::Param(..) => todo!("params"),
        Type::Record(fields) => {
            let mut max_align = 1;
            fields
                .iter()
                .fold(0usize, |offset: usize, field| {
                    max_align = max_align.max(align_of(&field.ty));
                    offset.next_multiple_of(max_align) + size_of(&field.ty)
                })
                .next_multiple_of(max_align)
        }
        Type::Option(ty) => {
            let align = align_of(ty);
            (1usize.next_multiple_of(align) + size_of(ty)).next_multiple_of(align)
        }
        Type::Infer(_) | Type::Unknown => unreachable!("Cannot have sizes"),
        Type::Function(FunctionType { resource, .. }) => {
            if *resource == IsResource::Data {
                ADDR_SIZE
            } else {
                ADDR_SIZE * 2
            }
        }
    }
}

pub fn encode_ptr(pointer: Pointer) -> Vec<Byte> {
    pointer
        .address
        .to_be_bytes()
        .iter()
        .copied()
        .map(|b| Byte::Init(b, pointer.alloc))
        .collect()
}

pub fn decode_ptr(bytes: &[Byte]) -> Result<Pointer, InterpretError> {
    let bytes: [Byte; ADDR_SIZE] = bytes
        .as_array()
        .copied()
        .ok_or(InterpretError::InvalidValue)?;
    let mut prov = bytes.first().unwrap().prov();
    let mut ptr_bytes = [0; ADDR_SIZE];
    for (i, byte) in bytes.into_iter().enumerate() {
        if prov != byte.prov() || prov.is_none() {
            prov = None;
        }
        match byte {
            Byte::Uninit => return Err(InterpretError::UninitByteInPointer),
            Byte::Init(b, _) => {
                ptr_bytes[i] = b;
            }
        }
    }
    let addr = usize::from_be_bytes(ptr_bytes);
    Ok(Pointer {
        address: addr,
        alloc: prov,
    })
}
pub fn encode_record(fields: &[Type], values: Vec<Value>) -> Vec<Byte> {
    let (size, offsets) = offsets_of(fields);
    let mut bytes = vec![Byte::Uninit; size];
    for ((offset, field), value) in offsets.into_iter().zip(fields).zip(values) {
        let size = size_of(field);
        bytes[offset..][..size].clone_from_slice(&encode(field, value));
    }
    bytes
}
pub fn decode_record(fields: &[Type], bytes: &[Byte]) -> Result<Vec<Value>, InterpretError> {
    let (_, offsets) = offsets_of(fields);
    let mut values = vec![];
    for (offset, field) in offsets.into_iter().zip(fields) {
        let size = size_of(field);
        values.push(decode(field, &bytes[offset..][..size])?);
    }
    Ok(values)
}
pub fn encode_int(value: Int) -> Vec<Byte> {
    let bytes = value.to_be_bytes();
    bytes.into_iter().map(|b| Byte::Init(b, None)).collect()
}
pub fn decode_int(bytes: &[Byte]) -> Result<Int, InterpretError> {
    let bytes = bytes
        .iter()
        .map(|b| {
            if let Byte::Init(b, _) = *b {
                Ok(b)
            } else {
                Err(InterpretError::UninitByteInInt)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Int::from_be_bytes(*bytes.as_array().unwrap()))
}
pub fn encode(ty: &Type, value: Value) -> Vec<Byte> {
    match ty {
        Type::Bool => vec![Byte::Init(
            if value.as_bool().unwrap() { 1 } else { 0 },
            None,
        )],
        Type::Char => {
            let value = value.as_char().unwrap() as u32;
            value.to_be_bytes().map(|b| Byte::Init(b, None)).to_vec()
        }
        Type::Unit => Vec::new(),
        Type::String => {
            let string = value.as_string().unwrap();
            let mut bytes = Vec::new();
            bytes.extend(encode_ptr(string.pointer));
            bytes.extend(encode_int(string.cap));
            bytes.extend(encode_int(string.len));
            bytes
        }
        Type::Int => {
            let value = value.as_int().unwrap();
            encode_int(value)
        }
        Type::Box(_) | Type::Imm(..) | Type::Mut(..) => encode_ptr(value.as_pointer().unwrap()),
        Type::Record(fields) => {
            let fields = fields
                .iter()
                .map(|field| field.ty.clone())
                .collect::<Vec<_>>();
            let values = value.as_tuple().unwrap().to_vec();
            encode_record(&fields, values)
        }
        Type::Infer(_) | Type::Unknown => unreachable!("Cant encode"),
        Type::Param(..) => todo!("Handle generic param encode"),
        Type::List(..) => todo!("List encode"),
        Type::Function(_) => {
            let (env, code) = value.into_pair().unwrap();
            let env = env.as_pointer().unwrap();
            let code = code.as_pointer().unwrap();
            let mut bytes = encode_ptr(env);
            bytes.extend(encode_ptr(code));
            bytes
        }
        Type::Option(inner) => {
            let value = value.into_option().unwrap();
            let mut bytes = vec![Byte::Uninit; size_of(ty)];
            let offset = align_of(inner);

            bytes[..1].copy_from_slice(&encode(&Type::Bool, Value::Bool(value.is_some()))[..1]);
            if let Some(value) = value {
                bytes[offset..].copy_from_slice(&encode(inner, value));
            }
            bytes
        }
    }
}
pub fn decode(ty: &Type, bytes: &[Byte]) -> Result<Value, InterpretError> {
    match ty {
        Type::Bool => {
            if !bytes.is_empty() {
                match bytes[0] {
                    Byte::Init(0, _) => Ok(Value::Bool(false)),
                    Byte::Init(1, _) => Ok(Value::Bool(true)),
                    _ => Err(InterpretError::InvalidValue),
                }
            } else {
                Err(InterpretError::NotEnoughBytes)
            }
        }
        Type::Unit => Ok(Value::unit()),
        Type::Infer(..) | Type::Unknown => unreachable!(),
        Type::String => {
            let values = decode_record(
                &[Type::Box(Box::new(Type::Unknown)), Type::Int, Type::Int],
                bytes,
            )?;
            let [ptr, cap, len] = values.as_array().ok_or(InterpretError::InvalidValue)?;
            let ptr = ptr.as_pointer().ok_or(InterpretError::InvalidPointer)?;
            let cap = cap.as_int().ok_or(InterpretError::InvalidPointer)?;
            let len = len.as_int().ok_or(InterpretError::InvalidPointer)?;
            Ok(Value::from_string(StringValue {
                pointer: ptr,
                cap,
                len,
            }))
        }
        Type::Box(_) | Type::Imm(..) | Type::Mut(..) => {
            let ptr = decode_ptr(bytes)?;
            Ok(Value::Pointer(ptr))
        }
        Type::Int => {
            let value = decode_int(bytes)?;
            Ok(Value::Int(value))
        }
        Type::Record(fields) => {
            let fields = fields
                .iter()
                .map(|field| field.ty.clone())
                .collect::<Vec<_>>();
            let values = decode_record(&fields, bytes)?;
            Ok(Value::Tuple(values))
        }
        Type::Function(_) => {
            let record = decode_record(
                &[
                    Type::Box(Box::new(Type::Unknown)),
                    Type::Box(Box::new(Type::Unknown)),
                ],
                bytes,
            )?;
            let (env, code) = Value::Tuple(record)
                .into_pair()
                .ok_or(InterpretError::InvalidValue)?;
            let env = env.as_pointer().ok_or(InterpretError::InvalidPointer)?;
            let code = code.as_pointer().ok_or(InterpretError::InvalidPointer)?;
            Ok(Value::pair(Value::Pointer(env), Value::Pointer(code)))
        }
        Type::Char => {
            let bytes = &bytes[0..4];
            let bytes = bytes
                .iter()
                .map(|b| match b {
                    Byte::Init(b, _) => Some(*b),
                    Byte::Uninit => None,
                })
                .collect::<Option<Vec<u8>>>()
                .ok_or(InterpretError::UninitByteInChar)?;
            let bytes = bytes.as_array().unwrap();
            Ok(Value::Char(
                char::from_u32(u32::from_be_bytes(*bytes)).ok_or(InterpretError::NotUtf8)?,
            ))
        }
        Type::Param(..) => todo!("Decode param"),
        Type::List(_) => todo!("Decode list"),
        Type::Option(inner) => {
            let bytes = &bytes[0..size_of(ty)];
            const NONE_DISCRIMINANT_AS_U8: u8 = Value::NONE_DISCRIMINANT as u8;
            const SOME_DISCRIMINANT_AS_U8: u8 = Value::SOME_DISCRIMINANT as u8;
            let is_some = match bytes[0] {
                Byte::Init(NONE_DISCRIMINANT_AS_U8, _) => false,
                Byte::Init(SOME_DISCRIMINANT_AS_U8, _) => true,
                b @ (Byte::Init(_, _) | Byte::Uninit) => {
                    return Err(InterpretError::InvalidDiscriminant(b));
                }
            };
            if is_some {
                let value_inner = decode(inner, &bytes[align_of(inner)..])?;
                Ok(Value::Variant(Value::SOME_DISCRIMINANT, vec![value_inner]))
            } else {
                Ok(Value::Variant(Value::NONE_DISCRIMINANT, Vec::new()))
            }
        }
    }
}

pub fn is_resource(ty: &Type) -> bool {
    match ty {
        Type::Bool
        | Type::Unit
        | Type::Unknown
        | Type::Int
        | Type::Imm(..)
        | Type::Char
        | Type::Function(FunctionType {
            resource: IsResource::Data,
            ..
        }) => false,
        Type::Option(ty) => is_resource(ty),
        Type::Mut(..)
        | Type::Function(FunctionType {
            resource: IsResource::Resource,
            ..
        })
        | Type::String
        | Type::Box(_)
        | Type::Param(..)
        | Type::List(_) => true,
        Type::Record(fields) => fields.iter().any(|field| is_resource(&field.ty)),
        Type::Infer(_) => unreachable!("All infers should be removed"),
    }
}
