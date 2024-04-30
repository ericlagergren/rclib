//! TODO

#![allow(dead_code)]

use std::{fmt, str::FromStr};

use anyhow::{Context, Result};
use bitflags::bitflags;

/// Parses a `syscall.master` file.
pub fn parse(data: &str) -> Result<Vec<Syscall<'_>>> {
    let mut syscalls = Vec::new();

    let iter = BlockIter::new(data);
    for block in iter {
        let sc = block?.try_into_syscall()?;
        syscalls.push(sc);
    }

    Ok(syscalls)
}

/// A system call.
#[derive(Debug)]
pub struct Syscall<'a> {
    number: i64,
    name: &'a str,
    args: Vec<(Name<'a>, Type<'a>)>,
}

impl fmt::Display for Syscall<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "pub unsafe fn {}(", self.name)?;
        for (name, typ) in &self.args {
            writeln!(f, "{name}: {typ}")?;
        }
        writeln!(f, ") -> Result<(i64, i64), Errno> {{")?;
        write!(f, "syscall!({}", self.number)?;
        for (name, _) in &self.args {
            write!(f, ", {name}")?;
        }
        writeln!(f, ")")?;
        writeln!(f, "}}")
    }
}

#[derive(Debug)]
struct Name<'a>(&'a str);

impl fmt::Display for Name<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug)]
struct Type<'a>(&'a str);

impl fmt::Display for Type<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

struct BlockIter<'a> {
    data: &'a str,
}

impl<'a> BlockIter<'a> {
    fn new(data: &'a str) -> Self {
        Self { data }
    }

    fn try_next(&mut self) -> Result<Option<Block<'a>>> {
        loop {
            let Some((line, rest)) = self.data.split_once("\n") else {
                return Ok(None);
            };
            self.data = rest;

            if line.is_empty()
                || line.starts_with(";")
                || line.starts_with("#")
                || line.starts_with("%%")
            {
                // Ignore comments, etc.
                continue;
            }

            // let line = line
            //     .trim()
            //     .strip_suffix("{")
            //     .context("missing `{` suffix")?
            //     .trim();

            println!("line={line}");
            let info = Info::try_parse(line)?;
            let (body, rest) = self
                .data
                .split_once("\t}\n")
                .context("missing closing `}`")?;
            self.data = rest;
            let block = Block { info, body };
            return Ok(Some(block));
        }
    }
}

impl<'a> Iterator for BlockIter<'a> {
    type Item = Result<Block<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.try_next().transpose()
    }
}

struct Block<'a> {
    info: Info<'a>,
    body: &'a str,
}

impl<'a> Block<'a> {
    fn try_into_syscall(self) -> Result<Syscall<'a>> {
        Ok(Syscall {
            number: self.info.number,
            name: "",
            args: Vec::new(),
        })
    }
}

struct Info<'a> {
    number: i64,
    audit: &'a str,
    types: Flags,
}

impl<'a> Info<'a> {
    fn try_parse(line: &'a str) -> Result<Self> {
        let mut parts = line.trim().split('\t');
        let number = parts.next().context("missing number")?.parse()?;
        let audit = parts.next().context("missing audit")?;
        let types = parts.next().context("missing types")?.parse()?;
        Ok(Self {
            number,
            audit,
            types,
        })
    }
}

#[derive(Copy, Clone, Default, Debug, Eq, PartialEq)]
#[repr(transparent)]
struct Flags(u64);

bitflags! {
    impl Flags: u64 {
        const STD = 1 << 0;
        const COMPAT = 1 << 1;
        const COMPAT4 = 1 << 2;
        const COMPAT6 = 1 << 3;
        const COMPAT7 = 1 << 4;
        const COMPAT10 = 1 << 5;
        const COMPAT11 = 1 << 6;
        const COMPAT12 = 1 << 7;
        const COMPAT13 = 1 << 8;
        const COMPAT14 = 1 << 9;
        const OBSOL = 1 << 10;
        const RESERVED = 1 << 11;
        const UNIMPL = 1 << 12;
        const NOSTD = 1 << 13;
        const NOARGS = 1 << 14;
        const NODEF = 1 << 15;
        const NOPROTO = 1 << 16;
        const NOTSTATIC = 1 << 17;
        const SYSMUX = 1 << 18;
        const CAPENABLED = 1 << 19;
    }
}

impl fmt::Display for Flags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        bitflags::parser::to_writer(self, f)
    }
}

impl FromStr for Flags {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let flags = s
            .split('|')
            .try_fold(Self::default(), |acc, name| -> Result<_> {
                let v = Flags::from_name(name.trim())
                    .with_context(|| format!("unknown `type`: {name}"))?;
                Ok(acc | v)
            })?;
        Ok(flags)
    }
}

#[derive(Default)]
struct Builder {
    decl: Decl,
}

#[derive(Default)]
struct Decl {
    result: Option<String>,
    name: Option<String>,
    inputs: Option<Vec<Input>>,
}

#[derive(Default)]
struct Input {
    annotations: Vec<String>,
    typ: String,
    name: String,
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    #[test]
    fn test_parse() {
        const SYSCALLS_MASTER: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/testdata/syscalls.master",
        ));

        let got = parse(SYSCALLS_MASTER).unwrap();
        for sc in got {
            println!("{sc}");
        }
    }
}
