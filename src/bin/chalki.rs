use io::Read from std;
use fs::File from std;
use sync::Arc from std;

use error_chain from error_chain;
use {self, errors as parse_errors} from chalk_parse;
use error::ReadlineError from rustyline;
use Editor from rustyline;

use {ir, errors as chalk_errors} from chalk;
use * from chalk::lower;
use solver::{self, Solver, CycleStrategy} from chalk::solve;

error_chain! {
    links {
        Parse(parse_errors::Error, parse_errors::ErrorKind);
        Chalk(chalk_errors::Error, chalk_errors::ErrorKind);
    }

    foreign_links {
        Io(::std::io::Error);
        Rustyline(ReadlineError);
    }
}

struct Program {
    text: String,
    ir: Arc<ir::Program>,
    env: Arc<ir::ProgramEnvironment>,
}

impl Program {
    fn new(text: String) -> Result<Program> {
        let ir = Arc::new(chalk_parse::parse_program(&text)?.lower()?);
        let env = Arc::new(ir.environment());
        Ok(Program { text, ir, env })
    }
}

quick_main!(run);

fn run() -> Result<()> {
    // Initialize global overflow depth before everything
    let overflow_depth = 10;
    solver::set_overflow_depth(overflow_depth);

    let mut prog = None;
    readline_loop(&mut Editor::new(), "?- ", |rl, line| {
        if let Err(e) = process(line, rl, &mut prog) {
            println!("error: {}", e);
        }
    })
}

/// Repeatedly calls `f`, passing in each line, using the given promt, until EOF is received
fn readline_loop<F>(rl: &mut Editor<()>, prompt: &str, mut f: F) -> Result<()>
    where F: FnMut(&mut Editor<()>, &str)
{
    loop {
        match rl.readline(prompt) {
            Ok(line) => {
                rl.add_history_entry(&line);
                f(rl, &line);
            }
            Err(ReadlineError::Eof) => break,
            Err(e) => Err(e)?,
        }
    }

    Ok(())
}

/// Process a single command
fn process(command: &str, rl: &mut Editor<()>, prog: &mut Option<Program>) -> Result<()> {
    if command == "help" {
        help()
    } else if command == "program" {
        *prog = Some(Program::new(read_program(rl)?)?);
    } else if command.starts_with("load ") {
        let filename = &command["load ".len()..];
        let mut text = String::new();
        File::open(filename)?.read_to_string(&mut text)?;
        *prog = Some(Program::new(text)?);
    } else {
        let prog = prog.as_ref().ok_or("no program currently loaded")?;
        ir::set_current_program(&prog.ir, || -> Result<()> {
            match command {
                "print" => println!("{}", prog.text),
                "lowered" => println!("{:#?}", prog.env),
                _ => goal(command, prog)?,
            }
            Ok(())
        })?
    }

    Ok(())
}

fn help() {
    println!("Commands:");
    println!("  help         print this output");
    println!("  program      provide a program via stdin");
    println!("  load <file>  load program from <file>");
    println!("  print        print the current program");
    println!("  lowered      print the lowered program");
    println!("  <goal>       attempt to solve <goal>");
}

fn read_program(rl: &mut Editor<()>) -> Result<String> {
    println!("Enter a program; press Ctrl-D when finished");
    let mut text = String::new();
    readline_loop(rl, "| ", |_, line| {
        text += line;
        text += "\n";
    })?;
    Ok(text)
}

fn goal(text: &str, prog: &Program) -> Result<()> {
    let goal = chalk_parse::parse_goal(text)?.lower(&*prog.ir)?;
    let mut solver = Solver::new(&prog.env, CycleStrategy::Tabling, solver::get_overflow_depth());
    let goal = ir::InEnvironment::new(&ir::Environment::new(), *goal);
    match solver.solve_closed_goal(goal) {
        Ok(v) => println!("{}\n", v),
        Err(e) => println!("No possible solution: {}\n", e),
    }
    Ok(())
}
