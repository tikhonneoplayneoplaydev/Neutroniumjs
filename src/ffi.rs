//! Native extension boundary. All unsafe code is isolated here; pointers are borrowed
//! for the duration of the callback and no native allocation is owned by Rust.
use anyhow::{Context, Result};
use std::path::Path;

#[repr(C)] struct NeutApi { version: u32, log: Option<unsafe extern "C" fn(*const u8, u32)> }
unsafe extern "C" fn log(ptr:*const u8,len:u32) { if ptr.is_null(){return} let bytes=std::slice::from_raw_parts(ptr,len as usize); eprintln!("[native] {}",String::from_utf8_lossy(bytes)); }

pub fn inspect(path: &Path, symbol: &str) -> Result<()> { unsafe { let lib=libloading::Library::new(path).with_context(||format!("cannot load {}",path.display()))?; let _:libloading::Symbol<unsafe extern "C" fn()>=lib.get(symbol.as_bytes()).with_context(||format!("symbol {symbol:?} not found"))?; println!("FFI symbol {symbol} is available in {}",path.display()); } Ok(()) }

pub fn load(path:&Path)->Result<()> { unsafe { let lib=libloading::Library::new(path).with_context(||format!("cannot load {}",path.display()))?; let init:libloading::Symbol<unsafe extern "C" fn(*const NeutApi)->i32>=lib.get(b"neut_init").context("native module must export neut_init")?; let api=NeutApi{version:1,log:Some(log)}; let code=init(&api); if code!=0 { anyhow::bail!("native module returned error code {code}") } println!("loaded native module {}",path.display()); } Ok(()) }
