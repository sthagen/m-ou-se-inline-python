//! Helper crate for `inline-python`.

#![feature(proc_macro_span)]

extern crate proc_macro;

use proc_macro::{Literal, Span, TokenStream, TokenTree};
use pyo3::{Py, Python};
use std::{
    collections::BTreeMap,
    ffi::{CStr, CString},
};

mod shared;
use shared::*;

#[doc(hidden)]
#[proc_macro]
pub fn python(input: TokenStream) -> TokenStream {
    python_impl(input).unwrap_or_else(|e| e)
}

#[rustfmt::skip]
fn python_impl(input: TokenStream) -> Result<TokenStream, TokenStream> {
    let mut variables = BTreeMap::new();
    let python = CString::new(python_from_macro(input.clone(), Some(&mut variables))?).unwrap();
    let filename = CString::new(Span::call_site().file()).unwrap();
    let bytecode = compile_to_bytecode(&python, &filename, input)?;
    Ok(TokenStream::from_iter([
        punct(':'), punct(':'), ident("inline_python"),
        punct(':'), punct(':'), ident("_python_block"),
        punct('!'),
        braces(
            [TokenTree::Literal(bytecode)].into_iter()
            .chain(variables.into_iter().map(|(_, value)| TokenTree::Ident(value)))
        ),
    ]))
}

fn compile_to_bytecode(
    python: &CStr,
    filename: &CStr,
    tokens: TokenStream,
) -> Result<Literal, TokenStream> {
    Python::with_gil(|py| {
        let compiled = compile_python(py, python, filename, tokens)?;
        let bytes = unsafe {
            let ptr =
                pyo3::ffi::PyMarshal_WriteObjectToString(compiled.as_ptr(), pyo3::marshal::VERSION);
            Py::from_owned_ptr(py, ptr)
        };
        Ok(Literal::byte_string(bytes.as_bytes(py)))
    })
}
