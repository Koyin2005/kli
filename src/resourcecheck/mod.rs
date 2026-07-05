use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    ops::ControlFlow,
};

use crate::{
    Symbol,
    ast::{IsResource, Mutable},
    collect::CtxtRef,
    define_id,
    diagnostics::DiagnosticReporter,
    index_vec::IndexVec,
    resolved_ast::{DefId, LocalRegionId, Var, VarId},
    src_loc::SrcLoc,
    typed_ast::{
        Expr, ExprKind, FieldId, Function, Pattern, PatternKind, Place, PlaceKind, Stmt, StmtKind,
    },
    types::{Region, Type},
};

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub enum MovePlace {
    Var(Var),
    FieldOf(Box<MovePlace>, FieldId),
    Deref(Box<MovePlace>),
}
impl MovePlace {
    fn indirect(&self) -> bool {
        match self {
            Self::Var(_) => false,
            Self::Deref(_) => true,
            Self::FieldOf(parent, _) => parent.indirect(),
        }
    }
    fn from_place(place: &Place) -> Option<Self> {
        match place.kind {
            PlaceKind::Invalid => None,
            PlaceKind::Deref(ref inner) => {
                let ExprKind::Load(ref place) = inner.kind else {
                    return None;
                };
                Self::from_place(place).map(|inner| MovePlace::Deref(Box::new(inner)))
            }
            PlaceKind::Var(var) | PlaceKind::Upvar(_, var) => Some(MovePlace::Var(var)),
            PlaceKind::Field(ref place, field) => {
                Self::from_place(place).map(|place| MovePlace::FieldOf(Box::new(place), field))
            }
        }
    }
}
#[derive(PartialEq, Eq, Clone, Copy, Hash)]
enum PlaceProjection {
    Field(FieldId),
    Deref,
}
#[derive(Debug, Clone, Copy)]
enum PlaceUse {
    Read,
    Write,
}
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum PlaceState {
    Owned,
    Moved,
}
struct VarInfo {
    ty: Type,
    name: Symbol,
    mutable: Mutable,
    loop_count: usize,
}
fn unify_state(state1: PlaceState, state2: PlaceState) -> Option<PlaceState> {
    match (state1, state2) {
        (PlaceState::Moved, PlaceState::Moved) => Some(PlaceState::Moved),
        (PlaceState::Owned, PlaceState::Owned) => Some(PlaceState::Owned),
        (PlaceState::Owned, PlaceState::Moved) | (PlaceState::Moved, PlaceState::Owned) => None,
    }
}
fn unify_state_or_move(state1: PlaceState, state2: PlaceState) -> PlaceState {
    unify_state(state1, state2).unwrap_or(PlaceState::Moved)
}
#[derive(PartialEq, Eq, Hash, Debug, Clone, Copy)]
enum PlacePath {
    Var(VarId),
    Field(MovePlaceId, FieldId),
    Deref(MovePlaceId),
}
define_id!(MovePlaceId);
struct PlaceInfo {
    parent: Option<MovePlaceId>,
    place: PlacePath,
    state: PlaceState,
    children: HashMap<PlaceProjection, MovePlaceId>,
}
#[derive(Default)]
pub struct PlaceMap {
    place_info: IndexVec<MovePlaceId, PlaceInfo>,
    place_ids: HashMap<PlacePath, MovePlaceId>,
}
impl PlaceMap {
    pub fn new() -> Self {
        Default::default()
    }
    fn state_map(&self) -> HashMap<MovePlaceId, PlaceState> {
        self.place_info
            .iter_enumerated()
            .map(|(id, info)| (id, info.state))
            .collect()
    }
    fn id_of(&mut self, place: &MovePlace) -> MovePlaceId {
        match place {
            &MovePlace::Var(var) => {
                *self
                    .place_ids
                    .entry(PlacePath::Var(var.1))
                    .or_insert_with(|| {
                        self.place_info.push(PlaceInfo {
                            parent: None,
                            place: PlacePath::Var(var.1),
                            children: HashMap::new(),
                            state: PlaceState::Owned,
                        })
                    })
            }
            &MovePlace::FieldOf(ref parent, field) => {
                let parent = self.id_of(parent);
                if let Some(child) = self.place_info[parent]
                    .children
                    .get(&PlaceProjection::Field(field))
                {
                    return *child;
                }
                let path = PlacePath::Field(parent, field);
                let id = *self.place_ids.entry(path).or_insert_with(|| {
                    self.place_info.push(PlaceInfo {
                        parent: Some(parent),
                        place: path,
                        children: HashMap::new(),
                        state: PlaceState::Owned,
                    })
                });
                self.place_info[parent]
                    .children
                    .insert(PlaceProjection::Field(field), id);
                id
            }
            MovePlace::Deref(parent) => {
                let parent = self.id_of(parent);
                if let Some(&child) = self.place_info[parent]
                    .children
                    .get(&PlaceProjection::Deref)
                {
                    return child;
                }
                let path = PlacePath::Deref(parent);
                let id = *self.place_ids.entry(path).or_insert_with(|| {
                    self.place_info.push(PlaceInfo {
                        parent: Some(parent),
                        place: path,
                        children: HashMap::new(),
                        state: PlaceState::Owned,
                    })
                });
                self.place_info[parent]
                    .children
                    .insert(PlaceProjection::Deref, id);
                id
            }
        }
    }
    fn combine_parent_states(&self, mut id: MovePlaceId) -> Option<PlaceState> {
        let mut state = None;
        while let Some(parent) = self.place_info[id].parent {
            let parent_state = self.state_of(parent);
            state = match state {
                Some(current) => Some(unify_state_or_move(parent_state, current)),
                None => Some(parent_state),
            };
            id = parent;
        }
        state
    }
    fn combined_state(&self, id: MovePlaceId) -> PlaceState {
        let parent_state = self.combine_parent_states(id).unwrap_or(PlaceState::Owned);
        let child_states = self.place_info[id]
            .children
            .values()
            .map(|child| self.place_info[*child].state)
            .fold(PlaceState::Owned, unify_state_or_move);
        unify_state_or_move(
            unify_state_or_move(parent_state, child_states),
            self.state_of(id),
        )
    }
    fn state_of(&self, id: MovePlaceId) -> PlaceState {
        self.place_info[id].state
    }
    fn update_state(&mut self, id: MovePlaceId, state: PlaceState) {
        self.place_info[id].state = state;
    }
    fn update_state_with_children(&mut self, id: MovePlaceId, state: PlaceState) {
        self.update_state(id, state);
        for child in self.place_info[id]
            .children
            .values()
            .copied()
            .collect::<Vec<_>>()
        {
            self.update_state(child, state);
        }
    }
    fn update_states(&mut self, map: HashMap<MovePlaceId, PlaceState>) {
        for (id, state) in map {
            self.update_state(id, state);
        }
    }
}
pub struct ResourceCheck<'ctxt> {
    vars: HashMap<VarId, VarInfo>,
    err: DiagnosticReporter,
    place_map: PlaceMap,
    expired_regions: HashSet<LocalRegionId>,
    borrowed: HashMap<MovePlaceId, (Mutable, Region)>,
    scopes: Vec<Vec<VarId>>,
    loops: usize,
    ctxt: CtxtRef<'ctxt>,
}
impl<'ctxt> ResourceCheck<'ctxt> {
    pub fn new(ctxt: CtxtRef<'ctxt>) -> Self {
        Self {
            ctxt,
            vars: HashMap::new(),
            err: DiagnosticReporter::new(),
            scopes: Vec::new(),
            place_map: PlaceMap::new(),
            expired_regions: HashSet::new(),
            loops: 0,
            borrowed: HashMap::new(),
        }
    }
    fn ty_is_expired(&self, ty: &Type) -> bool {
        ty.visit(&mut Type::no_op_visit, &mut |region| {
            if let Region::Local(_, ref local) = region
                && self.expired_regions.contains(local)
            {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .is_break()
    }
    fn in_drop_scope(&mut self, f: impl FnOnce(&mut Self)) -> Vec<VarId> {
        self.scopes.push(Default::default());
        f(self);
        let Some(scope) = self.scopes.pop() else {
            return Vec::new();
        };
        for &var in &scope {
            let var_info = &self.vars[&var];
            let id = self
                .place_map
                .id_of(&MovePlace::Var(Var(var_info.name, var)));
            self.borrowed.remove(&id);
        }
        scope
    }
    fn init_var(&mut self, mutable: Mutable, var: VarId, name: Symbol, ty: Type) {
        self.scopes.last_mut().unwrap().push(var);
        self.vars.insert(
            var,
            VarInfo {
                ty,
                name,
                mutable,
                loop_count: self.loops,
            },
        );
        self.place_map.id_of(&MovePlace::Var(Var(name, var)));
    }
    fn place_mutable(&self, place: &Place) -> Mutable {
        match &place.kind {
            PlaceKind::Var(var) | PlaceKind::Upvar(_, var) => self.vars[&var.1].mutable,
            PlaceKind::Invalid => Mutable::Mutable,
            PlaceKind::Field(place, _) => self.place_mutable(place),
            PlaceKind::Deref(inner) => {
                if let Type::Imm(..) = inner.ty {
                    return Mutable::Immutable;
                }
                let mut inner_place = place;
                while let PlaceKind::Deref(ref expr) = inner_place.kind {
                    let ExprKind::Load(ref place) = expr.kind else {
                        return Mutable::Mutable;
                    };
                    if matches!(expr.ty, Type::Imm(..)) {
                        return Mutable::Immutable;
                    }
                    inner_place = place;
                }
                Mutable::Mutable
            }
        }
    }
    fn check_place_mutable(&mut self, place: &Place) {
        match self.place_mutable(place) {
            Mutable::Immutable => self
                .err
                .add_diagnostic("Cannot write to immutable place".to_string(), place.loc),
            Mutable::Mutable => (),
        }
    }
    fn check_pattern(&mut self, pattern: &Pattern, in_ref: bool) {
        match &pattern.kind {
            PatternKind::Bool(_) | PatternKind::Int(_) | PatternKind::Err | PatternKind::Unit => (),
            PatternKind::Record(fields) => {
                for field in fields {
                    self.check_pattern(&field.pattern, in_ref);
                }
            }
            PatternKind::Ref(sub_pattern) => {
                self.check_pattern(sub_pattern, true);
            }
            PatternKind::Case(.., inner) => {
                if let Some(inner) = inner {
                    self.check_pattern(inner, in_ref);
                }
            }
            PatternKind::Binding(borrow, mutable, var, ty) => {
                if in_ref && ty.is_resource(self.ctxt) && borrow.is_none() {
                    self.err
                        .add_diagnostic("Cannot move out of reference".to_string(), pattern.loc)
                }
                self.init_var(*mutable, var.1, var.0, (**ty).clone());
            }
        }
    }
    fn regions_in(&self, ty: &Type) -> HashSet<Region> {
        let mut regions = HashSet::new();
        let ControlFlow::Continue(()) = ty.visit(
            &mut Type::no_op_visit::<std::convert::Infallible>,
            &mut |region| {
                regions.insert(region);
                ControlFlow::Continue(())
            },
        );
        regions
    }
    fn outlives_generic_regions(&self, ty: &Type) -> bool {
        ty.visit(&mut Type::no_op_visit, &mut |region| {
            if matches!(region, Region::Unknown | Region::Static | Region::Param(..)) {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .is_continue()
    }
    fn type_of(&self, place: &MovePlace) -> Type {
        match place {
            MovePlace::Deref(inner) => self.type_of(inner).as_pointer_type().unwrap().1,
            MovePlace::FieldOf(inner, field) => {
                self.type_of(inner).field_info(*field, self.ctxt).unwrap().0
            }
            MovePlace::Var(var) => self.vars[&var.1].ty.clone(),
        }
    }
    fn type_of_path(&self, path: PlacePath) -> Type {
        match path {
            PlacePath::Deref(inner) => {
                self.type_of_path(self.place_map.place_info[inner].place)
                    .as_pointer_type()
                    .unwrap()
                    .1
            }
            PlacePath::Field(inner, field) => {
                self.type_of_path(self.place_map.place_info[inner].place)
                    .field_info(field, self.ctxt)
                    .unwrap()
                    .0
            }
            PlacePath::Var(var) => self.vars[&var].ty.clone(),
        }
    }
    fn mutability_of(&self, place: &MovePlace) -> Mutable {
        match place {
            MovePlace::Var(var) => self.vars[&var.1].mutable,
            MovePlace::FieldOf(parent, _) => self.mutability_of(parent),
            MovePlace::Deref(_) => {
                let mut inner_place = place;
                while let MovePlace::Deref(inner) = inner_place {
                    let ty = self.type_of(inner);
                    if matches!(ty, Type::Imm(..)) {
                        return Mutable::Immutable;
                    }
                    inner_place = inner;
                }
                Mutable::Mutable
            }
        }
    }
    fn loop_depth_of(&self, place_id: MovePlaceId) -> usize {
        match self.place_map.place_info[place_id].place {
            PlacePath::Var(var) => self.vars[&var].loop_count,
            PlacePath::Field(parent, _) | PlacePath::Deref(parent) => self.loop_depth_of(parent),
        }
    }
    fn borrow_of(&self, place_id: MovePlaceId) -> Option<(Mutable, Region)> {
        self.borrowed
            .get(&place_id)
            .copied()
            .or_else(|| {
                let mut parent = self.place_map.place_info[place_id].parent;
                while let Some(curr) = parent {
                    if let Some(info) = self.borrowed.get(&curr).copied() {
                        return Some(info);
                    }
                    parent = self.place_map.place_info[curr].parent;
                }
                None
            })
            .or_else(|| {
                for &child in self.place_map.place_info[place_id].children.values() {
                    if let Some(borrow) = self.borrow_of(child) {
                        return Some(borrow);
                    }
                }
                None
            })
    }
    #[track_caller]
    fn format_move_place(&self, place: &MovePlace) -> (Type, String) {
        match place {
            MovePlace::Var(var) => {
                let info = &self.vars[&var.1];
                (info.ty.clone(), info.name.to_string())
            }
            MovePlace::FieldOf(inner, field) => {
                let (inner_ty, mut inner_str) = self.format_move_place(inner);
                let (ty, name) = inner_ty
                    .field_info(*field, self.ctxt)
                    .expect("should have a field");
                inner_str.push('.');
                inner_str.push_str(&name.to_string());
                (ty, inner_str)
            }
            MovePlace::Deref(inner) => {
                let (inner_ty, mut inner_str) = self.format_move_place(inner);
                let ty = inner_ty.as_pointer_type().unwrap().1;
                inner_str.push('^');
                (ty, inner_str)
            }
        }
    }
    fn check_move_place_use(
        &mut self,
        loc: SrcLoc,
        ty: &Type,
        place: MovePlace,
        place_use: PlaceUse,
    ) {
        let id = self.place_map.id_of(&place);
        let mutable = self.mutability_of(&place);
        match place_use {
            PlaceUse::Read => {
                if self.place_map.combined_state(id) == PlaceState::Moved {
                    self.err.add_diagnostic(
                        format!(
                            "Cannot read from '{}' as it has been moved from",
                            self.format_move_place(&place).1
                        ),
                        loc,
                    );
                    return;
                }
            }
            PlaceUse::Write => {
                if mutable == Mutable::Immutable {
                    self.err.add_diagnostic(
                        format!(
                            "Cannot write to immutable place '{}'",
                            self.format_move_place(&place).1
                        ),
                        loc,
                    );
                }
            }
        }
        let new_state = match place_use {
            PlaceUse::Write => {
                if ty.is_resource(self.ctxt) && self.borrow_of(id).is_some() {
                    self.err.add_diagnostic(
                        format!(
                            "Cannot assign to '{}' while borrowed",
                            self.format_move_place(&place).1
                        ),
                        loc,
                    );
                    return;
                }

                if self.place_map.state_of(id) == PlaceState::Owned {
                    return;
                }

                PlaceState::Owned
            }
            PlaceUse::Read => {
                if ty.is_resource(self.ctxt) {
                    let loop_depth = self.loop_depth_of(id);
                    if loop_depth < self.loops {
                        self.err.add_diagnostic(
                            format!(
                                "Cannot move from '{}' in a loop",
                                self.format_move_place(&place).1
                            ),
                            loc,
                        );
                        return;
                    }

                    if self.borrow_of(id).is_some() {
                        self.err.add_diagnostic(
                            format!(
                                "Cannot move from '{}' while borrowed",
                                self.format_move_place(&place).1
                            ),
                            loc,
                        );
                        return;
                    }
                    if place.indirect() {
                        self.err.add_diagnostic(
                            format!(
                                "Cannot move from '{}', as it contains indirection",
                                self.format_move_place(&place).1
                            ),
                            loc,
                        );
                        return;
                    }
                    PlaceState::Moved
                } else {
                    PlaceState::Owned
                }
            }
        };
        self.place_map.update_state_with_children(id, new_state);
    }
    fn can_borrow_place_with_region(&self, region: Region, id: MovePlaceId) -> bool {
        let path = self.place_map.place_info[id].place;
        match path {
            PlacePath::Var(_) => {
                matches!(region, Region::Local(..))
            }
            PlacePath::Field(parent, _) => self.can_borrow_place_with_region(region, parent),
            PlacePath::Deref(_) => {
                let ty = self.type_of_path(path);
                let regions_in = self.regions_in(&ty);
                match region {
                    Region::Static | Region::Unknown | Region::Infer(_) => {
                        regions_in.iter().all(|&curr| {
                            matches!(curr, Region::Infer(_) | Region::Static | Region::Unknown)
                                || curr == region
                        })
                    }
                    Region::Local(_, _) => regions_in.iter().all(|&curr| {
                        matches!(
                            curr,
                            Region::Infer(_) | Region::Unknown | Region::Param(..) | Region::Static
                        ) || curr == region
                    }),
                    Region::Param(_, _) => regions_in.iter().all(|curr| {
                        matches!(
                            curr,
                            Region::Infer(_) | Region::Unknown | Region::Param(..) | Region::Static
                        ) || *curr == region
                    }),
                }
            }
        }
    }
    fn check_place_use(&mut self, place: &Place, place_use: PlaceUse) {
        let ty = &place.ty;
        let loc = place.loc;
        let move_place = match &place.kind {
            PlaceKind::Invalid => return,
            _ => MovePlace::from_place(place),
        };
        let Some(place) = move_place else {
            return;
        };
        self.check_move_place_use(loc, ty, place, place_use);
    }
    fn check_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Expr(expr) => {
                self.check_expr(expr);
            }
            StmtKind::Let(let_binding) => {
                self.check_pattern(&let_binding.pattern, false);
                self.check_expr(&let_binding.value);
            }
        }
    }
    fn check_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Block(body, region) => {
                self.in_drop_scope(|this| {
                    for stmt in &body.stmts {
                        this.check_stmt(stmt);
                    }
                    this.check_expr(&body.expr);
                    if let Some(region) = region {
                        this.expired_regions.insert(*region);
                        if this.ty_is_expired(&body.expr.ty) {
                            this.err.add_diagnostic(
                                format!("Cannot let '{}' escape", body.expr.ty),
                                expr.loc,
                            );
                        }
                        this.borrowed.retain(|_, (_, region)| {
                            let Region::Local(_, local) = region else {
                                return true;
                            };
                            !this.expired_regions.contains(local)
                        });
                    }
                });
            }
            ExprKind::Bool(_)
            | ExprKind::Err
            | ExprKind::Panic
            | ExprKind::Unit
            | ExprKind::String(_)
            | ExprKind::Int(_)
            | ExprKind::Const(..) => {}
            ExprKind::Function(..) => {}
            ExprKind::VariantInit(.., value) => {
                self.check_expr(value);
            }
            ExprKind::Print(value) => {
                if let Some(value) = value {
                    self.check_expr(value);
                    if value.ty.is_resource(self.ctxt) {
                        self.err.add_diagnostic(
                            format!("Cannot print resource '{}'", value.ty),
                            value.loc,
                        );
                    }
                }
            }
            ExprKind::Call(callee, args) => {
                self.check_expr(callee);
                for arg in args {
                    self.check_expr(arg);
                }
            }
            ExprKind::BuiltinCall(.., args) => {
                for arg in args {
                    self.check_expr(arg);
                }
            }
            ExprKind::List(values) => {
                for value in values {
                    self.check_expr(value);
                }
            }
            ExprKind::Record(fields) => {
                for field in fields {
                    self.check_expr(&field.value);
                }
            }
            ExprKind::Load(place) => self.check_place_use(place, PlaceUse::Read),
            ExprKind::Binary(_, left, right) => {
                self.check_expr(left);
                self.check_expr(right);
            }
            ExprKind::Assign(place, value) => {
                self.check_expr(value);
                self.check_place_mutable(place);
                self.check_place_use(place, PlaceUse::Write);
            }
            ExprKind::Lambda(lambda) => {
                self.in_drop_scope(|this| {
                    let captures = lambda.captures.as_slice();
                    let mut errors = Vec::new();
                    for capture in captures {
                        let loc = expr.loc;
                        let var = capture.var.1;
                        this.check_move_place_use(
                            loc,
                            &capture.ty,
                            MovePlace::Var(Var(this.vars[&var].name, var)),
                            PlaceUse::Read,
                        );
                        let info = &this.vars[&var];
                        //Function is not a resource, can't capture anything
                        if lambda.is_resource == IsResource::Data {
                            this.err.add_diagnostic(
                                format!(
                                    "Cannot capture '{}', as data functions cannot capture",
                                    info.name
                                ),
                                loc,
                            );
                        }
                        if !this.outlives_generic_regions(&info.ty) {
                            this.err.add_diagnostic(
                                format!(
                                    "Cannot capture '{}', as  borrowed content cannot be captured",
                                    info.name
                                ),
                                loc,
                            );
                        }

                        let var_info = &this.vars[&capture.var.1];
                        if this
                            .regions_in(&var_info.ty)
                            .iter()
                            .any(|region| *region != Region::Static || *region != Region::Unknown)
                        {
                            errors.push((var_info.name, expr.loc));
                        }
                    }
                    errors.sort_by_key(|(_, loc)| loc.line);
                    for (name, loc) in errors {
                        this.err.add_diagnostic(
                            format!("Cannot capture '{}' that contains borrows", name),
                            loc,
                        );
                    }
                });
            }
            &ExprKind::Borrow {
                mutable,
                ref place,
                region,
            } => {
                let old_var_mutable = self.place_mutable(place);
                match (old_var_mutable, mutable) {
                    (Mutable::Mutable, _) | (Mutable::Immutable, Mutable::Immutable) => (),
                    (Mutable::Immutable, Mutable::Mutable) => {
                        self.err
                            .add_diagnostic("Cannot borrow  as mut".to_string(), place.loc);
                    }
                }

                let move_place = match MovePlace::from_place(place) {
                    Some(place) => place,
                    None => {
                        self.err
                            .add_diagnostic("Cannot borrow this place".to_string(), place.loc);
                        return;
                    }
                };
                let id = self.place_map.id_of(&move_place);
                if self.place_map.combined_state(id) == PlaceState::Moved {
                    self.err.add_diagnostic(
                        format!(
                            "Cannot borrow '{}' while moved",
                            self.format_move_place(&move_place).1
                        ),
                        place.loc,
                    );
                    return;
                }
                if !self.can_borrow_place_with_region(region, id) {
                    self.err.add_diagnostic(
                        format!(
                            "Cannot borrow '{}' with region '{}'",
                            self.format_move_place(&move_place).1,
                            region
                        ),
                        place.loc,
                    );
                }
                let loc = place.loc;
                match self.borrowed.entry(id) {
                    Entry::Vacant(entry) => {
                        entry.insert_entry((mutable, region));
                    }
                    Entry::Occupied(entry) => match (entry.get().0, mutable) {
                        (Mutable::Immutable, Mutable::Immutable) => (),
                        (_, Mutable::Mutable) => {
                            self.err
                                .add_diagnostic("Cannot borrow as mut".to_string(), loc);
                        }
                        (Mutable::Mutable, Mutable::Immutable) => {
                            self.err
                                .add_diagnostic("Cannot borrow as imm".to_string(), loc);
                        }
                    },
                }
            }
            ExprKind::For {
                pattern,
                iterator,
                iterator_type: _,
                body,
            } => {
                self.check_expr(iterator);
                let new_loop = self.loops + 1;
                let old_loop = std::mem::replace(&mut self.loops, new_loop);

                self.in_drop_scope(|this| {
                    this.check_pattern(pattern, false);
                    this.check_expr(body);
                });
                self.loops = old_loop;
            }
            ExprKind::Case(value, arms) => {
                self.check_expr(value);
                let mut combined_state = if arms.is_empty() {
                    self.place_map.state_map()
                } else {
                    HashMap::new()
                };
                for arm in arms {
                    let old_state = self.place_map.state_map();
                    self.in_drop_scope(|this| {
                        this.check_pattern(&arm.pattern, false);
                        this.check_expr(&arm.body);
                    });
                    let new_state = self.place_map.state_map();
                    self.place_map.update_states(old_state);
                    for (var, state) in new_state {
                        match combined_state.entry(var) {
                            Entry::Occupied(mut entry) => {
                                let new_state = unify_state_or_move(state, *entry.get());
                                entry.insert(new_state);
                            }
                            Entry::Vacant(entry) => {
                                entry.insert(state);
                            }
                        }
                    }
                }
                self.place_map.update_states(combined_state);
            }
            ExprKind::AddressOf(place) => {
                self.check_place_use(place, PlaceUse::Write);
            }
        }
    }
    pub fn check_function(mut self, id: DefId, function: &Function) -> bool {
        self.in_drop_scope(|this| {
            let captures = this.ctxt.captures(id).unwrap_or_default().into_vars();
            for (i, param) in function.params.iter().enumerate() {
                let var = if let Some(var) = param.var {
                    var
                } else if !captures.is_empty() {
                    captures[i]
                } else {
                    continue;
                };
                this.init_var(Mutable::Immutable, var, param.name.symbol, param.ty.clone());
            }
            if let Some(ref body) = function.body {
                this.check_expr(body);
            }
        });
        self.err.report_all()
    }
}
