use crate::{
    ast::IsResource,
    collect::CtxtRef,
    mir::{
        BodySource, CastKind, Constant, ConstantValue, Context, Local, LocalKind, Operand, Place,
        PointerCast, Rvalue, TerminatorKind, build::Builder, visitor::Visit,
        well_formed::WellFormed,
    },
    resolved_ast::DefId,
    src_loc::SrcLoc,
    typed_ast::{self, FieldId, Lambda},
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
    pub(super) fn lambda_code_constant(ctxt: CtxtRef<'_>, lambda: &Lambda) -> Constant {
        let ty = Type::Function(FunctionType {
            resource: IsResource::Data,
            params: lambda
                .captures
                .iter()
                .map(|capture| &capture.ty)
                .chain(lambda.param_tys.iter())
                .cloned()
                .collect(),
            return_type: lambda.return_type.clone(),
        });
        let args = if !ctxt.generics(ctxt.expect_parent(lambda.id)).is_empty() {
            todo!("Handle generic lambdas")
        } else {
            GenericArgs::new()
        };
        Constant {
            ty: Box::new(ty),
            value: crate::mir::ConstantValue::NamedConst(lambda.id, args),
        }
    }

    pub(super) fn closure_shim(
        mir_context: &mut Context,
        ctxt: CtxtRef<'_>,
        id: DefId,
        lambda: &Lambda,
    ) -> Constant {
        let args = if !ctxt.generics(ctxt.expect_parent(lambda.id)).is_empty() {
            todo!("Handle generic lambdas")
        } else {
            GenericArgs::new()
        };
        let constant = Constant {
            ty: Box::new(Type::new_function(
                std::iter::once(Type::pointer(Type::Byte))
                    .chain(lambda.param_tys.iter().cloned())
                    .collect(),
                *lambda.return_type.clone(),
            )),
            value: ConstantValue::ClosureShim(id, args),
        };
        if mir_context
            .bodies
            .contains_key(&BodySource::ClosureShim(id))
        {
            return constant;
        }
        /*
           fun(x) -> x + upvar

           lambda l (upvar : int, x : int) -> int = ..

           closure_shim l (env : ptr[byte], x : int) -> int = let env = cast(ptr[{upvar : int}],env); return (lambda l)(env^.upvar,x);
        */

        let mut builder = Builder::new(
            mir_context,
            BodySource::ClosureShim(id),
            (*lambda.return_type).clone(),
            None,
            ctxt,
        );
        builder.add_param_locals(
            std::iter::once((LocalKind::Param(None), Type::pointer(Type::Byte))).chain(
                lambda
                    .params
                    .iter()
                    .zip(lambda.param_tys.iter())
                    .map(|(param, ty)| (LocalKind::Param(Some(param.var)), ty.clone())),
            ),
        );

        let env_ty = Type::closure_env(lambda.captures.iter().cloned());
        let casted_env = builder.assign_to_temp(
            lambda.loc,
            Type::pointer(env_ty.clone()),
            Rvalue::Cast(
                CastKind::PointerCast(PointerCast::RawToRaw(env_ty)),
                Operand::Load(Place::local(Local::new(0))),
            ),
        );
        builder.assign(
            lambda.loc,
            Place::return_place(),
            Rvalue::Call(
                Operand::Constant(Self::lambda_code_constant(ctxt, lambda)),
                lambda
                    .captures
                    .iter()
                    .enumerate()
                    .map(|(i, _)| {
                        Place::local(casted_env)
                            .with_deref()
                            .with_field(FieldId::new(i))
                    })
                    .chain(
                        lambda
                            .params
                            .iter()
                            .enumerate()
                            .map(|(i, _)| Place::local(Local::new(i + 1))),
                    )
                    .map(Operand::Load)
                    .collect(),
            ),
        );
        builder.finish_block(lambda.loc, TerminatorKind::Return);
        builder.add_finished_body();
        constant
    }
}
