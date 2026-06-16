use crate::{
    mir::{BodySource, Context, LocalKind, Place, Terminator, build::Builder},
    resolved_ast::{FunctionId, Var},
    typed_ast,
};

impl Builder<'_> {
    pub fn build_function(context: &mut Context, id: FunctionId, function: &typed_ast::Function) {
        let mut builder = Builder::new(
            context,
            BodySource::Function(id),
            function.return_type.clone(),
        );
        for param in &function.params {
            builder.new_local(
                param.ty.clone(),
                LocalKind::Param(Var(param.name.content.clone(), param.var)),
            );
        }
        if let Some(body) = &function.body {
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
}
