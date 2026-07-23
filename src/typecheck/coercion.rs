use crate::{resolved_ast, typecheck::root::FunctionCtxt, typed_ast, types::Type};

pub struct Coercion<'ctxt> {
    target_ty: Option<Type>,
    exprs: Vec<typed_ast::Expr>,
    ctxt: &'ctxt FunctionCtxt<'ctxt>,
}
impl Coercion<'_> {
    pub fn new<'a>(target_ty: Option<Type>, ctxt: &'a FunctionCtxt<'_>) -> Coercion<'a> {
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

    pub fn finish(self) -> (Option<Type>, Vec<typed_ast::Expr>) {
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
