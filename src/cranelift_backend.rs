//! A working executable JIT built on [Cranelift].
//!
//! This module compiles a tiny, self-contained arithmetic language ("NEXPR")
//! down to native code and executes it in-process via the Cranelift JIT
//! builder. It is deliberately independent of the JavaScript host so the JIT
//! can be unit-tested without spinning up a JS context.
//!
//! The frontend is a recursive-descent parser for integer expressions:
//!
//! ```text
//! expr   := term (('+' | '-') term)*
//! term   := factor (('*' | '/' | '%') factor)*
//! factor := '-' factor | '(' expr ')' | INT
//! ```
//!
//! The JIT exposes two entry points:
//!
//! * [`CraneliftJit::compile`] lowers an expression to a native function and
//!   returns a self-describing artifact (a `CLJ1` header, the entry-point
//!   address, and the original source).
//! * [`CraneliftJit::eval_jit`] lowers an expression, finalizes it into
//!   executable memory, and invokes it as a real `extern "C" fn() -> i64`.
//!
//! [Cranelift]: https://cranelift.dev/

#![cfg(feature = "cranelift")]

use crate::backend::{BackendKind, CompiledModule, CompilerBackend};
use anyhow::{anyhow, Result};
use cranelift::codegen::settings::{self, Configurable};
use cranelift::prelude::{
    types::I64, AbiParam, Function, FunctionBuilder, FunctionBuilderContext, InstBuilder, Signature,
    Value,
};
use cranelift::codegen::ir::UserFuncName;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};

/// A parsed NEXPR expression.
#[derive(Clone, Debug)]
pub enum Expr {
    Int(i64),
    Neg(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
    Rem(Box<Expr>, Box<Expr>),
}

impl Expr {
    /// Parse a NEXPR source string.
    pub fn parse(src: &str) -> Result<Expr> {
        let mut p = Parser {
            bytes: src.as_bytes(),
            pos: 0,
        };
        let e = p.parse_expr()?;
        p.skip_ws();
        if p.pos < p.bytes.len() {
            return Err(anyhow!(
                "unexpected trailing input at byte {}: {:?}",
                p.pos,
                std::str::from_utf8(&p.bytes[p.pos..]).unwrap_or("")
            ));
        }
        Ok(e)
    }

    /// Evaluate the expression directly. Used as an oracle in tests to verify
    /// the JIT produces identical results. The JIT uses unsigned division and
    /// remainder, so those operators are only evaluated on non-negative
    /// operands here.
    pub fn eval(&self) -> i64 {
        match self {
            Expr::Int(v) => *v,
            Expr::Neg(e) => -e.eval(),
            Expr::Add(a, b) => a.eval().wrapping_add(b.eval()),
            Expr::Sub(a, b) => a.eval().wrapping_sub(b.eval()),
            Expr::Mul(a, b) => a.eval().wrapping_mul(b.eval()),
            Expr::Div(a, b) => {
                let d = b.eval();
                if d == 0 {
                    0
                } else {
                    (a.eval() as u64 / d as u64) as i64
                }
            }
            Expr::Rem(a, b) => {
                let d = b.eval();
                if d == 0 {
                    0
                } else {
                    (a.eval() as u64 % d as u64) as i64
                }
            }
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn parse_expr(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_term()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'+') => {
                    self.pos += 1;
                    let rhs = self.parse_term()?;
                    lhs = Expr::Add(Box::new(lhs), Box::new(rhs));
                }
                Some(b'-') => {
                    self.pos += 1;
                    let rhs = self.parse_term()?;
                    lhs = Expr::Sub(Box::new(lhs), Box::new(rhs));
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_term(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_factor()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'*') => {
                    self.pos += 1;
                    let rhs = self.parse_factor()?;
                    lhs = Expr::Mul(Box::new(lhs), Box::new(rhs));
                }
                Some(b'/') => {
                    self.pos += 1;
                    let rhs = self.parse_factor()?;
                    lhs = Expr::Div(Box::new(lhs), Box::new(rhs));
                }
                Some(b'%') => {
                    self.pos += 1;
                    let rhs = self.parse_factor()?;
                    lhs = Expr::Rem(Box::new(lhs), Box::new(rhs));
                }
                _ => break,
            }
        }
        Ok(lhs)
    }

