use crate::{
    ast::IsResource,
    mir::{
        BodySource, Captures, Constant, ConstantValue, Context, Local, LocalKind, Operand, Place,
        PointerCast, Rvalue, Terminator, build::Builder,
    },
    resolved_ast::{FunctionId, Var},
    typed_ast::{self, Lambda},
    types::{FunctionType, Type},
};

impl Builder<'_> {
    fn add_finished_body(self) {
        let body = self.body;
        let context = self.context;
        context.body_sources.push(body.src);
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
        context: &mut Context,
        id: FunctionId,
        function: &typed_ast::Function,
    ) {
        let mut builder = Builder::new(
            context,
            BodySource::Function(id),
            function.return_type.clone(),
            None,
        );
        builder.add_param_locals(
            function
                .params
                .iter()
                .map(|param| (LocalKind::Param(param.var()), param.ty.clone())),
        );
        for param in function.params.iter() {
            builder.new_local(
                param.ty.clone(),
                LocalKind::Param(Var(param.name.content.clone(), param.var)),
            );
        }
        if let Some(body) = function.body.as_ref() {
            builder.expr_into_dest(Place::return_place(), body);
            builder.finish_block(Terminator::Return);
        } else {
            builder.finish_block(Terminator::Unreachable);
        }
        builder.add_finished_body();
    }
    pub(super) fn lambda_code(&mut self, lambda: &Lambda) -> Constant {
        let ty = Type::Function(FunctionType {
            resource: IsResource::Data,
            params: lambda.params.iter().map(|param| param.ty.clone()).collect(),
            return_type: Box::new(lambda.return_type.clone()),
        });
        if self
            .context
            .bodies
            .contains_key(&BodySource::Lambda(lambda.id))
        {
            return Constant {
                ty,
                value: ConstantValue::Lambda(lambda.id, Vec::new()),
            };
        }
        let is_resource = lambda.is_resource == IsResource::Resource;
        let context = &mut *self.context;
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
                Type::pointer(env_ty),
                Rvalue::PointerCast(
                    PointerCast::RawToRaw,
                    Operand::Load(Place::local(Local::zero())),
                ),
            );
            builder.body.capture_info.as_mut().unwrap().env_ptr = Some(casted);
        }
        builder.expr_into_dest(Place::return_place(), &lambda.body);
        builder.finish_block(Terminator::Return);
        builder.add_finished_body();
        Constant {
            ty,
            value: ConstantValue::Lambda(lambda.id, Vec::new()),
        }
    }
}
