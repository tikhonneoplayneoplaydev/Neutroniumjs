use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod backend;
mod ffi;
mod nim;
mod runtime;

#[cfg(feature = "cranelift")]
mod cranelift_backend;
#[cfg(feature = "llvm")]
mod llvm_backend;

#[derive(Parser, Debug)]
#[command(
    name = "neut",
    version,
    about = "Neutronium.js — fast modular JavaScript runtime on Rust"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// JavaScript file to run (shorthand for `neut run <file>`).
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run a JavaScript file.
    Run { file: PathBuf },
    /// Start an interactive REPL.
    Repl,
    /// Print build and backend information.
    Info,
    /// Ahead-of-time build (placeholder for the AOT pipeline).
    Build {
        #[arg(long, default_value = "jit")]
        backend: String,
    },
    /// Inspect a native FFI symbol in a shared library.
    Ffi { library: PathBuf, symbol: String },
    /// Load and initialize a native module through the stable Neutronium ABI.
    Load { library: PathBuf },
    /// Pack a directory into a `.nim` module container.
    Pack {
        /// Directory to pack.
        #[arg(value_name = "DIR")]
        dir: PathBuf,
        /// Output `.nim` path.
        #[arg(long, value_name = "FILE")]
        output: Option<PathBuf>,
        /// Module name written into the manifest (when no manifest.json exists).
        #[arg(long)]
        name: Option<String>,
        /// Module version written into the manifest.
        #[arg(long, default_value = "0.1.0")]
        version: String,
    },
    /// Unpack a `.nim` archive into a directory.
    Unpack {
        /// Path to the `.nim` archive.
        #[arg(value_name = "FILE")]
        file: PathBuf,
        /// Output directory.
        #[arg(long, value_name = "DIR")]
        out: PathBuf,
    },
    /// Print the manifest contained in a `.nim` archive.
    Manifest {
        /// Path to the `.nim` archive.
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
    /// JIT-compile and evaluate an integer expression with Cranelift.
    Jit {
        /// Expression, e.g. "(1+2)*3".
        expr: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let command = cli
        .command
        .or_else(|| cli.file.map(|file| Command::Run { file }));

    match command {
        Some(Command::Run { file }) => runtime::run_file(&file),
        Some(Command::Repl) => runtime::repl(),
        Some(Command::Info) => {
            println!(
                "Neutronium.js {}\nbackend: {}\nfeatures: FFI, C ABI, .nim modules, AOT, JIT, Cranelift, LLVM (optional)",
                env!("CARGO_PKG_VERSION"),
                runtime::backend_name()
            );
            Ok(())
        }
        Some(Command::Build { backend }) => {
            let kind: backend::BackendKind = backend.parse().map_err(anyhow::Error::msg)?;
            println!("AOT build requested with {kind} backend");
            if matches!(kind, backend::BackendKind::Llvm) && !cfg!(feature = "llvm") {
                println!("note: rebuild with --features llvm to activate the LLVM provider");
            }
            if matches!(kind, backend::BackendKind::Cranelift) && !cfg!(feature = "cranelift")
            {
                println!("note: rebuild with --features cranelift to activate the Cranelift provider");
            }
            Ok(())
        }
        Some(Command::Ffi { library, symbol }) => ffi::inspect(&library, &symbol),
        Some(Command::Load { library }) => ffi::load(&library),
        Some(Command::Pack {
            dir,
            output,
            name,
            version,
        }) => {
            let output = output.unwrap_or_else(|| {
                let stem = dir
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "module".to_string());
                PathBuf::from(format!("{stem}.nim"))
            });
            let manifest = nim::NeutroniumManifest::new(
                name.unwrap_or_else(|| {
                    dir.file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "module".to_string())
                }),
                version,
            );
            let archive = nim::pack_directory(&dir, &output, manifest)?;
            println!(
                "packed {} ({} entries) -> {}",
                archive.manifest.name,
                archive.entries.len(),
                output.display()
            );
            Ok(())
        }
        Some(Command::Unpack { file, out }) => {
            let archive = nim::load_nim(&file)
                .with_context(|| format!("loading {}", file.display()))?;
            nim::unpack_to(&archive, &out)?;
            println!(
                "unpacked {} ({} entries) -> {}",
                archive.manifest.name,
                archive.entries.len(),
                out.display()
            );
            Ok(())
        }
        Some(Command::Manifest { file }) => {
            let archive = nim::load_nim(&file)
                .with_context(|| format!("loading {}", file.display()))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&archive.manifest)
                    .context("serializing manifest")?
            );
            Ok(())
        }
        Some(Command::Jit { expr }) => {
            #[cfg(feature = "cranelift")]
            {
                let value = cranelift_backend::CraneliftJit::eval_jit(&expr)?;
                println!("{value}");
                Ok(())
            }
            #[cfg(not(feature = "cranelift"))]
            {
                let _ = expr;
                anyhow::bail!("Cranelift JIT is not enabled; rebuild with --features cranelift")
            }
        }
        None => {
            println!("neut <file.js> | neut repl | neut info | neut build --backend jit | neut pack <dir> | neut jit \"1+2\"");
            Ok(())
        }
    }
}
