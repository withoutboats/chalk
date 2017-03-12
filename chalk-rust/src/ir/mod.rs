use cast::Cast;
use chalk_rust_parse::ast;
use fold::Subst;
use lalrpop_intern::InternedString;
use std::collections::{HashSet, HashMap};
use std::sync::Arc;

pub type Identifier = InternedString;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// From type-name to item-id. Used during lowering only.
    pub type_ids: HashMap<Identifier, ItemId>,

    /// For each struct/trait:
    pub type_kinds: HashMap<ItemId, TypeKind>,

    /// For each impl:
    pub impl_data: HashMap<ItemId, ImplDatum>,

    /// For each trait:
    pub trait_data: HashMap<ItemId, TraitDatum>,

    /// For each trait:
    pub associated_ty_data: HashMap<ItemId, AssociatedTyDatum>,

    /// Compiled forms of the above:
    pub program_clauses: Vec<ProgramClause>,
}

impl Program {
    pub fn split_projection<'p>(&self, projection: &'p ProjectionTy)
                            -> (&AssociatedTyDatum, &'p [Parameter], &'p [Parameter]) {
        let ProjectionTy { associated_ty_id, ref parameters } = *projection;
        let associated_ty_data = &self.associated_ty_data[&associated_ty_id];
        let trait_datum = &self.trait_data[&associated_ty_data.trait_id];
        let trait_num_params = trait_datum.binders.len();
        let split_point = parameters.len() - trait_num_params;
        let (other_params, trait_params) = parameters.split_at(split_point);
        (associated_ty_data, trait_params, other_params)
    }
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Environment {
    pub universe: UniverseIndex,
    pub clauses: Vec<WhereClause>,
}

impl Environment {
    pub fn new() -> Arc<Environment> {
        Arc::new(Environment { universe: UniverseIndex::root(), clauses: vec![] })
    }

    pub fn add_clauses<I>(&self, clauses: I) -> Arc<Environment>
        where I: IntoIterator<Item = WhereClause>
    {
        let mut env = self.clone();
        env.clauses.extend(clauses);
        Arc::new(env)
    }

    pub fn new_universe(&self) -> Arc<Environment> {
        let mut env = self.clone();
        env.universe = UniverseIndex { counter: self.universe.counter + 1 };
        Arc::new(env)
    }

    pub fn elaborated_clauses(&self, program: &Program) -> impl Iterator<Item = WhereClause> {
        let mut set = HashSet::new();
        set.extend(self.clauses.iter().cloned());

        let mut stack: Vec<_> = set.iter().cloned().collect();

        while let Some(clause) = stack.pop() {
            let mut push_clause = |clause: WhereClause| {
                if !set.contains(&clause) {
                    set.insert(clause.clone());
                    stack.push(clause);
                }
            };

            match clause {
                WhereClause::Implemented(ref trait_ref) => {
                    // trait Foo<A> where Self: Bar<A> { }
                    // T: Foo<U>
                    // ----------------------------------------------------------
                    // T: Bar<U>

                    let trait_datum = &program.trait_data[&trait_ref.trait_id];
                    for where_clause in &trait_datum.binders.value.where_clauses {
                        let where_clause = Subst::apply(&trait_ref.parameters, where_clause);
                        push_clause(where_clause);
                    }
                }
                WhereClause::Normalize(Normalize { ref projection, ty: _ }) => {
                    // <T as Trait<U>>::Foo ===> V
                    // ----------------------------------------------------------
                    // T: Trait<U>

                    let (associated_ty_data, trait_params, _) = program.split_projection(projection);
                    let trait_ref = TraitRef {
                        trait_id: associated_ty_data.trait_id,
                        parameters: trait_params.to_owned()
                    };
                    push_clause(trait_ref.cast());
                }
            }
        }

        set.into_iter()
    }
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InEnvironment<G> {
    pub environment: Arc<Environment>,
    pub goal: G,
}

impl<G> InEnvironment<G> {
    pub fn new(environment: &Arc<Environment>, goal: G) -> Self {
        InEnvironment { environment: environment.clone(), goal }
    }

