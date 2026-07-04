use std::collections::{HashMap, HashSet};

use crate::{
    CtxtRef,
    res_visitor::Visitor,
    resolved_ast::{DefId, ExprKind, VarId},
};
#[derive(Clone, Default)]
pub struct CaptureSet {
    map: HashMap<VarId, usize>,
    vars: Vec<VarId>,
}
impl CaptureSet {
    pub fn captured(&self, var: VarId) -> bool {
        self.map.contains_key(&var)
    }
    pub fn capture_index(&self, var: VarId) -> Option<usize> {
        self.map.get(&var).copied()
    }
    pub fn iter_vars(&self) -> impl Iterator<Item = VarId> {
        self.vars.iter().copied()
    }
    pub fn into_vars(self) -> Vec<VarId> {
        self.vars
    }
}

#[track_caller]
pub fn captures(ctxt: CtxtRef<'_>, id: DefId) -> Option<CaptureSet> {
    let lambda = ctxt.node(id).lambda()?;
    pub struct CaptureCollector<'ctxt> {
        ctxt: CtxtRef<'ctxt>,
        locals: HashSet<VarId>,
        captures: Vec<VarId>,
    }
    impl CaptureCollector<'_> {
        fn visit_var_use(&mut self, var: VarId) {
            if !self.locals.contains(&var) {
                self.captures.push(var);
            }
        }
    }
    impl Visitor for CaptureCollector<'_> {
        fn visit_var_def(&mut self, var: crate::resolved_ast::Var) {
            self.locals.insert(var.1);
        }
        fn visit_expr(&mut self, expr: &crate::resolved_ast::Expr) {
            match &expr.kind {
                &ExprKind::Var(var) => self.visit_var_use(var.1),
                ExprKind::Lambda(lambda) => {
                    for var in self
                        .ctxt
                        .captures(lambda.id)
                        .map(|captures| captures.vars)
                        .into_iter()
                        .flatten()
                    {
                        self.visit_var_use(var);
                    }
                }
                _ => self.super_visit_expr(expr),
            }
        }
    }
    let mut collector = CaptureCollector {
        ctxt,
        locals: HashSet::new(),
        captures: Vec::new(),
    };
    collector.visit_body(
        lambda.param_tys.iter().flatten(),
        lambda.params.iter(),
        &lambda.body,
    );
    println!("{:?} {:?}", id, collector.captures);
    let mut map = HashMap::new();
    for (i, var) in collector.captures.iter().copied().enumerate() {
        map.insert(var, i);
    }
    Some(CaptureSet {
        map,
        vars: collector.captures,
    })
}
