use crate::{
    ast::IsResource,
    mir::{
        BodySource, Constant, ConstantValue, Context, LocalKind, Place, Terminator, build::Builder,
    },
    resolved_ast::{FunctionId, Var},
    typed_ast::{self, Lambda},
    types::{FunctionType, Type},
};

impl Builder<'_> {
    pub fn build_from_function(
        context: &mut Context,
        id: FunctionId,
        function: &typed_ast::Function,
    ) {
        Self::build(
            context,
            BodySource::Function(id),
            &function.return_type,
            function.params.iter().map(|param| {
                (
                    LocalKind::Param(Var(param.name.content.clone(), param.var)),
                    param.ty.clone(),
                )
            }),
            function.body.as_ref(),
            Vec::new(),
        );
    }
    pub fn build(
        context: &mut Context,
        source: BodySource,
        return_type: &Type,
        params: impl Iterator<Item = (LocalKind, Type)>,
        body: Option<&typed_ast::Expr>,
        captures: Vec<(Var, Type)>,
    ) {
        let mut builder = Builder::new(context, source, return_type.clone(), captures);
        for (kind, ty) in params {
            builder.new_local(ty.clone(), kind);
        }
        if let Some(body) = body {
            builder.expr_into_dest(Place::return_place(), body);
            builder.finish_block(Terminator::Return);
        } else {
            builder.finish_block(Terminator::Unreachable);
        }
        let body = builder.finish();
        context.body_sources.push(body.src);
        assert!(
            context.bodies.insert(body.src, body).is_none(),
            "Can only have one source for each body"
        );
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
        Self::build(
            self.context,
            BodySource::Lambda(lambda.id),
            &lambda.return_type,
            std::iter::once(if is_resource {
                Some((LocalKind::Env, Type::OwningPointer))
            } else {
                None
            })
            .flatten()
            .chain(lambda.params.iter().map(|param| {
                (
                    LocalKind::Param(Var(param.name.content.clone(), param.var)),
                    param.ty.clone(),
                )
            })),
            Some(&lambda.body),
            lambda.captures.clone(),
        );
        Constant {
            ty,
            value: ConstantValue::Lambda(lambda.id, Vec::new()),
        }
    }
}
