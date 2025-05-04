//! Execute Python code at compile time to generate Rust code.
//!
//! # Example
//!
//! ```
//! use ct_python::ct_python;
//!
//! static SIN_2: f64 = ct_python! {
//!     from math import sin
//!     print(sin(2))
//! };
//!
//! ct_python! {
//!     print("type num = f64;")
//! }
//!
//! fn main() {
//!     assert_eq!(num::sin(2.0), SIN_2);
//! }
//! ```
//!
//! # How to use
//!
//! Use the `ct_python!{..}` macro to generate Rust code from an embedded
//! Python script.
//! The output of the script (`print()` and anything else through `sys.stdout`)
//! is captured, and will be parsed and injected as Rust code.
//!
//! ## Python Errors
//!
//! Any syntax errors and runtime exceptions from the Python code will be
//! reported by the Rust compiler as compiler errors.
//!
//! ## Syntax issues
//!
//! Since the Rust tokenizer will tokenize the Python code, some valid Python
//! code is rejected. See [the `inline-python` documentation][1] for details.
//!
//! [1]: https://docs.rs/inline-python/#syntax-issues

#![feature(proc_macro_span)]

use proc_macro::{Span, TokenStream};
use pyo3::{PyObject, PyResult, Python, prelude::*};
use std::{ffi::CString, ptr::null_mut, str::FromStr};

mod shared;
use shared::*;

/// A block of compile-time executed Rust code generating Python code.
///
/// See [the crate's module level documentation](index.html) for examples.
#[proc_macro]
pub fn ct_python(input: TokenStream) -> TokenStream {
    ct_python_impl(input).unwrap_or_else(|e| e)
}

fn ct_python_impl(input: TokenStream) -> Result<TokenStream, TokenStream> {
    let python = CString::new(python_from_macro(input.clone(), None)?).unwrap();
    let filename = CString::new(Span::call_site().file()).unwrap();

    Python::with_gil(|py| {
        let code = compile_python(py, &python, &filename, input.clone())?;
        let output = run_and_capture(py, code)
            .map_err(|err| python_error_to_compile_error(py, err, input))?;
        TokenStream::from_str(&output)
            .map_err(|_| compile_error(None, "produced invalid Rust code"))
    })
}

fn run_and_capture(py: Python, code: PyObject) -> PyResult<String> {
    #[cfg(unix)]
    let _ = ensure_libpython_symbols_loaded(py);

    let globals = py.import("__main__")?.dict().copy()?;

    let sys = py.import("sys")?;
    let stdout = py.import("io")?.getattr("StringIO")?.call0()?;
    let original_stdout = sys.dict().get_item("stdout")?;
    sys.dict().set_item("stdout", &stdout)?;

    let result = unsafe {
        let ptr = pyo3::ffi::PyEval_EvalCode(code.as_ptr(), globals.as_ptr(), null_mut());
        PyObject::from_owned_ptr_or_err(py, ptr)
    };

    sys.dict().set_item("stdout", original_stdout)?;

    result?;

    stdout.call_method0("getvalue")?.extract()
}

#[cfg(unix)]
fn ensure_libpython_symbols_loaded(py: Python) -> PyResult<()> {
    // On Unix, Rustc loads proc-macro crates with RTLD_LOCAL, which (at least
    // on Linux) means all their dependencies (in our case: libpython) don't
    // get their symbols made available globally either. This means that
    // loading modules (e.g. `import math`) will fail, as those modules refer
    // back to symbols of libpython.
    //
    // This function tries to (re)load the right version of libpython, but this
    // time with RTLD_GLOBAL enabled.
    let sysconfig = py.import("sysconfig")?;
    let libdir: String = sysconfig
        .getattr("get_config_var")?
        .call1(("LIBDIR",))?
        .extract()?;
    let so_name: String = sysconfig
        .getattr("get_config_var")?
        .call1(("INSTSONAME",))?
        .extract()?;
    let path = CString::new(format!("{libdir}/{so_name}")).unwrap();
    unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) };
    Ok(())
}
