//! Execution backend abstraction.
//!
//! Backends are deliberately separate from the JavaScript host (Boa). The host
//! asks a backend to compile source; it never links against a specific code
//! generator.
//!
//! * [`crate::cranelift_backend::CraneliftJit`] is a real, in-process
//!   executable JIT that emits native code and can invoke it directly. It is
//!   wired up by default.
//! * LLVM lives entirely behind the `llvm` feature flag and is isolated in its
//!   own module ([`crate::llvm_backend`]). Nothing else in the crate depends on
//!   it.

use std::fmt;

#[cfg(feature = "cranelift")]
use crate::cranelift_backend::CraneliftJit;
#[cfg(feature = "llvm")]
use crate::llvm_backend::LlvmBackend;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendKind {
    Jit,
    Cranelift,
    Llvm,
    Aot,
}

impl std::str::FromStr for BackendKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "jit" => Ok(Self::Jit),
            "aot" => Ok(Self::Aot),
            "cranelift" => Ok(Self::Cranelift),
            "llvm" => Ok(Self::Llvm),
            _ => Err(format!(
                "unknown backend {s:?}; expected jit, aot, cranelift or llvm"
            )),
        }
    }
}

impl fmt::Display for BackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Jit => "jit",
            Self::Aot => "aot",
            Self::Cranelift => "cranelift",
            Self::Llvm => "llvm",
        })
    }
}

#[derive(Debug)]
pub struct CompiledModule {
    pub backend: BackendKind,
    /// Native code / object bytes (for JIT backends) or the original source
    /// bytes (for the baseline interpreter).
    pub code: Vec<u8>,
}

pub trait CompilerBackend {
    fn kind(&self) -> BackendKind;
    fn name(&self) -> &'static str;
    fn compile(&self, source: &str) -> Result<CompiledModule, String>;
}

/// A trivial baseline "JIT": it just stores the source and reports itself as a
/// JIT. It exists so there is always a backend available and feature flags can
/// toggle the heavy code generators independently.
pub struct BaselineJit;

impl CompilerBackend for BaselineJit {
    fn kind(&self) -> BackendKind {
        BackendKind::Jit
    }

    fn name(&self) -> &'static str {
        "baseline-jit"
    }

    fn compile(&self, source: &str) -> Result<CompiledModule, String> {
        Ok(CompiledModule {
            backend: BackendKind::Jit,
            code: source.as_bytes().to_vec(),
        })
    }
}

/// Which backend is selected at compile time via Cargo features.
pub fn selected() -> BackendKind {
    if cfg!(feature = "llvm") {
        BackendKind::Llvm
    } else if cfg!(feature = "cranelift") {
        BackendKind::Cranelift
    } else {
        BackendKind::Jit
    }
}

/// Construct the default (feature-selected) backend.
pub fn default_backend() -> Box<dyn CompilerBackend> {
    if cfg!(feature = "llvm") {
        Box::new(LlvmBackend)
    } else if cfg!(feature = "cranelift") {
        Box::new(CraneliftJit::new())
    } else {
        Box::new(BaselineJit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jit_compiles_source() {
        assert!(!BaselineJit.compile("1+1").unwrap().code.is_empty());
    }

    #[test]
    fn backend_kind_roundtrips() {
        for k in [
            BackendKind::Jit,
            BackendKind::Aot,
            BackendKind::Cranelift,
            BackendKind::Llvm,
        ] {
            let parsed: BackendKind = k.to_string().parse().unwrap();
            assert_eq!(parsed, k);
        }
        assert!("v8".parse::<BackendKind>().is_err());
    }
}
