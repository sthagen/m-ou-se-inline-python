//! Inline Python code directly in your Rust code.
//!
//! # Example
//!
//! ```
//! use inline_python::python;
//!
//! let who = "world";
//! let n = 5;
//! python! {
//!     for i in range('n):
//!         print(i, "Hello", 'who)
//!     print("Goodbye")
//! }
//! ```
//!
//! # How to use
//!
//! Use the `python!{..}` macro to write Python code directly in your Rust code.
//!
//! _NOTE:_ This crate uses the **unstable** [`proc_macro_span` feature](https://github.com/rust-lang/rust/issues/54725),
//! so it will only compile on Rust **nightly**.
//!
//! ## Using Rust variables
//!
//! To reference Rust variables, use `'var`, as shown in the example above.
//! `var` needs to implement [`pyo3::IntoPyObject`].
//!
//! ## Re-using a Python context
//!
//! It is possible to create a [`Context`] object ahead of time and use it for running the Python code.
//! The context can be re-used for multiple invocations to share global variables across macro calls.
//!
//! ```
//! # use inline_python::{Context, python};
//! let c = Context::new();
//!
//! c.run(python! {
//!   foo = 5
//! });
//!
//! c.run(python! {
//!   assert foo == 5
//! });
//! ```
//!
//! As a shortcut, you can assign a `python!{}` invocation directly to a
//! variable of type `Context` to create a new context and run the Python code
//! in it.
//!
//! ```
//! # use inline_python::{Context, python};
//! let c: Context = python! {
//!   foo = 5
//! };
//!
//! c.run(python! {
//!   assert foo == 5
//! });
//! ```
//!
//! ## Getting information back
//!
//! A [`Context`] object could also be used to pass information back to Rust,
//! as you can retrieve the global Python variables from the context through
//! [`Context::get`].
//!
//! ```
//! # use inline_python::{Context, python};
//! let c: Context = python! {
//!   foo = 5
//! };
//!
//! assert_eq!(c.get::<i32>("foo"), 5);
//! ```
//!
//! ## Syntax issues
//!
//! Since the Rust tokenizer will tokenize the Python code, some valid Python
//! code is rejected. The main things to remember are:
//!
//! - Use double quoted strings (`""`) instead of single quoted strings (`''`).
//!
//!   (Single quoted strings only work if they contain a single character, since
//!   in Rust, `'a'` is a character literal.)
//!
//! - Use `//`-comments instead of `#`-comments.
//!
//!   (If you use `#` comments, the Rust tokenizer will try to tokenize your
//!   comment, and complain if your comment doesn't tokenize properly.)
//!
//! - Write `f ""` instead of `f""`.
//!
//!   (String literals with prefixes, like `f""`, are reserved in Rust for
//!   future use. You can write `f ""` instead, which is automatically
//!   converted back to to `f""`.)
//!
//! Other minor things that don't work are:
//!
//! - The `//` and `//=` operators are unusable, as they start a comment.
//!
//!   Workaround: you can write `##` instead, which is automatically converted
//!   to `//`.
//!
//! - Certain escape codes in string literals.
//!   (Specifically: `\a`, `\b`, `\f`, `\v`, `\N{..}`, `\123` (octal escape
//!   codes), `\u`, and `\U`.)
//!
//!   These, however, are accepted just fine: `\\`, `\n`, `\t`, `\r`, `\xAB`
//!   (hex escape codes), and `\0`.
//!
//! - Raw string literals with escaped double quotes. (E.g. `r"...\"..."`.)
//!
//! - Triple-quoted byte- and raw-strings with content that would not be valid
//!   as a regular string. And the same for raw-byte and raw-format strings.
//!   (E.g. `b"""\xFF"""`, `r"""\z"""`, `fr"\z"`, `br"\xFF"`.)
//!
//! Everything else should work fine.

use pyo3::{Bound, Python, types::PyDict};

mod context;
mod run;

pub use self::context::Context;
pub use pyo3;

/// A block of Python code within your Rust code.
///
/// This macro can be used in three different ways:
///
///  1. By itself as a statement.
///     In this case, the Python code is executed directly.
///
///  2. By assigning it to a [`Context`].
///     In this case, the Python code is executed directly, and the context
///     (the global variables) are available for re-use by other Python code
///     or inspection by Rust code.
///
///  3. By passing it as an argument to a function taking a `PythonBlock`, such
///     as [`Context::run`].
///
/// See [the crate's module level documentation](index.html) for examples.
pub use inline_python_macros::python;

// `python!{..}` expands to `python_impl!{b"bytecode" var1 var2 …}`,
// which then expands to a call to `FromInlinePython::from_python_macro`.
#[macro_export]
#[doc(hidden)]
macro_rules! _python_block {
    ($bytecode:literal $($var:ident)*) => {
        $crate::FromInlinePython::from_python_macro(
            // The compiled python bytecode:
            $bytecode,
            // The closure that puts all the captured variables in the 'globals' dictionary:
            |globals| {
                $(
                    $crate::pyo3::prelude::PyDictMethods::set_item(
                        globals, concat!("_RUST_", stringify!($var)), $var
                    ).expect("python");
                )*
            },
            // The closure that is used to throw panics with the right location:
            |e| ::std::panic::panic_any(e),
        )
    }
}

#[doc(hidden)]
pub trait FromInlinePython<F: FnOnce(&Bound<PyDict>)> {
    /// The `python!{}` macro expands to a call to this function.
    fn from_python_macro(bytecode: &'static [u8], set_vars: F, panic: fn(String) -> !) -> Self;
}

/// Converting a `python!{}` block to `()` will run the Python code.
///
/// This happens when `python!{}` is used as a statement by itself.
impl<F: FnOnce(&Bound<PyDict>)> FromInlinePython<F> for () {
    #[track_caller]
    fn from_python_macro(bytecode: &'static [u8], set_vars: F, panic: fn(String) -> !) {
        let _: Context = FromInlinePython::from_python_macro(bytecode, set_vars, panic);
    }
}

/// Assigning a `python!{}` block to a `Context` will run the Python code and capture the resulting context.
impl<F: FnOnce(&Bound<PyDict>)> FromInlinePython<F> for Context {
    #[track_caller]
    fn from_python_macro(bytecode: &'static [u8], set_vars: F, panic: fn(String) -> !) -> Self {
        Python::with_gil(|py| {
            let context = Context::new_with_gil(py);
            context.run_with_gil(
                py,
                PythonBlock {
                    bytecode,
                    set_vars,
                    panic,
                },
            );
            context
        })
    }
}

/// Using a `python!{}` block as a `PythonBlock` object will not do anything yet.
#[cfg(not(doc))]
impl<F: FnOnce(&Bound<PyDict>)> FromInlinePython<F> for PythonBlock<F> {
    fn from_python_macro(bytecode: &'static [u8], set_vars: F, panic: fn(String) -> !) -> Self {
        Self {
            bytecode,
            set_vars,
            panic,
        }
    }
}

/// Represents a `python!{}` block.
#[cfg(not(doc))]
pub struct PythonBlock<F> {
    bytecode: &'static [u8],
    set_vars: F,
    panic: fn(String) -> !,
}

/// In the documentation, we just show `PythonBlock` in
/// `Context::run`'s signature, without any generic arguments.
#[cfg(doc)]
#[doc(hidden)]
pub struct PythonBlock;
