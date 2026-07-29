use crate::{
    collect::CtxtRef,
    mir::{
        BodySource, Constant, Context, LocalKind, Place, TerminatorKind, build::Builder,
        visitor::Visit, well_formed::WellFormed,
    },
    src_loc::SrcLoc,
    typed_ast::{self, Lambda},
    types::{FunctionType, GenericArgs, Type},
};

impl Builder<'_> {
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
    fn add_param_locals(&mut self, params: impl Iterator<Item = (LocalKind, Type)>) {
        for (kind, ty) in params {
            self.new_local(ty, kind);
        }
    }
    pub fn build_from_function(
        ctxt: CtxtRef,
        mir_context: &mut Context,
        function: &typed_ast::Function,
        src : BodySource
    ) {
        let mut builder = Builder::new(
            mir_context,
            src,
            function.return_type.clone(),
            ctxt,
        );
        builder.add_param_locals(
            function
                .params
                .iter()
                .map(|param| (LocalKind::Param(param.var()), param.ty.clone())),
        );
        if let Some(body) = function.body.as_ref() {
            builder.expr_into_dest(Place::return_place(), body);
            builder.finish_block(body.loc, TerminatorKind::Return);
        } else {
            builder.finish_block(SrcLoc::dummy(), TerminatorKind::Unreachable);
        }
        builder.add_finished_body();
    }
    pub(super) fn lambda_code_constant(ctxt: CtxtRef<'_>, lambda: &Lambda) -> Constant {
        let ty = Type::Function(FunctionType {
            params: lambda.param_tys.clone(),
            return_type: lambda.return_type.clone(),
        });
        let generics = ctxt.generics(lambda.id);
        let args = if generics.is_empty() {
            generics.instantiate_identity()
        } else {
            GenericArgs::new()
        };
        Constant {
            ty: Box::new(ty),
            value: crate::mir::ConstValue::Named(lambda.id, args),
        }
    }
}
