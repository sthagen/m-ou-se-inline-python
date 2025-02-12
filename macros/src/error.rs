use proc_macro::Span;
use proc_macro2::TokenStream;
use pyo3::{prelude::*, types::PyTraceback, Bound, PyErr, PyResult, PyTypeInfo, Python, IntoPyObject};
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
			if let Some(span) = span_for_line(tokens.clone(), line) {
				let error = format!("python: {}", msg);
				return quote_spanned!(span.into() => compile_error!{#error});
			}
		}
	}

	if let Some(tb) = &error.traceback(py) {
		if let Ok((file, line)) = get_traceback_info(tb) {
			if file == Span::call_site().source_file().path().to_string_lossy() {
				if let Ok(msg) = value.str() {
					if let Some(span) = span_for_line(tokens, line) {
						let error = format!("python: {}", msg);
						return quote_spanned!(span.into() => compile_error!{#error});
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

/// Get a span for a specific line of input from a TokenStream.
fn span_for_line(input: TokenStream, line: usize) -> Option<Span> {
	let mut spans = input
		.into_iter()
		.map(|x| x.span().unwrap())
		.skip_while(|span| span.start().line() < line)
		.take_while(|span| span.start().line() == line);

	let mut result = spans.next()?;
	for span in spans {
		result = match result.join(span) {
			None => return Some(result),
			Some(span) => span,
		}
	}

	Some(result)
}
