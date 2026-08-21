use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod backend;
mod ffi;
mod runtime;

#[derive(Parser, Debug)]
#[command(name = "neut", version, about = "Neutronium.js — быстрый модульный JavaScript runtime на Rust")]
struct Cli { #[command(subcommand)] command: Option<Command>, #[arg(value_name="FILE")] file: Option<PathBuf> }

#[derive(Subcommand, Debug)]
enum Command { Run { file: PathBuf }, Repl, Info, Build { #[arg(long, default_value="jit")] backend: String }, Ffi { library: PathBuf, symbol: String }, Load { library: PathBuf } }

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command.or_else(|| cli.file.map(|file| Command::Run { file })) {
        Some(Command::Run { file }) => runtime::run_file(&file),
        Some(Command::Repl) => runtime::repl(),
        Some(Command::Info) => { println!("Neutronium.js {}\nbackend: {}\nfeatures: FFI, C ABI, AOT, JIT, Cranelift, LLVM", env!("CARGO_PKG_VERSION"), runtime::backend_name()); Ok(()) },
        Some(Command::Build { backend }) => { let kind: backend::BackendKind = backend.parse().map_err(anyhow::Error::msg)?; println!("AOT build requested with {kind} backend"); if matches!(kind, backend::BackendKind::Llvm) && !cfg!(feature="llvm") { println!("note: rebuild with --features llvm to activate the LLVM provider"); } if matches!(kind, backend::BackendKind::Cranelift) && !cfg!(feature="cranelift") { println!("note: rebuild with --features cranelift to activate the Cranelift provider"); } Ok(()) },
        Some(Command::Ffi { library, symbol }) => ffi::inspect(&library, &symbol),
        Some(Command::Load { library }) => ffi::load(&library),
        None => { println!("neut <file.js> | neut repl | neut info | neut build --backend jit"); Ok(()) }
    }
}

