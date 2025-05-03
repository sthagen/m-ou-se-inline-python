use proc_macro::{TokenTree, Span, TokenStream as TokenStream1};
use proc_macro2::TokenStream;
use pyo3::{prelude::*, types::PyTraceback, Bound, IntoPyObject, PyErr, PyResult, PyTypeInfo, Python};
use quote::{quote, quote_spanned};

/// Format a nice error message for a python compilation error.
pub fn compile_error_msg(py: Python, error: PyErr, tokens: TokenStream) -> TokenStream {
	let value = (&error).into_pyobject(py).unwrap();

	if value.is_none() {
		let error = format!("python: {}", error.get_type(py).name().unwrap());
		return quote!(compile_error! {#error});
	}

	if let Ok(true) = error.matches(py, pyo3::exceptions::PySyntaxError::type_object(py)) {
		let line: Option<usize> = value.getattr("lineno").ok().and_then(|x| x.extract().ok());
		let msg: Option<String> = value.getattr("msg").ok().and_then(|x| x.extract().ok());
		if let (Some(line), Some(msg)) = (line, msg) {
			if let Some(spans) = spans_for_line(tokens.clone().into(), line) {
				return compile_error(spans, format!("python: {msg}"));
			}
		}
	}

	if let Some(tb) = &error.traceback(py) {
		if let Ok((file, line)) = get_traceback_info(tb) {
			if file == Span::call_site().file() {
				if let Ok(msg) = value.str() {
					if let Some(spans) = spans_for_line(tokens.into(), line) {
						return compile_error(spans, format!("python: {msg}"));
					}
				}
			}
		}
	}

	let error = format!("python: {}", value.str().unwrap());
	quote!(compile_error! {#error})
}

fn get_traceback_info(tb: &Bound<'_, PyTraceback>) -> PyResult<(String, usize)> {
	let frame = tb.getattr("tb_frame")?;
	let code = frame.getattr("f_code")?;
	let file: String = code.getattr("co_filename")?.extract()?;
	let line: usize = frame.getattr("f_lineno")?.extract()?;
	Ok((file, line))
}

fn for_all_spans(input: TokenStream1, f: &mut impl FnMut(Span)) {
	for token in input {
		match token {
			TokenTree::Group(group) => {
				f(group.span_open());
				for_all_spans(group.stream(), f);
				f(group.span_close());
			}
			_ => f(token.span()),
		}
	}
}

/// Get the first and last span for a specific line of input from a TokenStream.
fn spans_for_line(input: TokenStream1, line: usize) -> Option<(Span, Span)> {
	let mut spans = None;
	for_all_spans(input, &mut |span| {
		if span.start().line() == line {
			spans.get_or_insert((span, span)).1 = span;
		}
	});
	spans
}

/// Create a compile_error!{} using two spans that mark the start and end of the error.
fn compile_error(spans: (Span, Span), error: String) -> TokenStream {
	let path = quote_spanned!(spans.0.into() => ::core::compile_error);
	quote_spanned!(spans.1.into() => #path!{#error})
}
