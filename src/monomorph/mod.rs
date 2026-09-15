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
    }
    Instantiator { ctxt, args: &args }.visit_body_no_invalidate(&mut new_instance);
    new_instance
}
