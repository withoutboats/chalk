use errors::*;
use fold::{Fold, Folder, Shifter};
use ir::*;

use super::{InferenceTable, TyInferenceVariable, KrateInferenceVariable, LifetimeInferenceVariable,
            ParameterInferenceVariable};
use super::var::InferenceValue;

impl InferenceTable {
    /// Given a value `value` with variables in it, replaces those
    /// variables with their instantiated values; any variables not
    /// yet instantiated are replaces with a small integer index 0..N
    /// in order of appearance. The result is a canonicalized
    /// representation of `value`.
    ///
    /// Example:
    ///
    ///    ?22: Foo<?23>
    ///
    /// would be quantified to
    ///
    ///    Quantified { value: `?0: Foo<?1>`, binders: [ui(?22), ui(?23)] }
    ///
    /// where `ui(?22)` and `ui(?23)` are the universe indices of
    /// `?22` and `?23` respectively.
    pub fn make_query<T>(&mut self, value: &T) -> Query<T::Result>
        where T: Fold
    {
        debug!("make_query({:#?})", value);
        let mut q = Querifier {
            table: self,
            free_vars: QueryBinders::default(),
        };
        let r = value.fold_with(&mut q, 0).unwrap();
        Query {
            value: r,
            binders: q.into_binders(),
        }
    }
}

struct Querifier<'q> {
    table: &'q mut InferenceTable,
    free_vars: QueryBinders<TyInferenceVariable, LifetimeInferenceVariable, KrateInferenceVariable>,
}

impl<'q> Querifier<'q> {
    fn into_binders(self) -> QueryBinders {
        let Querifier { table, free_vars } = self;
        let mut binders = QueryBinders::default();
        for ty in free_vars.tys {
            debug_assert!(table.ty_unify.find(ty) == ty);
            match table.ty_unify.probe_value(ty) {
                InferenceValue::Unbound(ui) => binders.tys.push(ui),
                InferenceValue::Bound(_) => panic!("free var now bound"),
            }
        }

        for lifetime in free_vars.lifetimes {
            match table.lifetime_unify.probe_value(lifetime) {
                InferenceValue::Unbound(ui) => binders.lifetimes.push(ui),
                InferenceValue::Bound(_) => panic!("free var now bound"),
            }
        }
        for krate in free_vars.krates {
            match table.krate_unify.probe_value(krate) {
                InferenceValue::Unbound(ui) => binders.krates.push(ui),
                InferenceValue::Bound(_) => panic!("free var now bound"),
            }
        }
        binders
    }

    fn add(&mut self, free_var: ParameterInferenceVariable) -> usize {
        fn find_idx<T: Copy + Eq>(vars: &mut Vec<T>, free_var: T) -> usize {
            match vars.iter().position(|&v| v == free_var) {
                Some(i) => i,
                None    => {
                    let next_index = vars.len();
                    vars.push(free_var);
                    next_index
                }
            }
        }
        match free_var {
            ParameterKind::Ty(t)        => find_idx(&mut self.free_vars.tys, t),
            ParameterKind::Lifetime(l)  => find_idx(&mut self.free_vars.lifetimes, l),
            ParameterKind::Krate(k)     => find_idx(&mut self.free_vars.krates, k),
        }
    }
}

impl<'q> Folder for Querifier<'q> {
    fn fold_free_var(&mut self, depth: usize, binders: usize) -> Result<Ty> {
        let var = TyInferenceVariable::from_depth(depth);
        match self.table.probe_var(var) {
            Some(ty) => {
                // If this variable is bound, we want to replace it
                // with a quantified version of its bound value; we
                // also have to shift *that* into the correct binder
                // depth.
                let mut folder = (self, Shifter::new(binders));
                ty.fold_with(&mut folder, 0)
            }
            None => {
                // If this variable is not yet bound, find its
                // canonical index `root_var` in the union-find table,
                // and then map `root_var` to a fresh index that is
                // unique to this quantification.
                let free_var = ParameterKind::Ty(self.table.ty_unify.find(var));
                let position = self.add(free_var) + binders;
                Ok(TyInferenceVariable::from_depth(position).to_ty())
            }
        }
    }

    fn fold_free_lifetime_var(&mut self, depth: usize, binders: usize) -> Result<Lifetime> {
        debug!("fold_free_lifetime_var(depth={:?}, binders={:?})", depth, binders);
        let var = LifetimeInferenceVariable::from_depth(depth);
        match self.table.probe_lifetime_var(var) {
            Some(l) => {
                debug!("fold_free_lifetime_var: {:?} mapped to {:?}", var, l);
                let mut folder = (self, Shifter::new(binders));
                l.fold_with(&mut folder, 0)
            }
            None => {
                debug!("fold_free_lifetime_var: {:?} not unified", var);
                let free_var = ParameterKind::Lifetime(self.table.lifetime_unify.find(var));
                let position = self.add(free_var) + binders;
                Ok(LifetimeInferenceVariable::from_depth(position).to_lifetime())
            }
        }
    }

    fn fold_free_krate_var(&mut self, depth: usize, binders: usize) -> Result<Krate> {
        let var = KrateInferenceVariable::from_depth(depth);
        match self.table.probe_krate_var(var) {
            Some(k) => {
                let mut folder = (self, Shifter::new(binders));
                k.fold_with(&mut folder, 0)
            }
            None => {
                let free_var = ParameterKind::Krate(self.table.krate_unify.find(var));
                let position = self.add(free_var) + binders;
                Ok(KrateInferenceVariable::from_depth(position).to_krate())
            }
        }
    }
}
