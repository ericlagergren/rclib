//! TODO

#![allow(dead_code)]

use std::{fmt, ops::RangeInclusive, str::FromStr};

use anyhow::{ensure, Context, Result};
use bitflags::bitflags;
use strum::{AsRefStr, Display, EnumString, IntoStaticStr};

/// Parses a `syscall.master` file.
pub fn parse(data: &str) -> Syscalls<'_> {
    Syscalls::new(data)
}

/// An iterator over [`Syscall`]s.
#[derive(Copy, Clone, Debug)]
pub struct Syscalls<'a> {
    data: &'a str,
}

impl<'a> Syscalls<'a> {
    const fn new(data: &'a str) -> Self {
        Self { data }
    }

    fn try_next(&mut self) -> Result<Option<Syscall<'a>>> {
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

            let (line, has_block) = {
                if let Some(line) = line.trim().strip_suffix("{") {
                    (line.trim(), true)
                } else {
                    (line, false)
                }
            };
            //println!("# line={line} has_block={has_block}");

            let (numbers, audit, flags, name) = {
                let mut cols = line.trim().split_ascii_whitespace();
                let numbers = cols.next().context("missing number")?.parse::<Numbers>()?;
                let audit = cols.next().context("missing audit")?;
                let flags = cols.next().context("missing types")?.parse::<Flags>()?;
                let name = cols.next();
                (numbers, audit, flags, name)
            };

            if flags.intersects(Flags::OBSOL | Flags::RESERVED) {
                // These don't have blocks.
                ensure!(!has_block);
            }

            if flags.contains(Flags::RESERVED) {
                // Nothing to do here.
                continue;
            }

            // Some `RESERVED` use ranges of numbers, everything
            // else just uses a single number.
            ensure!(numbers.start() == numbers.end());
            let number = numbers.start();

            let (name, args) = if !has_block {
                (name.context("missing `name`")?, Vec::new())
            } else {
                let (block, rest) = self
                    .data
                    .split_once("\t}\n")
                    .context("missing closing `\\t}\\n`")?;
                self.data = rest;
                let Block { name, args, .. } = Block::parse(block.trim())?;
                (name, args.0)
            };

            let sc = Syscall {
                number,
                name,
                args,
                audit,
                flags,
            };
            return Ok(Some(sc));
        }
    }

    /// Writes the syscalls to `w`.
    pub fn display<W: fmt::Write>(&self, w: &mut W) -> Result<()> {
        for sc in *self {
            let sc = sc?;

            if sc.flags.contains(Flags::RESERVED) {
                writeln!(w, "// Reserved: {}", sc.number)?;
                return Ok(());
            }

            writeln!(w, "pub const SYS_{}: u64 = {};", sc.name, sc.number)?;

            writeln!(w, "pub unsafe fn {}(", sc.name)?;
            for Arg { name, typ } in &sc.args {
                writeln!(w, "\t{name}: {typ},")?;
            }
            writeln!(w, ") -> Result<(i64, i64), Errno> {{")?;
            write!(w, "\tsyscall!(SYS_{}", sc.name)?;
            for Arg { name, .. } in &sc.args {
                write!(w, ", {name}")?;
            }
            writeln!(w, ")")?;
            writeln!(w, "}}")?;
        }
        Ok(())
    }
}

impl<'a> Iterator for Syscalls<'a> {
    type Item = Result<Syscall<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.try_next().transpose()
    }
}

impl<'a> fmt::Display for Syscalls<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.display(f).map_err(|_| fmt::Error)
    }
}

/// A system call.
#[derive(Debug)]
pub struct Syscall<'a> {
    /// The syscall's number.
    pub number: u64,
    /// The name of the syscall.
    pub name: &'a str,
    /// The syscall's arguments.
    pub args: Vec<Arg<'a>>,
    /// The audit event associated with the syscall.
    pub audit: &'a str,
    /// Flags applied to the syscall.
    pub flags: Flags,
}

impl fmt::Display for Syscall<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SYS_{}", self.name)
    }
}

struct Numbers(RangeInclusive<u64>);

impl Numbers {
    const fn start(&self) -> u64 {
        *self.0.start()
    }

    const fn end(&self) -> u64 {
        *self.0.end()
    }
}

impl FromStr for Numbers {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some((lhs, rhs)) = s.split_once("-") {
            let start = lhs.parse()?;
            let end = rhs.parse()?;
            Ok(Self(start..=end))
        } else {
            let start = s.parse()?;
            Ok(Self(start..=start))
        }
    }
}

impl IntoIterator for Numbers {
    type Item = u64;
    type IntoIter = RangeInclusive<u64>;

    fn into_iter(self) -> Self::IntoIter {
        self.0
    }
}

// ssize_t write(
//     int fd,
//     _In_reads_bytes_(nbyte) const void *buf,
//     size_t nbyte
// );
#[derive(Default)]
struct Block<'a> {
    result: &'a str,
    name: &'a str,
    args: Args<'a>,
}

impl<'a> Block<'a> {
    fn parse(s: &'a str) -> Result<Self> {
        let (result, rest) = s.split_once(" ").context("missing result")?;
        let (name, rest) = rest.split_once("(").context("missing opening '(")?;
        let (inner, rest) = rest.rsplit_once(")").context("missing trailing ')")?;

        println!("inner = '{}'", inner.trim());
        let args = Args::parse(inner.trim())?;

        ensure!(rest == ";");
        Ok(Block { result, name, args })
    }
}

#[derive(Default)]
struct Args<'a>(Vec<Arg<'a>>);

