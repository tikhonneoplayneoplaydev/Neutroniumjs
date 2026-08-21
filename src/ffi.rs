//! Native extension boundary.
//!
//! All `unsafe` code is isolated here: pointers are borrowed for the duration
//! of a callback and no native allocation is owned by Rust. The on-the-wire ABI
//! is described in `native/neutronium.h` and is intentionally C-only.

use anyhow::{Context as _, Result};
use std::path::Path;

/// Stable ABI v1. Must match `NeutApi` in `native/neutronium.h`.
#[repr(C)]
struct NeutApi {
    version: u32,
    log: Option<unsafe extern "C" fn(*const u8, u32)>,
}

/// `NeutApi::log` implementation. The pointer is only dereferenced for `len`
/// bytes while the call is in flight.
unsafe extern "C" fn log(ptr: *const u8, len: u32) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: contract with the native module — it passes a valid pointer and
    // length for the duration of the call.
    let bytes = std::slice::from_raw_parts(ptr, len as usize);
    eprintln!("[native] {}", String::from_utf8_lossy(bytes));
}

/// Check that `symbol` exists in `library` without initializing it.
pub fn inspect(path: &Path, symbol: &str) -> Result<()> {
    // SAFETY: `libloading` loads the library for the lifetime of `lib`. We only
    // probe for a symbol and drop both immediately; no function is called.
    unsafe {
        let lib = libloading::Library::new(path)
            .with_context(|| format!("cannot load {}", path.display()))?;
        let _: libloading::Symbol<unsafe extern "C" fn()> = lib
            .get(symbol.as_bytes())
            .with_context(|| format!("symbol {symbol:?} not found"))?;
        println!(
            "FFI symbol {symbol} is available in {}",
            path.display()
        );
    }
    Ok(())
}

/// Load a native module and call its `neut_init` entry point with the stable
/// Neutronium host API.
pub fn load(path: &Path) -> Result<()> {
    // SAFETY: the library is kept alive for the duration of the `neut_init`
    // call. The ABI contract requires the module to only borrow the `NeutApi`
    // pointer (and any pointers it receives through callbacks) for the call.
    unsafe {
        let lib = libloading::Library::new(path)
            .with_context(|| format!("cannot load {}", path.display()))?;
        let init: libloading::Symbol<unsafe extern "C" fn(*const NeutApi) -> i32> = lib
            .get(b"neut_init")
            .context("native module must export neut_init")?;

        let api = NeutApi {
            version: 1,
            log: Some(log),
        };
        let code = init(&api);
        if code != 0 {
            anyhow::bail!("native module returned error code {code}");
        }
        println!("loaded native module {}", path.display());
    }
    Ok(())
}
