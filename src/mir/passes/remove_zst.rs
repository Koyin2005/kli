use crate::{
    CtxtRef,
    collect::TypeDefKind,
    mir::{Constant, Locals, Location, Operand, StmtKind, passes::MirPass, visitor::MutVisit},
    types::Type,
};

pub struct RemoveZst;
impl RemoveZst {
    fn is_zst(ty: &Type, ctxt: CtxtRef<'_>) -> bool {
        match ty {
            Type::Unit | Type::Never => true,
            Type::Array(ty, _) => Self::is_zst(ty, ctxt),
            Type::Bool
            | Type::Char
            | Type::Int
            | Type::String
            | Type::Byte
            | Type::Param(..)
            | Type::Function(..)
            | Type::RawPointer(..)
            | Type::Imm(..)
            | Type::Mut(..) => false,
            Type::Infer(_) => false,
            Type::Unknown => false,
            Type::Record(fields) => fields.iter().all(|field| Self::is_zst(&field.ty, ctxt)),
            Type::Named(def_id, _, generic_args) => {
                if ctxt.is_type_recursive(*def_id) {
                    return false;
                }
                match ctxt.type_def(*def_id).kind {
                    TypeDefKind::Record(ref fields) => fields
                        .iter()
                        .all(|field| Self::is_zst(&field.type_of(generic_args, ctxt), ctxt)),
                    TypeDefKind::Variant(ref cases) => cases.iter().all(|case| {
                        let field_ty = if let Some(field) = case.field {
                            field.type_of(generic_args, ctxt)
                        } else {
                            Type::Unit
                        };
                        field_ty.is_uninhabited(ctxt) && Self::is_zst(&field_ty, ctxt)
                    }),
                }
            }
        }
    }
}
impl MirPass for RemoveZst {
    fn name(&self) -> &'static str {
        "remove-zst"
    }
    fn run(&self, ctxt: crate::CtxtRef<'_>, body: &mut crate::mir::Body) {
        struct RemoveZstVisit<'a>(CtxtRef<'a>, &'a Locals, &'a Type);
        impl MutVisit for RemoveZstVisit<'_> {
            fn visit_operand(&mut self, _: Location, operand: &mut crate::mir::Operand) {
                let Operand::Load(place) = operand else {
                    return;
                };
                let ty = place.type_of(self.0, self.1, self.2);
                if RemoveZst::is_zst(&ty, self.0) {
                    *operand = Operand::Constant(Constant::zero_sized(ty));
                }
            }
            fn visit_stmt(&mut self, loc: Location, stmt: &mut crate::mir::Stmt) {
                let place = match &mut stmt.kind {
                    StmtKind::Assign(place, rvalue) => {
                        rvalue.can_remove_if_unused().then_some(place)
                    }
                    _ => None,
                };
                if let Some(place) = place
                    && RemoveZst::is_zst(&place.type_of(self.0, self.1, self.2), self.0)
                {
                    stmt.kind = StmtKind::Noop;
                } else {
                    self.super_visit_stmt(loc, stmt);
                }
            }
        }
        let mut visit = RemoveZstVisit(ctxt, &body.locals, &body.return_type);
        for (id, block) in body.blocks.iter_mut_enumerated() {
            visit.visit_block(id, block);
        }
    }
}
