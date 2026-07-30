use crate::{resolved_ast, typecheck::root::FunctionCtxt, typed_ast, types::TypeKind};

pub struct Coercion<'a, 'ctxt> {
    target_ty: Option<TypeKind>,
    exprs: Vec<typed_ast::Expr>,
    ctxt: &'a FunctionCtxt<'a, 'ctxt>,
}
impl<'a, 'b> Coercion<'a, 'b> {
    pub fn new(target_ty: Option<TypeKind>, ctxt: &'a FunctionCtxt<'a, 'b>) -> Coercion<'a, 'b> {
        Coercion {
            target_ty,
            exprs: Vec::new(),
            ctxt,
        }
    }

    pub fn check_expr(&mut self, expr: &resolved_ast::Expr) {
        self.exprs.push(
            self.ctxt
                .check_expr_coerces_to(expr, self.target_ty.clone()),
        );
    }

    pub fn finish(self) -> (Option<TypeKind>, Vec<typed_ast::Expr>) {
        let Some(combined_ty) = self
            .ctxt
            .merge_ty(self.exprs.iter().map(|expr| expr.ty.clone()))
        else {
            return (self.target_ty, self.exprs);
        };
        let exprs = self
            .exprs
            .into_iter()
            .map(|expr| {
                let Ok(coercion) =
                    self.ctxt
                        .unify_or_coerce(expr.loc, combined_ty.clone(), expr.ty.clone())
                else {
                    return expr;
                };
                self.ctxt.apply_coercion(coercion, expr)
            })
            .collect();
        (Some(combined_ty), exprs)
    }
}
