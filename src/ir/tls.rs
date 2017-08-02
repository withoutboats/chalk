use RefCell from std::cell;
use Arc from std::sync;

use Program from super;

thread_local! {
    static PROGRAM: RefCell<Option<Arc<Program>>> = RefCell::new(None)
}

pub fn with_current_program<OP, R>(op: OP) -> R
    where OP: FnOnce(Option<&Arc<Program>>) -> R
{
    PROGRAM.with(|prog_cell| {
        let p = prog_cell.borrow();
        op(p.as_ref())
    })
}

pub fn set_current_program<OP, R>(p: &Arc<Program>, op: OP) -> R
    where OP: FnOnce() -> R
{
    PROGRAM.with(|prog_cell| {
        *prog_cell.borrow_mut() = Some(p.clone());
        let r = op();
        *prog_cell.borrow_mut() = None;
        r
    })
}