impl<'a> Args<'a> {
    fn parse(s: &'a str) -> Result<Self> {
        if s == "void" {
            return Ok(Self(Vec::new()));
        }
        let mut args = Vec::new();
        for line in s.trim().lines() {
            let line = strip_suffix(line.trim(), ",");
            println!("line = '{line}'");
            let arg = Arg::parse(line.trim())?;
            args.push(arg);
        }
        Ok(Self(args))
    }
}

/// An arugment to a syscall.
#[derive(Debug)]
pub struct Arg<'a> {
    /// The argument's name.
    pub name: &'a str,
    /// The argument's type.
    pub typ: &'a str,
}

impl<'a> Arg<'a> {
    fn parse(s: &'a str) -> Result<Self> {
        println!("parse: '{s}'");
        if s == "..." {
            return Ok(Self {
                name: "_todo",
                typ: "()",
            });
        }
        let (rest, mut name) = s.rsplit_once(" ").context("missing ' ' in arg")?;
        let mut star = 0;
        loop {
            if let Some(v) = name.strip_prefix("*") {
                star += 1;
                name = v;
            } else {
                break;
            }
        }
        Ok(Self { name, typ: "()" })
    }
}

struct Type<'a> {
    name: &'a str,
    annotations: Vec<()>,
}

impl<'a> Type<'a> {
    fn parse(_s: &'a str) -> Result<Self> {
        todo!()
    }
}

#[derive(Debug, Eq, PartialEq)]
enum Annotation<'a> {
    In,
    Out,
    InOut,
    InZ,
    OutZ,
    InOutZ,
    InReadsZ(&'a str),
    OutWritesZ(&'a str),
    InOutUpdatesZ(&'a str),
    InReads(&'a str),
    OutWrites(&'a str),
    InOutUpdates(&'a str),
    InReadsBytes(&'a str),
    OutWritesBytes(&'a str),
    InOutUpdatesBytes(&'a str),
}

impl<'a> Annotation<'a> {
    fn parse(s: &'a str) -> Result<Self> {
        match s {
            "_In_" => return Ok(Self::In),
            "_Out_" => return Ok(Self::Out),
            "_Inout_" => return Ok(Self::InOut),
            "_In_z_" => return Ok(Self::InZ),
            "_Out_z_" => return Ok(Self::OutZ),
            "_Inout_z_" => return Ok(Self::InOutZ),
            _ => {}
        }
        let (name, rest) = s.split_once("(").context("missing opening '('")?;
        let (n, rest) = rest.split_once(")").context("missing closing ')")?;
        ensure!(rest.is_empty());
        match name {
            "InReadsZ" => Ok(Self::InReadsZ(n)),
            "OutWritesZ" => Ok(Self::OutWritesZ(n)),
            "InOutUpdatesZ" => Ok(Self::InOutUpdatesZ(n)),
            "InReads" => Ok(Self::InReads(n)),
            "OutWrites" => Ok(Self::OutWrites(n)),
            "InOutUpdates" => Ok(Self::InOutUpdates(n)),
            "InReadsBytes" => Ok(Self::InReadsBytes(n)),
            "OutWritesBytes" => Ok(Self::OutWritesBytes(n)),
            "InOutUpdatesBytes" => Ok(Self::InOutUpdatesBytes(n)),
        }
    }
}

impl fmt::Display for Annotation<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

/// Flags applied to a syscall.
#[derive(Copy, Clone, Default, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Flags(u64);

bitflags! {
    impl Flags: u64 {
        /// Always included.
        const STD = 1 << 0;
        /// FreeBSD compat.
        const COMPAT = 1 << 1;
        /// FreeBSD 4 compat.
        const COMPAT4 = 1 << 2;
        /// FreeBSD 6 compat.
        const COMPAT6 = 1 << 3;
        /// FreeBSD 7 compat.
        const COMPAT7 = 1 << 4;
        /// FreeBSD 10 compat.
        const COMPAT10 = 1 << 5;
        /// FreeBSD 11 compat.
        const COMPAT11 = 1 << 6;
        /// FreeBSD 12 compat.
        const COMPAT12 = 1 << 7;
        /// FreeBSD 13 compat.
        const COMPAT13 = 1 << 8;
        /// FreeBSD 14 compat.
        const COMPAT14 = 1 << 9;
        /// Obsolete and not included in the system.
        const OBSOL = 1 << 10;
        /// Reserved for non-FreeBSD use.
        const RESERVED = 1 << 11;
        /// Unimplemented, placeholder only.
        const UNIMPL = 1 << 12;
        /// Implemented but as an LKM.
        const NOSTD = 1 << 13;
        /// Like `STD`, but does not create a structure in
        /// `sys/sysproto.h`.
        const NOARGS = 1 << 14;
        /// Same as `STD`, but do not create a structure or
        /// prototype in `sys/sysproto.h`.
        const NODEF = 1 << 15;
        /// Same as `STD`, but but do not create a prototype in
        /// `sys/sysproto.h`.
        const NOPROTO = 1 << 16;
        /// Syscall is loadable.
        const NOTSTATIC = 1 << 17;
        /// Syscall multiplexer.
        const SYSMUX = 1 << 18;
        /// Syscall is allowed in capability mode.
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
                    .with_context(|| format!("unknown `type`: '{name}'"))?;
                Ok(acc | v)
            })?;
        Ok(flags)
    }
}

fn strip_suffix<'a>(s: &'a str, suffix: &'a str) -> &'a str {
    s.strip_suffix(suffix).unwrap_or(s)
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

        let got = parse(SYSCALLS_MASTER);
        //println!("{got}");
        let mut buf = String::new();
        got.display(&mut buf).unwrap();
        println!("{buf}");
    }
}
