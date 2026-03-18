//! Minimal GDScript interpreter example.
//!
//! Runs a short GDScript program and prints the output to stdout.
//! Demonstrates basic variable assignment, arithmetic, string concatenation,
//! and the built-in `print` / `str` functions.

use gdscript_interop::interpreter::Interpreter;

fn main() {
    let source = r#"
var greeting = "Hello from Patina!"
print(greeting)
var x = 10 + 20
print("Result: " + str(x))
"#;

    let mut interp = Interpreter::new();
    match interp.run(source) {
        Ok(result) => {
            for line in &result.output {
                println!("{line}");
            }
        }
        Err(e) => {
            eprintln!("Runtime error: {e}");
            std::process::exit(1);
        }
    }
}
