use anyhow::{Context as _, Result};
use boa_engine::{Context, JsValue, Source};
use std::{
    fs,
    io::{self, Write},
    path::Path,
};

/// Human-readable name of the backend selected at compile time.
pub fn backend_name() -> &'static str {
    match crate::backend::selected() {
        crate::backend::BackendKind::Llvm => "LLVM",
        crate::backend::BackendKind::Cranelift => "Cranelift",
        _ => "Baseline JIT",
    }
}

fn new_context() -> Context {
    let mut cx = Context::default();
    cx.register_global_builtin_callable("print", 1, |_this, args, _cx| {
        println!("{}", args.first().unwrap_or(&JsValue::undefined()));
        Ok(JsValue::undefined())
    })
    .expect("print registration");
    cx.register_global_builtin_callable("exit", 1, |_this, args, _cx| {
        let code = args
            .first()
            .and_then(|v| v.as_number())
            .unwrap_or(0.0) as i32;
        std::process::exit(code)
    })
    .expect("exit registration");
    cx
}

pub fn run_file(path: &Path) -> Result<()> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    run_source(&source, &path.to_string_lossy())
}

pub fn run_source(source: &str, name: &str) -> Result<()> {
    let mut cx = new_context();
    cx.eval(Source::from_bytes(source))
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("{name}: JavaScript error: {e}"))
}

pub fn repl() -> Result<()> {
    println!(
        "Neutronium.js REPL {} — Ctrl-D to exit",
        env!("CARGO_PKG_VERSION")
    );
    let mut cx = new_context();
    let stdin = io::stdin();
    loop {
        print!("> ");
        io::stdout().flush()?;
        let mut line = String::new();
        if stdin.read_line(&mut line)? == 0 {
            break;
        }
        match cx.eval(Source::from_bytes(&line)) {
            Ok(v) => println!("{}", v.display()),
            Err(e) => eprintln!("error: {e}"),
        }
    }
    Ok(())
}
