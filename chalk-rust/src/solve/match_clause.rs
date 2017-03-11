use cast::Cast;
use errors::*;
use fold::Fold;
use ir::*;
use solve::Solution;
use solve::fulfill::Fulfill;
use solve::infer::InferenceTable;
use solve::solver::Solver;
use std::sync::Arc;

pub struct MatchClause<'s, G: 's> {
    fulfill: Fulfill<'s>,
    environment: Arc<Environment>,
    goal: &'s G,
    clause: &'s WhereClause,
}

impl<'s, G> MatchClause<'s, G>
    where G: Clone + Cast<WhereClause> + Fold<Result = G>
{
    pub fn new(solver: &'s mut Solver,
               q: &'s Query<InEnvironment<G>>,
               clause: &'s WhereClause)
               -> Self {
        let InEnvironment { ref environment, ref goal } = q.value;
        let infer = InferenceTable::new_with_vars(&q.binders);
        let environment = environment.clone();
        let fulfill = Fulfill::new(solver, infer);
        MatchClause { fulfill, environment, goal, clause }
    }

    pub fn solve(mut self) -> Result<Solution<InEnvironment<G>>> {
        let environment = self.environment.clone();
        self.fulfill.unify(&environment, &self.goal.clone().cast(), &self.clause)?;
        let successful = self.fulfill.solve_all()?;
        let refined_goal = self.fulfill.refine_goal(InEnvironment::new(&environment, &self.goal));
        Ok(Solution {
            successful: successful,
            refined_goal: refined_goal,
        })
    }
}
