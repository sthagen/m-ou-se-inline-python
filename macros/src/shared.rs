//! This file is shared between inline-python-macros and ct-python.

use proc_macro::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};
use pyo3::{
    Bound, IntoPyObject, PyErr, PyResult, PyTypeInfo, Python, exceptions::PyBaseException,
    prelude::*, types::PyTraceback,
};
use std::{
    collections::BTreeMap,
    ffi::CStr,
    fmt::{Display, Write},
};

/// Create a compile_error!{} using two spans that mark the start and end of the error.
#[rustfmt::skip]
pub(crate) fn compile_error(spans: Option<(Span, Span)>, error: &(impl Display + ?Sized)) -> TokenStream {
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

pub(crate) fn punct(p: char) -> TokenTree {
    TokenTree::Punct(Punct::new(p, Spacing::Joint))
}

pub(crate) fn ident(s: &str) -> TokenTree {
    TokenTree::Ident(Ident::new(s, Span::call_site()))
}

pub(crate) fn braces(t: impl IntoIterator<Item = TokenTree>) -> TokenTree {
    TokenTree::Group(Group::new(Delimiter::Brace, TokenStream::from_iter(t)))
}

pub(crate) fn string(s: &str) -> TokenTree {
    TokenTree::Literal(Literal::string(s))
}

/// Turn the tokens into a string with reconstructed whitespace.
///
/// If `variables` is set, variables (syntax: 'var) are replaced by `_RUST_var` and inserted in the map.
pub(crate) fn python_from_macro(
    input: TokenStream,
    variables: Option<&mut BTreeMap<String, Ident>>,
) -> Result<String, TokenStream> {
    struct Location {
        first_indent: Option<usize>,
        line: usize,
        column: usize,
    }

    fn add_whitespace(
        python: &mut String,
        loc: &mut Location,
        span: Span,
    ) -> Result<(), TokenStream> {
        let line = span.line();
        let column = span.column();
        if line > loc.line {
            while line > loc.line {
                python.push('\n');
                loc.line += 1;
            }
            let first_indent = *loc.first_indent.get_or_insert(column);
            let indent = column.checked_sub(first_indent);
            let indent =
                indent.ok_or_else(|| compile_error(Some((span, span)), "invalid indent"))?;
            for _ in 0..indent {
                python.push(' ');
            }
            loc.column = column;
        } else if line == loc.line {
            while column > loc.column {
                python.push(' ');
                loc.column += 1;
            }
        }
        Ok(())
    }

    fn add_tokens(
        python: &mut String,
        loc: &mut Location,
        input: TokenStream,
        mut variables: Option<&mut BTreeMap<String, Ident>>,
    ) -> Result<(), TokenStream> {
        let mut tokens = input.into_iter();
        while let Some(token) = tokens.next() {
            let span = token.span();
            add_whitespace(python, loc, span)?;
            match &token {
                TokenTree::Group(x) => {
                    let (start, end) = match x.delimiter() {
                        Delimiter::Parenthesis => ("(", ")"),
                        Delimiter::Brace => ("{", "}"),
                        Delimiter::Bracket => ("[", "]"),
                        Delimiter::None => ("", ""),
                    };
                    add_whitespace(python, loc, x.span_open())?;
                    python.push_str(start);
                    loc.column += start.len();
                    add_tokens(python, loc, x.stream(), variables.as_deref_mut())?;
                    add_whitespace(python, loc, x.span_close())?;
                    python.push_str(end);
                    loc.column += end.len();
                }
                TokenTree::Punct(x) => {
                    if let Some(variables) = &mut variables
                        && x.as_char() == '\''
                        && x.spacing() == Spacing::Joint
                    {
                        let Some(TokenTree::Ident(name)) = tokens.next() else {
                            unreachable!()
                        };
                        let name_str = format!("_RUST_{name}");
                        python.push_str(&name_str);
                        loc.column += name_str.chars().count() - 6 + 1;
                        variables.entry(name_str).or_insert(name);
                    } else if x.as_char() == '#' && x.spacing() == Spacing::Joint {
                        // Convert '##' to '//', because otherwise it's
                        // impossible to use the Python operators '//' and '//='.
                        match tokens.next() {
                            Some(TokenTree::Punct(ref p)) if p.as_char() == '#' => {
                                python.push_str("//");
                                loc.column += 2;
                            }
                            Some(TokenTree::Punct(p)) => {
                                python.push(x.as_char());
                                python.push(p.as_char());
                                loc.column += 2;
                            }
                            _ => {
                                unreachable!();
                            }
                        }
                    } else {
                        python.push(x.as_char());
                        loc.column += 1;
                    }
                }
                TokenTree::Ident(x) => {
                    write!(python, "{x}").unwrap();
                    let end_span = token.span().end();
                    loc.line = end_span.line();
                    loc.column = end_span.column();
                }
                TokenTree::Literal(x) => {
                    let s = x.to_string();
                    // Remove space in prefixed strings like `f ".."`.
                    // (`f".."` is not allowed in some versions+editions of Rust.)
                    if s.starts_with('"')
                        && python.ends_with(' ')
                        && python[..python.len() - 1].ends_with(|c: char| c.is_ascii_alphabetic())
                    {
                        python.pop();
                    }
                    python.push_str(&s);
                    let end_span = token.span().end();
                    loc.line = end_span.line();
                    loc.column = end_span.column();
                }
            }
        }
        Ok(())
    }

    let mut python = String::new();
    let mut location = Location {
        line: 1,
        column: 0,
        first_indent: None,
    };
    add_tokens(&mut python, &mut location, input, variables)?;
    Ok(python)
}

pub(crate) fn compile_python(
    py: Python<'_>,
    python: &CStr,
    filename: &CStr,
    tokens: TokenStream,
) -> Result<Py<PyAny>, TokenStream> {
    unsafe {
        pyo3::PyObject::from_owned_ptr_or_err(
            py,
            pyo3::ffi::Py_CompileString(
                python.as_ptr(),
                filename.as_ptr(),
                pyo3::ffi::Py_file_input,
            ),
        )
    }
    .map_err(|err| python_error_to_compile_error(py, err, tokens))
}

/// Format a nice error message for a python compilation error.
pub(crate) fn python_error_to_compile_error(
    py: Python,
    error: PyErr,
    tokens: TokenStream,
) -> TokenStream {
    /// Iterate recursively over all spans in a token stream.
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

    fn get_syntax_error_info(err: &Bound<'_, PyBaseException>) -> PyResult<(usize, String)> {
        let line: usize = err.getattr("lineno")?.extract()?;
        let msg: String = err.getattr("msg")?.extract()?;
        Ok((line, msg))
    }

    fn get_traceback_info(tb: &Bound<'_, PyTraceback>) -> PyResult<(String, usize)> {
        let frame = tb.getattr("tb_frame")?;
        let code = frame.getattr("f_code")?;
        let file: String = code.getattr("co_filename")?.extract()?;
        let line: usize = frame.getattr("f_lineno")?.extract()?;
        Ok((file, line))
    }

    let value = (&error).into_pyobject(py).unwrap();

    if value.is_none() {
        compile_error(None, &error.get_type(py).name().unwrap())
    } else if let Ok(true) = error.matches(py, pyo3::exceptions::PySyntaxError::type_object(py))
        && let Ok((line, msg)) = get_syntax_error_info(&value)
        && let Some(spans) = spans_for_line(tokens.clone(), line)
    {
        compile_error(Some(spans), &msg)
    } else if let Some(tb) = &error.traceback(py)
        && let Ok((file, line)) = get_traceback_info(tb)
        && file == Span::call_site().file()
        && let Some(spans) = spans_for_line(tokens, line)
        && let Ok(msg) = value.str()
    {
        compile_error(Some(spans), &msg)
    } else if let Ok(msg) = value.str() {
        compile_error(None, &msg)
    } else {
        compile_error(None, &error.get_type(py).name().unwrap())
    }
}
