use super::compile_error;
use proc_macro::{Span, TokenStream, TokenTree};
use pyo3::{Bound, IntoPyObject, PyErr, PyResult, PyTypeInfo, Python, prelude::*, types::PyTraceback};

/// Format a nice error message for a python compilation error.
pub fn compile_error_msg(py: Python, error: PyErr, tokens: TokenStream) -> TokenStream {
	let value = (&error).into_pyobject(py).unwrap();

	if value.is_none() {
		return compile_error(None, &error.get_type(py).name().unwrap());
	}

	if let Ok(true) = error.matches(py, pyo3::exceptions::PySyntaxError::type_object(py)) {
		let line: Option<usize> = value.getattr("lineno").ok().and_then(|x| x.extract().ok());
		let msg: Option<String> = value.getattr("msg").ok().and_then(|x| x.extract().ok());
		if let (Some(line), Some(msg)) = (line, msg) {
			if let Some(spans) = spans_for_line(tokens.clone(), line) {
				return compile_error(Some(spans), &msg);
			}
		}
	}

	if let Some(tb) = &error.traceback(py) {
		if let Ok((file, line)) = get_traceback_info(tb) {
			if file == Span::call_site().file() {
				if let Ok(msg) = value.str() {
					if let Some(spans) = spans_for_line(tokens, line) {
						return compile_error(Some(spans), &msg);
					}
				}
			}
		}
	}

	compile_error(None, &value.str().unwrap())
}

fn get_traceback_info(tb: &Bound<'_, PyTraceback>) -> PyResult<(String, usize)> {
	let frame = tb.getattr("tb_frame")?;
	let code = frame.getattr("f_code")?;
	let file: String = code.getattr("co_filename")?.extract()?;
	let line: usize = frame.getattr("f_lineno")?.extract()?;
	Ok((file, line))
}

fn for_all_spans(input: TokenStream, f: &mut impl FnMut(Span)) {
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
fn spans_for_line(input: TokenStream, line: usize) -> Option<(Span, Span)> {
	let mut spans = None;
	for_all_spans(input, &mut |span| {
		if span.start().line() == line {
			spans.get_or_insert((span, span)).1 = span;
		}
	});
	spans
}
