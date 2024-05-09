use std::{
    borrow::Cow,
    env,
    io::{self, Write},
    path::PathBuf,
    process::{Command, Stdio},
    thread,
};

use proc_macro2::TokenStream;
use tracing::warn;

pub struct Formatter {
    options: Options,
}

impl Formatter {
    /// Creates a new `Formatter`.
    pub fn new() -> Self {
        Self {
            options: Options::default(),
        }
    }

    /// Gets the rustfmt path to rustfmt the generated bindings.
    fn rustfmt_path(&self) -> io::Result<Cow<'_, PathBuf>> {
        if let Some(ref p) = self.options.rustfmt_path {
            return Ok(Cow::Borrowed(p));
        }
        if let Ok(rustfmt) = env::var("RUSTFMT") {
            return Ok(Cow::Owned(rustfmt.into()));
        }
        // No rustfmt binary was specified, so assume that the
        // binary is called "rustfmt" and that it is in the
        // user's PATH.
        Ok(Cow::Owned("rustfmt".into()))
    }

    /// Formats `source`.
    pub fn format(&self, tokens: &TokenStream) -> io::Result<String> {
        let rustfmt = self.rustfmt_path()?;
        let mut cmd = Command::new(&*rustfmt);

        cmd.stdin(Stdio::piped()).stdout(Stdio::piped());

        if let Some(path) = self
            .options
            .rustfmt_configuration_file
            .as_ref()
            .and_then(|f| f.to_str())
        {
            cmd.args(["--config-path", path]);
        }

        let mut child = cmd.spawn()?;
        let mut child_stdin = child.stdin.take().unwrap();
        let mut child_stdout = child.stdout.take().unwrap();

        let source = tokens.to_string();

        // Write to stdin in a new thread, so that we can read
        // from stdout on this thread. This keeps the child from
        // blocking on writing to its stdout which might block us
        // from writing to its stdin.
        let stdin_handle = thread::spawn(move || {
            let _ = child_stdin.write_all(source.as_bytes());
            source
        });

        let mut output = vec![];
        io::copy(&mut child_stdout, &mut output)?;

        let status = child.wait()?;
        let source = stdin_handle.join().expect(
            "The thread writing to rustfmt's stdin doesn't do \
             anything that could panic",
        );

        match String::from_utf8(output) {
            Ok(bindings) => match status.code() {
                Some(0) => Ok(bindings),
                Some(2) => Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Rustfmt parsing errors.".to_string(),
                )),
                Some(3) => {
                    rustfmt_non_fatal_error_diagnostic("Rustfmt could not format some lines");
                    Ok(bindings)
                }
                _ => Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Internal rustfmt error".to_string(),
                )),
            },
            _ => Ok(source),
        }
    }
}

fn rustfmt_non_fatal_error_diagnostic(msg: &str) {
    warn!("{msg}");
}

#[derive(Default)]
struct Options {
    rustfmt_path: Option<PathBuf>,
    rustfmt_configuration_file: Option<PathBuf>,
}
