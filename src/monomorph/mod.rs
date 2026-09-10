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
        fn visit_constant(&mut self, _: mir::Location, constant: &mut mir::Constant<'ctxt>) {
            constant.ty = self.instantiate(constant.ty);
            match &mut constant.value {
                mir::ConstValue::Named(_, generic_args) => {
                    for arg in generic_args.iter_mut() {
                        *arg = self.instantiate(*arg);
                    }
                }
                mir::ConstValue::Scalar(_)
                | mir::ConstValue::String(_)
                | mir::ConstValue::ZeroSized => (),
            }
        }
        fn visit_rvalue(&mut self, loc: mir::Location, rvalue: &mut mir::Rvalue<'ctxt>) {
            self.super_visit_rvalue(loc, rvalue);
            match rvalue {
                mir::Rvalue::Aggregate(kind, _) => match kind {
                    mir::AggregateKind::Variant(_, _, args)
                    | mir::AggregateKind::NamedRecord(_, args) => {
                        for arg in args.iter_mut() {
                            *arg = self.instantiate(*arg);
                        }
                    }
                    mir::AggregateKind::Tuple => (),
                },
                mir::Rvalue::AllocArray(ty, _) | mir::Rvalue::Cast(_, _, ty) => {
                    *ty = self.instantiate(*ty)
                }
                mir::Rvalue::ReadLine
                | mir::Rvalue::Use(..)
                | mir::Rvalue::Call(..)
                | mir::Rvalue::Binary(..)
                | mir::Rvalue::Len(_)
                | mir::Rvalue::Discriminant(_) => (),
            }
        }
    }
    Instantiator { ctxt, args: &args }.visit_body_no_invalidate(&mut new_instance);
    new_instance
}
