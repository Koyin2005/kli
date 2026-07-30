use crate::{
    collect::CtxtRef,
    mir::{
        BodySource, Constant, Context, LocalKind, Place, TerminatorKind, build::Builder,
        visitor::Visit, well_formed::WellFormed,
    },
    src_loc::SrcLoc,
    typed_ast::{self, Lambda},
    types::{GenericArgs, Type},
};

impl<'ctxt> Builder<'_, 'ctxt> {
    fn add_finished_body(self) {
        let body = self.body;
        let context = self.mir_context;
        context.body_sources.push(body.src);
        if context.check_well_formed {
            let mut wf = WellFormed::new(&body, self.ctxt);
            wf.visit_body(&body);
        }
        assert!(
            context.bodies.insert(body.src, body).is_none(),
            "Can only have one source for each body"
        );
    }
    fn add_param_locals(&mut self, params: impl Iterator<Item = (LocalKind, Type<'ctxt>)>) {
        for (kind, ty) in params {
            self.new_local(ty, kind);
        }
    }
    pub fn build_from_function<'b>(
        ctxt: CtxtRef<'ctxt>,
        mir_context: &'b mut Context<'ctxt>,
        function: &typed_ast::Function<'ctxt>,
        src: BodySource,
    ) {
        let mut builder = Builder::new(mir_context, src, function.return_type, ctxt);
        builder.add_param_locals(
            function
                .params
                .iter()
                .map(|param| (LocalKind::Param(param.var()), param.ty)),
        );
        if let Some(body) = function.body.as_ref() {
            builder.expr_into_dest(Place::return_place(), body);
            builder.finish_block(body.loc, TerminatorKind::Return);
        } else {
            builder.finish_block(SrcLoc::dummy(), TerminatorKind::Unreachable);
        }
        builder.add_finished_body();
    }
    pub(super) fn lambda_code_constant(
        ctxt: CtxtRef<'ctxt>,
        lambda: &Lambda<'ctxt>,
    ) -> Constant<'ctxt> {
        let ty = Type::function_type(ctxt, lambda.param_tys.clone(), lambda.return_type);
        let generics = ctxt.generics(lambda.id);
        let args = if generics.is_empty() {
            generics.instantiate_identity(ctxt)
        } else {
            GenericArgs::new()
        };
        Constant {
            ty,
            value: crate::mir::ConstValue::Named(lambda.id, args),
        }
    }
}
