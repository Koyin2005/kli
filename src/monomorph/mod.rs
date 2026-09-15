use crate::{
    CtxtRef,
    mir::{self, Body, BodyId, visitor::MutVisit},
    scheme::Scheme,
    types::{GenericArgs, GenericArgsRef, TypeMappable},
};

pub fn monomorphise<'ctxt>(_ctxt: CtxtRef<'ctxt>, _mir: &mut mir::Context<'ctxt>) {}

pub fn instantiate_body<'ctxt>(
    ctxt: CtxtRef<'ctxt>,
    mir: &mir::Context<'ctxt>,
    id: BodyId,
    args: GenericArgs<'ctxt>,
) -> Body<'ctxt> {
    let mut new_instance = mir.get_body(id).clone();
    if args.is_empty() {
        return new_instance;
    }
    for local in new_instance.locals.iter_mut() {
        local.ty = Scheme::new(local.ty).bind(ctxt, &args);
    }
    for reg in new_instance.registers.iter_mut() {
        reg.ty = Scheme::new(reg.ty).bind(ctxt, &args);
    }
    new_instance.return_type = Scheme::new(new_instance.return_type).bind(ctxt, &args);

    struct Instantiator<'a, 'ctxt> {
        ctxt: CtxtRef<'ctxt>,
        args: GenericArgsRef<'a, 'ctxt>,
    }
    impl<'ctxt> Instantiator<'_, 'ctxt> {
        fn instantiate<T: TypeMappable<'ctxt>>(&self, value: T) -> T {
            Scheme::new(value).bind(self.ctxt, self.args)
        }
    }
    impl<'ctxt> MutVisit<'ctxt> for Instantiator<'_, 'ctxt> {
        fn visit_operation(&mut self, loc: mir::Location, operation: &mut mir::Operation<'ctxt>) {
            self.super_visit_operation(loc, operation);
            match operation {
                mir::Operation::Zeroed(ty) => {
                    *ty = self.instantiate(*ty);
                }
                mir::Operation::AllocArray(ty, _) => {
                    *ty = self.instantiate(*ty);
                }
                mir::Operation::Aggregate(aggregate_kind, _) => match aggregate_kind {
                    mir::AggregateKind::NamedRecord(_, args)
                    | mir::AggregateKind::Variant(_, _, args) => {
                        *args = self.instantiate(std::mem::take(args));
                    }
                    mir::AggregateKind::Tuple => (),
                },
                mir::Operation::Not(_)
                | mir::Operation::Cmp(..)
                | mir::Operation::Arith(..)
                | mir::Operation::Bitwise(..)
                | mir::Operation::ExtractPayload(..)
                | mir::Operation::ExtractField(..)
                | mir::Operation::ExtractElement(..)
                | mir::Operation::Call(..)
                | mir::Operation::InBounds(..)
                | mir::Operation::Len(_)
                | mir::Operation::Discriminant(_)
                | mir::Operation::Load(_)
                | mir::Operation::ReadLine
                | mir::Operation::Copy(_) => (),
            }
        }
        fn visit_value(&mut self, _: mir::Location, value: &mut mir::Value<'ctxt>) {
            match value {
                mir::Value::Lambda(ty, _, args) => {
                    *ty = self.instantiate(*ty);
                    *args = self.instantiate(std::mem::take(args));
                }
                mir::Value::Unknown(ty) => *ty = self.instantiate(*ty),
                mir::Value::Function(_, args) => {
                    *args = self.instantiate(std::mem::take(args));
                }
                mir::Value::Reg(_)
                | mir::Value::Int(_)
                | mir::Value::Bool(_)
                | mir::Value::Char(_)
                | mir::Value::String(_)
                | mir::Value::Unit => todo!(),
            }
        }
    }
    Instantiator { ctxt, args: &args }.visit_body_no_invalidate(&mut new_instance);
    new_instance
}
