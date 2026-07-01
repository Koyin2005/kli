use crate::{
    ast::IsResource,
    collect::CtxtRef,
    mir::{
        BodySource, Captures, Constant, Context, Local, LocalKind, Operand, Place, PointerCast,
        Rvalue, TerminatorKind, build::Builder, visitor::Visit, well_formed::WellFormed,
    },
    resolved_ast::DefId,
    src_loc::SrcLoc,
    typed_ast::{self, Lambda},
    types::{FunctionType, Type},
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
        id: DefId,
        function: &typed_ast::Function,
    ) {
        let mut builder = Builder::new(
            mir_context,
            BodySource::Function(id),
            function.return_type.clone(),
            None,
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
    pub(super) fn lambda_code(&mut self, lambda: &Lambda) -> Constant {
        let _ty = Type::Function(FunctionType {
            resource: IsResource::Data,
            params: lambda.params.iter().map(|param| param.ty.clone()).collect(),
            return_type: Box::new(lambda.return_type.clone()),
        });
        if self
            .mir_context
            .bodies
            .contains_key(&BodySource::Lambda(lambda.id))
        {
            todo!("Handle lambdas")
        }
        let is_resource = lambda.is_resource == IsResource::Resource;
        let context = &mut *self.mir_context;
        let mut builder = Builder::new(
            context,
            BodySource::Lambda(lambda.id),
            lambda.return_type.clone(),
            if is_resource {
                Some(Captures {
                    env_ptr: None,
                    captures: lambda.captures.clone(),
                })
            } else {
                None
            },
            self.ctxt,
        );

        builder.add_param_locals(
            std::iter::once(if is_resource {
                Some((LocalKind::Env, Type::pointer(Type::Byte)))
            } else {
                None
            })
            .flatten()
            .chain(
                lambda
                    .params
                    .iter()
                    .map(|param| (LocalKind::Param(param.var()), param.ty.clone())),
            ),
        );
        if !lambda.captures.is_empty() {
            let env_ty = builder.body.capture_info.as_ref().unwrap().env_type();
            let casted = builder.assign_to_temp(
                lambda.body.loc,
                Type::pointer(env_ty),
                Rvalue::pointer_cast(
                    PointerCast::RawToRaw(Type::Byte),
                    Operand::Load(Place::local(Local::FIRST_PARAM)),
                ),
            );
            builder.body.capture_info.as_mut().unwrap().env_ptr = Some(casted);
        }
        builder.expr_into_dest(Place::return_place(), &lambda.body);
        builder.finish_block(lambda.body.loc, TerminatorKind::Return);
        builder.add_finished_body();
        todo!("Handle lambda ids")
    }
}
