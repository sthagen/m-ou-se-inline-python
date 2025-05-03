//! Helper crate for `inline-python` and `ct-python`.

#![feature(proc_macro_span)]

extern crate proc_macro;

use self::embed_python::EmbedPython;
use proc_macro::{Span, TokenStream as TokenStream1};
use proc_macro2::{Literal, TokenStream};
use pyo3::{ffi, Py, PyObject, Python};
use quote::quote;
use std::ffi::CString;

mod embed_python;
mod error;
mod run;

fn python_impl(input: TokenStream) -> Result<TokenStream, TokenStream> {
	let tokens = input.clone();

	let filename = Span::call_site().file();

	let mut x = EmbedPython::new();

	x.add(input)?;

	let EmbedPython { python, variables, .. } = x;

	let python = CString::new(python).unwrap();
	let filename = CString::new(filename).unwrap();

	let bytecode = unsafe {
		let result: Result<Literal, TokenStream> = Python::with_gil(|py| {
			let code = PyObject::from_owned_ptr_or_err(py, ffi::Py_CompileString(python.as_ptr(), filename.as_ptr(), ffi::Py_file_input))
				.map_err(|err| error::compile_error_msg(py, err, tokens))?;

			Ok(Literal::byte_string(
				Py::from_owned_ptr_or_err(py, ffi::PyMarshal_WriteObjectToString(code.as_ptr(), pyo3::marshal::VERSION))
					.map_err(|_e| quote!(compile_error! {"failed to generate python bytecode"}))?
					.as_bytes(py),
			))
		});
		result?
	};

	let varname = variables.keys();
	let var = variables.values();

	Ok(quote! {
		::inline_python::FromInlinePython::from_python_macro(
			#bytecode,
			|globals| {
				#(
					::inline_python::pyo3::prelude::PyDictMethods::set_item(
						globals,
						#varname,
						#var
					).expect("Unable to convert variable to Python");
				)*
			},
		)
	})
}

fn ct_python_impl(input: TokenStream) -> Result<TokenStream, TokenStream> {
	let tokens = input.clone();

	let filename = Span::call_site().file();

	let mut x = EmbedPython::new();

	x.compile_time = true;

	x.add(input)?;

	let EmbedPython { python, .. } = x;

	let python = CString::new(python).unwrap();
	let filename = CString::new(filename).unwrap();

	Python::with_gil(|py| {
		let code = unsafe {
			PyObject::from_owned_ptr_or_err(py, ffi::Py_CompileString(python.as_ptr(), filename.as_ptr(), ffi::Py_file_input))
				.map_err(|err| error::compile_error_msg(py, err, tokens.clone()))?
		};

		run::run_ct_python(py, code, tokens)
	})
}

#[doc(hidden)]
#[proc_macro]
pub fn python(input: TokenStream1) -> TokenStream1 {
	TokenStream1::from(match python_impl(TokenStream::from(input)) {
		Ok(tokens) => tokens,
		Err(tokens) => tokens,
	})
}

#[doc(hidden)]
#[proc_macro]
pub fn ct_python(input: TokenStream1) -> TokenStream1 {
	TokenStream1::from(match ct_python_impl(TokenStream::from(input)) {
		Ok(tokens) => tokens,
		Err(tokens) => tokens,
	})
}
