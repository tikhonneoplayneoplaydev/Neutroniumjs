//! Execution backend abstraction. Backends are deliberately separate from the JS host.
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendKind { Jit, Cranelift, Llvm, Aot }
impl std::str::FromStr for BackendKind { type Err=String; fn from_str(s:&str)->Result<Self,Self::Err>{match s.to_ascii_lowercase().as_str(){"jit"=>Ok(Self::Jit),"aot"=>Ok(Self::Aot),"cranelift"=>Ok(Self::Cranelift),"llvm"=>Ok(Self::Llvm), _=>Err(format!("unknown backend {s:?}; expected jit, aot, cranelift or llvm"))}}}
impl fmt::Display for BackendKind { fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result { write!(f,"{}",match self{Self::Jit=>"jit",Self::Aot=>"aot",Self::Cranelift=>"cranelift",Self::Llvm=>"llvm"}) } }

pub trait CompilerBackend { fn name(&self)->&'static str; fn compile(&self, source:&str)->Result<CompiledModule,String>; }
#[derive(Debug)] pub struct CompiledModule { pub backend:BackendKind, pub code:Vec<u8> }
pub struct BaselineJit;
impl CompilerBackend for BaselineJit { fn name(&self)->&'static str{"baseline-jit"} fn compile(&self,source:&str)->Result<CompiledModule,String>{Ok(CompiledModule{backend:BackendKind::Jit,code:source.as_bytes().to_vec()})} }
pub fn selected()->BackendKind { if cfg!(feature="llvm"){BackendKind::Llvm}else if cfg!(feature="cranelift"){BackendKind::Cranelift}else{BackendKind::Jit} }
#[cfg(test)] mod tests { use super::*; #[test] fn jit_compiles_source(){assert!(!BaselineJit.compile("1+1").unwrap().code.is_empty())} }