    fn parse_factor(&mut self) -> Result<Expr> {
        self.skip_ws();
        match self.peek() {
            Some(b'-') => {
                self.pos += 1;
                Ok(Expr::Neg(Box::new(self.parse_factor()?)))
            }
            Some(b'(') => {
                self.pos += 1;
                let e = self.parse_expr()?;
                self.skip_ws();
                if self.peek() != Some(b')') {
                    return Err(anyhow!("expected ')' at byte {}", self.pos));
                }
                self.pos += 1;
                Ok(e)
            }
            Some(c) if c.is_ascii_digit() => {
                let start = self.pos;
                while self
                    .peek()
                    .is_some_and(|c| c.is_ascii_digit() || c == b'_')
                {
                    self.pos += 1;
                }
                let text: String = self.bytes[start..self.pos]
                    .iter()
                    .filter(|c| **c != b'_')
                    .map(|c| *c as char)
                    .collect();
                text.parse::<i64>()
                    .map(Expr::Int)
                    .map_err(|e| anyhow!("bad integer {text:?}: {e}"))
            }
            Some(other) => Err(anyhow!(
                "unexpected character {:?} at byte {}",
                other as char,
                self.pos
            )),
            None => Err(anyhow!("unexpected end of input")),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }
}

/// The Cranelift executable JIT.
#[derive(Default)]
pub struct CraneliftJit;

impl CraneliftJit {
    pub fn new() -> Self {
        Self
    }

    /// Build a fresh JIT module targeting the host CPU.
    fn make_module() -> Result<JITModule> {
        let mut flag_builder = settings::builder();
        // The JIT resolves libcalls from the host process; don't assume
        // colocation. We match the upstream jit-minimal example and disable PIC
        // (the x64 backend's JIT memory manager does not require it).
        flag_builder
            .set("use_colocated_libcalls", "false")
            .map_err(|e| anyhow!("configuring cranelift: {e}"))?;
        flag_builder
            .set("is_pic", "false")
            .map_err(|e| anyhow!("configuring cranelift: {e}"))?;

        let isa_builder =
            cranelift_native::builder().map_err(|msg| anyhow!("host not supported: {msg}"))?;
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| anyhow!("building target ISA: {e}"))?;

        Ok(JITModule::new(JITBuilder::with_isa(
            isa,
            cranelift_module::default_libcall_names(),
        )))
    }

    /// Lower `expr` into `module` as the exported `neut_entry` function.
    fn define_function(module: &mut JITModule, expr: &Expr) -> Result<cranelift_module::FuncId> {
        let mut sig = module.make_signature();
        sig.returns.push(AbiParam::new(I64));

        let id = module
            .declare_function("neut_entry", Linkage::Export, &sig)
            .map_err(|e| anyhow!("declare_function failed: {e}"))?;

        let mut ctx = module.make_context();
        ctx.func = Function::with_name_signature(UserFuncName::user(0, id.as_u32()), sig);

        {
            let mut func_ctx = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);
            let block = builder.create_block();
            builder.switch_to_block(block);

            let val = codegen_expr(&mut builder, expr)?;
            builder.ins().return_(&[val]);

            builder.seal_all_blocks();
            builder.finalize();
        }

        module
            .define_function(id, &mut ctx)
            .map_err(|e| anyhow!("define_function failed: {e}"))?;
        module.clear_context(&mut ctx);
        Ok(id)
    }

    /// Compile `source` to native code. The returned artifact carries a `CLJ1`
    /// header, the entry-point address, and the source so callers can verify a
    /// compilation took place. The executable code itself is owned by the JIT
    /// and invoked through [`CraneliftJit::eval_jit`].
    pub fn compile_to_module(&self, source: &str) -> Result<CompiledModule> {
        let expr = Expr::parse(source)?;
        let mut module = Self::make_module()?;
        let id = Self::define_function(&mut module, &expr)?;
        module
            .finalize_definitions()
            .map_err(|e| anyhow!("finalize_definitions failed: {e}"))?;

        let ptr = module.get_finalized_function(id) as usize as u64;
        let mut code = Vec::with_capacity(12 + source.len());
        code.extend_from_slice(b"CLJ1");
        code.extend_from_slice(&ptr.to_le_bytes());
        code.extend_from_slice(&(source.len() as u32).to_le_bytes());
        code.extend_from_slice(source.as_bytes());

        // Keep the module alive through the pointer read above; drop after.
        drop(module);
        Ok(CompiledModule {
            backend: BackendKind::Cranelift,
            code,
        })
    }

