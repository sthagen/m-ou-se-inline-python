//! Helper crate for `inline-python` and `ct-python`.

#![feature(proc_macro_span)]

extern crate proc_macro;

use self::embed_python::EmbedPython;
use proc_macro::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};
use pyo3::{Py, PyObject, Python, ffi};
use std::{ffi::CString, fmt::Display};

mod embed_python;
mod error;
mod run;

#[rustfmt::skip]
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
			let code = PyObject::from_owned_ptr_or_err(py,
				ffi::Py_CompileString(python.as_ptr(), filename.as_ptr(), ffi::Py_file_input)
			).map_err(|err| error::compile_error_msg(py, err, tokens))?;
			Ok(Literal::byte_string(Py::from_owned_ptr_or_err(py,
				ffi::PyMarshal_WriteObjectToString(code.as_ptr(), pyo3::marshal::VERSION)
			).map_err(|_| compile_error(None, "failed to generate bytecode"))?.as_bytes(py)))
		});
		result?
	};

	Ok(TokenStream::from_iter([
		punct(':'), punct(':'), ident("inline_python"),
		punct(':'), punct(':'), ident("FromInlinePython"),
		punct(':'), punct(':'), ident("from_python_macro"),
		parens([
			TokenTree::Literal(bytecode), punct(','),
			punct('|'), ident("globals"), punct('|'),
			braces(variables.into_iter().flat_map(|(key, value)| [
				punct(':'), punct(':'), ident("inline_python"),
				punct(':'), punct(':'), ident("pyo3"),
				punct(':'), punct(':'), ident("prelude"),
				punct(':'), punct(':'), ident("PyDictMethods"),
				punct(':'), punct(':'), ident("set_item"),
				parens([
					ident("globals"), punct(','),
					string(&key), punct(','),
					TokenTree::Ident(value)
				]),
				punct('.'), ident("expect"), parens([string("python")]),
				punct(';'),
			]))
		]),
	]))
}

/// Create a compile_error!{} using two spans that mark the start and end of the error.
#[rustfmt::skip]
fn compile_error(spans: Option<(Span, Span)>, error: &(impl Display + ?Sized)) -> TokenStream {
	let mut tokens = [
		punct(':'), punct(':'), ident("core"),
		punct(':'), punct(':'), ident("compile_error"),
		punct('!'), braces([string(&format!("python: {error}"))]),
	];
	if let Some((span1, span2)) = spans {
		for (i, t) in tokens.iter_mut().enumerate() {
			t.set_span(if i < 6 { span1 } else { span2 });
		}
	}
	TokenStream::from_iter(tokens)
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
pub fn python(input: TokenStream) -> TokenStream {
	match python_impl(input) {
		Ok(tokens) => tokens,
		Err(tokens) => tokens,
	}
}

#[doc(hidden)]
#[proc_macro]
pub fn ct_python(input: TokenStream) -> TokenStream {
	match ct_python_impl(input) {
		Ok(tokens) => tokens,
		Err(tokens) => tokens,
	}
}

fn punct(p: char) -> TokenTree {
	TokenTree::Punct(Punct::new(p, Spacing::Joint))
}

fn ident(s: &str) -> TokenTree {
	TokenTree::Ident(Ident::new(s, Span::call_site()))
}

fn parens(t: impl IntoIterator<Item = TokenTree>) -> TokenTree {
	TokenTree::Group(Group::new(Delimiter::Parenthesis, TokenStream::from_iter(t)))
}

fn braces(t: impl IntoIterator<Item = TokenTree>) -> TokenTree {
	TokenTree::Group(Group::new(Delimiter::Brace, TokenStream::from_iter(t)))
}

fn string(s: &str) -> TokenTree {
	TokenTree::Literal(Literal::string(s))
}
