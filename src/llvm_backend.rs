//! Isolated LLVM backend.
//!
//! LLVM is deliberately kept in its own module and only compiled when the
//! `llvm` Cargo feature is enabled. The rest of the crate (including the JS
//! host and the Cranelift JIT) never links against LLVM and does not depend on
//! `inkwell` types.
//!
//! When the feature is enabled this module uses [`inkwell`] to build an LLVM
//! module for an `add(i64, i64) -> i64` function and runs it through LLVM's
//! MCJIT. This demonstrates a fully working LLVM-provided execution path
//! without leaking LLVM into the rest of the runtime.
//!
//! > Building this backend requires a system LLVM (e.g. `llvm-17-dev`) and must
//! > be opted into explicitly:
//! >
//! > ```sh
//! > cargo check --no-default-features --features "llvm/llvm-17"
//! > ```

#![cfg(feature = "llvm")]

use crate::backend::{BackendKind, CompiledModule, CompilerBackend};
use inkwell::context::Context;
use inkwell::execution_engine::JitFunction;
use inkwell::targets::{InitializationConfig, Target};
use inkwell::OptimizationLevel;
use std::sync::Once;

static INIT_LLVM_NATIVE: Once = Once::new();

/// One-time initialization of the native LLVM target. MCJIT requires this or
/// function lookups can spuriously fail.
fn initialize_native() {
    INIT_LLVM_NATIVE.call_once(|| {
        Target::initialize_native(&InitializationConfig::default())
            .expect("failed to initialize LLVM native target");
    });
}

pub struct LlvmBackend;

impl LlvmBackend {
    /// Compile and JIT-execute an `add(a, b)` function through LLVM. Returns
    /// the result. Kept here so the LLVM dependency is fully encapsulated.
    pub fn jit_add(a: i64, b: i64) -> Result<i64, String> {
        initialize_native();

        // SAFETY: All LLVM values created below are owned by `Context`, which
        // outlives the execution engine; `JitFunction` is called with the exact
        // declared signature.
        unsafe {
            let context = Context::create();
            let module = context.create_module("neutronium_llvm");
            let builder = context.create_builder();

            let i64_ty = context.i64_type();
            let fn_type = i64_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let function = module.add_function("neut_add", fn_type, None);
            let entry = context.append_basic_block(function, "entry");
            builder.position_at_end(entry);

            let x = function.get_nth_param(0).unwrap().into_int_value();
            let y = function.get_nth_param(1).unwrap().into_int_value();
            let sum = builder.build_int_add(x, y, "sum");
            builder.build_return(Some(&sum));

            let mut exec = module
                .create_jit_execution_engine(OptimizationLevel::Default)
                .map_err(|e| format!("failed to create LLVM JIT: {e}"))?;

            let add: JitFunction<unsafe extern "C" fn(i64, i64) -> i64> = exec
                .get_function("neut_add")
                .map_err(|e| format!("failed to look up neut_add: {e}"))?;
            Ok(add.call(a, b))
        }
    }
}

impl CompilerBackend for LlvmBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Llvm
    }

    fn name(&self) -> &'static str {
        "llvm-mcjit"
    }

    fn compile(&self, source: &str) -> Result<CompiledModule, String> {
        // The LLVM backend currently only understands the built-in `add`
        // expression surface. Validate that LLVM is functional on this host by
        // JIT-compiling and running `add(0, 0)`, then tag the artifact so
        // callers can tell this path was taken.
        let _ = Self::jit_add(0, 0)?;
        let mut code = b"LLVM".to_vec();
        code.extend_from_slice(source.as_bytes());
        Ok(CompiledModule {
            backend: BackendKind::Llvm,
            code,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn llvm_jit_add_works() {
        assert_eq!(LlvmBackend::jit_add(2, 3).unwrap(), 5);
        assert_eq!(LlvmBackend::jit_add(-40, 42).unwrap(), 2);
    }
}