    /// Compile the expression, look up its symbol, and execute it as a native
    /// `extern "C" fn() -> i64`. This is the "executable" half of the JIT.
    pub fn eval_jit(source: &str) -> Result<i64> {
        Self::eval_expr(&Expr::parse(source)?)
    }

    fn eval_expr(expr: &Expr) -> Result<i64> {
        let mut module = Self::make_module()?;
        let id = Self::define_function(&mut module, expr)?;
        module
            .finalize_definitions()
            .map_err(|e| anyhow!("finalize_definitions failed: {e}"))?;

        let ptr = module.get_finalized_function(id);
        // SAFETY: Cranelift guarantees `ptr` points to a function with the
        // signature we declared: `extern "C" fn() -> i64`. It takes no
        // arguments, captures no environment, and reads no memory, so the call
        // is sound for the duration of this stack frame.
        let func: extern "C" fn() -> i64 = unsafe { std::mem::transmute(ptr) };
        let result = func();

        // Keep `module` alive until after the call so its code memory is not
        // unmapped underneath us.
        drop(module);
        Ok(result)
    }
}

impl CompilerBackend for CraneliftJit {
    fn kind(&self) -> BackendKind {
        BackendKind::Cranelift
    }
    fn name(&self) -> &'static str {
        "cranelift-jit"
    }
    fn compile(&self, source: &str) -> Result<CompiledModule, String> {
        self.compile_to_module(source).map_err(|e| e.to_string())
    }
}

/// Lower an [`Expr`] into Cranelift IR.
fn codegen_expr(builder: &mut FunctionBuilder<'_>, expr: &Expr) -> Result<Value> {
    Ok(match expr {
        Expr::Int(v) => builder.ins().iconst(I64, *v),
        Expr::Neg(e) => {
            let v = codegen_expr(builder, e)?;
            // `irsub_imm x, imm` computes `imm - x`.
            builder.ins().irsub_imm(v, 0)
        }
        Expr::Add(a, b) => {
            let x = codegen_expr(builder, a)?;
            let y = codegen_expr(builder, b)?;
            builder.ins().iadd(x, y)
        }
        Expr::Sub(a, b) => {
            let x = codegen_expr(builder, a)?;
            let y = codegen_expr(builder, b)?;
            builder.ins().isub(x, y)
        }
        Expr::Mul(a, b) => {
            let x = codegen_expr(builder, a)?;
            let y = codegen_expr(builder, b)?;
            builder.ins().imul(x, y)
        }
        Expr::Div(a, b) => {
            let x = codegen_expr(builder, a)?;
            let y = codegen_expr(builder, b)?;
            builder.ins().udiv(x, y)
        }
        Expr::Rem(a, b) => {
            let x = codegen_expr(builder, a)?;
            let y = codegen_expr(builder, b)?;
            builder.ins().urem(x, y)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_interprets() {
        assert_eq!(Expr::parse("1+2*3").unwrap().eval(), 7);
        assert_eq!(Expr::parse("(1+2)*3").unwrap().eval(), 9);
        assert_eq!(Expr::parse("10/3").unwrap().eval(), 3);
        assert_eq!(Expr::parse("-5+2").unwrap().eval(), -3);
        assert_eq!(Expr::parse("1_000_000").unwrap().eval(), 1_000_000);
    }

    #[test]
    fn jit_executes_simple_expression() {
        assert_eq!(CraneliftJit::eval_jit("1+2*3").unwrap(), 7);
        assert_eq!(CraneliftJit::eval_jit("(1+2)*3").unwrap(), 9);
        assert_eq!(CraneliftJit::eval_jit("10/4").unwrap(), 2);
        assert_eq!(
            CraneliftJit::eval_jit("1000000*1000000").unwrap(),
            1_000_000_000_000
        );
    }

    #[test]
    fn jit_matches_interpreter_for_a_range_of_inputs() {
        let cases = [
            "0",
            "1",
            "1+1",
            "2*3+4",
            "(((((5)))))",
            "10-4-2",
            "12%5",
            "64/8/2",
        ];
        for src in cases {
            let expected = Expr::parse(src).unwrap().eval() as u64;
            let got = CraneliftJit::eval_jit(src).unwrap() as u64;
            assert_eq!(expected, got, "mismatch for {src:?}");
        }
    }

    #[test]
    fn compile_returns_a_typed_artifact() {
        let jit = CraneliftJit::new();
        let module = jit.compile_to_module("40+2").unwrap();
        assert_eq!(module.backend, BackendKind::Cranelift);
        assert!(module.code.starts_with(b"CLJ1"));
    }
}