    pub fn map<OP, H>(self, op: OP) -> InEnvironment<H>
        where OP: FnOnce(G) -> H
    {
        InEnvironment {
            environment: self.environment,
            goal: op(self.goal),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypeName {
    /// a type like `Vec<T>`
    ItemId(ItemId),

    /// skolemized form of a type parameter like `T`
    ForAll(UniverseIndex),

    /// an associated type like `Iterator::Item`; see `AssociatedType` for details
    AssociatedType(ItemId),
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UniverseIndex {
    pub counter: usize,
}

impl UniverseIndex {
    pub fn root() -> UniverseIndex {
        UniverseIndex { counter: 0 }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId {
    pub index: usize
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KrateId {
    pub name: Identifier
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Krate {
    Var(usize),
    Id(KrateId),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TypeKind {
    pub sort: TypeSort,
    pub krate_id: KrateId,
    pub name: Identifier,
    pub binders: Binders<()>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypeSort {
    Struct,
    Trait,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ImplDatum {
    pub krate_id: KrateId,
    pub binders: Binders<ImplDatumBound>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ImplDatumBound {
    pub trait_ref: TraitRef,
    pub where_clauses: Vec<WhereClause>,
    pub associated_ty_values: Vec<AssociatedTyValue>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StructDatum {
    pub krate_id: KrateId,
    pub binders: Binders<StructDatumBound>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StructDatumBound {
    pub self_ty: ApplicationTy,
    pub where_clauses: Vec<WhereClause>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TraitDatum {
    pub krate_id: KrateId,
    pub binders: Binders<TraitDatumBound>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TraitDatumBound {
    pub trait_ref: TraitRef,
    pub where_clauses: Vec<WhereClause>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AssociatedTyDatum {
    /// The trait this associated type is defined in.
    pub trait_id: ItemId,

    /// Name of this associated type.
    pub name: Identifier,

    /// Parameters on this associated type, beginning with those from the trait,
    /// but possibly including more.
    pub parameter_kinds: Vec<ParameterKind<Identifier>>,

    /// Where clauses that must hold for the projection be well-formed.
    pub where_clauses: Vec<WhereClause>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AssociatedTyValue {
    pub associated_ty_id: ItemId,

    // note: these binders are in addition to those from the impl
    pub value: Binders<AssociatedTyValueBound>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AssociatedTyValueBound {
    /// Type that we normalize to. The X in `type Foo<'a> = X`.
    pub ty: Ty,

    /// Where-clauses that must hold for projection to be valid. The
    /// WC in `type Foo<'a> = X where WC`.
    pub where_clauses: Vec<WhereClause>,
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ty {
    /// References the binding at the given depth (deBruijn index
    /// style). In an inference context (i.e., when solving goals),
    /// free bindings refer into the inference table.
    Var(usize),
    Apply(ApplicationTy),
    Projection(ProjectionTy),
    ForAll(Box<QuantifiedTy>),
}

/// for<'a...'z> X -- all binders are instantiated at once,
/// and we use deBruijn indices within `self.ty`
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QuantifiedTy {
    pub num_binders: usize,
    pub ty: Ty
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lifetime {
    /// See Ty::Var(_).
    Var(usize),
    ForAll(UniverseIndex),
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ApplicationTy {
    pub name: TypeName,
    pub parameters: Vec<Parameter>,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ParameterKind<T, L = T, C = T> {
    Ty(T),
    Lifetime(L),
    Krate(C),
}

impl<T> ParameterKind<T> {
    pub fn map<OP, U>(self, op: OP) -> ParameterKind<U>
        where OP: FnOnce(T) -> U
    {
        match self {
            ParameterKind::Ty(t) => ParameterKind::Ty(op(t)),
            ParameterKind::Lifetime(t) => ParameterKind::Lifetime(op(t)),
            ParameterKind::Krate(t) => ParameterKind::Krate(op(t)),
        }
    }
}

impl<T, L, C> ParameterKind<T, L, C> {
    pub fn as_ref(&self) -> ParameterKind<&T, &L, &C> {
        match *self {
            ParameterKind::Ty(ref t) => ParameterKind::Ty(t),
            ParameterKind::Lifetime(ref l) => ParameterKind::Lifetime(l),
            ParameterKind::Krate(ref c) => ParameterKind::Krate(c),
        }
    }

    pub fn ty(self) -> Option<T> {
        match self {
            ParameterKind::Ty(t) => Some(t),
            _ => None,
        }
    }

    pub fn lifetime(self) -> Option<L> {
        match self {
            ParameterKind::Lifetime(t) => Some(t),
            _ => None,
        }
    }

    pub fn krate(self) -> Option<C> {
        match self {
            ParameterKind::Krate(t) => Some(t),
            _ => None,
        }
    }
}

impl<T, L, C> ast::Kinded for ParameterKind<T, L, C> {
    fn kind(&self) -> ast::Kind {
        match *self {
            ParameterKind::Ty(_) => ast::Kind::Ty,
            ParameterKind::Lifetime(_) => ast::Kind::Lifetime,
            ParameterKind::Krate(_) => ast::Kind::Krate,
        }
    }
}

pub type Parameter = ParameterKind<Ty, Lifetime, Krate>;

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectionTy {
    pub associated_ty_id: ItemId,
    pub parameters: Vec<Parameter>,
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TraitRef {
    pub trait_id: ItemId,
    pub parameters: Vec<Parameter>,
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WhereClause {
    Implemented(TraitRef),
    Normalize(Normalize),
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WhereClauseGoal {
    Implemented(TraitRef),
    Normalize(Normalize),
    UnifyTys(Unify<Ty>),
    UnifyKrates(Unify<Krate>),
    WellFormed(WellFormed),
    TyLocalTo(LocalTo<Ty>),

    NotImplemented(Not<TraitRef>),
    NotNormalize(Not<Normalize>),
    NotUnifyTys(Not<Unify<Ty>>),
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WellFormed {
    Ty(Ty),
    TraitRef(TraitRef),
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalTo<F> {
    pub value: F,
    pub krate: Krate,
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Unify<T> {
    pub a: T,
    pub b: T,
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Not<T> {
    pub predicate: T,
    pub krate: Krate,
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Normalize {
    pub projection: ProjectionTy,
    pub ty: Ty,
}

/// Indicates that the `value` is universally quantified over `N`
/// parameters of the given kinds, where `N == self.binders.len()`. A
/// variable with depth `i < N` refers to the value at
/// `self.binders[i]`. Variables with depth `>= N` are free.
///
/// (IOW, we use deBruijn indices, where binders are introduced in
/// reverse order of `self.binders`.)
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Binders<T> {
    pub binders: Vec<ParameterKind<()>>,
    pub value: T,
}

impl<T> Binders<T> {
    pub fn map_ref<U, OP>(&self, op: OP) -> Binders<U>
        where OP: FnOnce(&T) -> U
    {
        let value = op(&self.value);
        Binders {
            binders: self.binders.clone(),
            value: value,
        }
    }

    pub fn len(&self) -> usize {
        self.binders.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProgramClause {
    pub implication: Binders<ProgramClauseImplication>
}

/// Represents one clause of the form `consequence :- conditions`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProgramClauseImplication {
    pub consequence: WhereClauseGoal,
    pub conditions: Vec<Goal>,
}

/// Wraps a "canonicalized query". Queries are canonicalized as follows:
///
/// - All unresolved existential variables are "renumbered" according
///   to their first appearance; the kind/universe of the variable is
///   recorded in the `binders` field.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Query<T> {
    pub value: T,
    pub binders: Vec<ParameterKind<UniverseIndex>>,
}

impl<T> Query<T> {
    pub fn map<OP, U>(self, op: OP) -> Query<U>
        where OP: FnOnce(T) -> U
    {
        Query { value: op(self.value), binders: self.binders }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Constrained<T> {
    pub value: T,
    pub constraints: Vec<InEnvironment<Constraint>>,
}

impl<T> Constrained<T> {
    pub fn map<OP, U>(self, op: OP) -> Constrained<U>
        where OP: FnOnce(T) -> U
    {
        Constrained { value: op(self.value), constraints: self.constraints }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Goal {
    /// Introduces a binding at depth 0, shifting other bindings up
    /// (deBruijn index).
    Quantified(QuantifierKind, Binders<Box<Goal>>),
    Implies(Vec<WhereClause>, Box<Goal>),
    And(Box<Goal>, Box<Goal>),
    Leaf(WhereClauseGoal),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum QuantifierKind {
    ForAll, Exists
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Constraint {
    LifetimeEq(Lifetime, Lifetime),
}

pub mod debug;
mod tls;

pub use self::tls::set_current_program;
pub use self::tls::with_current_program;

