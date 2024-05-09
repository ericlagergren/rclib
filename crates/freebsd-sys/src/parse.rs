//! TODO

#![allow(dead_code)]

use std::{fmt, io::Write, ops::RangeInclusive, str::FromStr};

use anyhow::{anyhow, ensure, Context, Result};
use bitflags::bitflags;

use super::rustfmt::Formatter;

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
    pub fn write<W: Write>(&self, w: &mut W) -> Result<()> {
        let tokens = self.to_tokens()?;
        let source = Formatter::new().format(&tokens)?;
        Ok(w.write_all(source.as_bytes())?)
    }
}

impl<'a> Iterator for Syscalls<'a> {
    type Item = Result<Syscall<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.try_next().transpose()
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
struct Block<'a> {
    result: &'a str,
    star: usize,
    name: &'a str,
    args: Args<'a>,
}

impl<'a> Block<'a> {
    fn parse(s: &'a str) -> Result<Self> {
        let (result, rest) = s.split_once(" ").context("missing result")?;
        let (mut name, rest) = rest.split_once("(").context("missing opening '(")?;
        let (inner, rest) = rest.rsplit_once(")").context("missing trailing ')")?;

        let mut star = 0;
        while let Some(s) = name.strip_prefix("*") {
            name = s;
            star += 1;
        }

        let args = Args::parse(inner.trim())?;

        ensure!(rest == ";");
        Ok(Block {
            result,
            star,
            name,
            args,
        })
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
    pub typ: TypeKind<'a>,
    /// SAL annotation, if any.
    pub annotation: Option<Annotation<'a>>,
    /// `_Contains_` annotation, if any.
    pub contains: Option<Contains>,
}

impl<'a> Arg<'a> {
    fn parse(mut s: &'a str) -> Result<Self> {
        if s == "..." {
            return Ok(Self {
                name: "_variadic",
                typ: TypeKind::Void,
                annotation: None,
                contains: None,
            });
        }

        fn next<'a>(s: &mut &'a str) -> Result<&'a str> {
            let (token, rest) = s
                .split_once(|c: char| c.is_ascii_whitespace())
                .context("should have at least one token")?;
            *s = rest;
            Ok(token)
        }

        let mut token = next(&mut s)?;

        let annotation = Annotation::parse(token)?;
        if annotation.is_some() {
            token = next(&mut s)?;
        }
        let contains = Contains::parse(token)?;
        if contains.is_some() {
            token = next(&mut s)?;
        }

        let (typ, name) = s
            .rsplit_once(|c: char| c == '*' || c.is_ascii_whitespace())
            .context("syntax error")?;
        let typ = TypeKind::parse(typ.trim())?;
        Ok(Self {
            name,
            typ,
            annotation,
            contains,
        })
    }
}

/// A C type.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum TypeKind<'a> {
    /// `void`.
    Void,
    /// An integer.
    Int(IntKind),
    /// A pointer to a type.
    Pointer(Box<TypeKind<'a>>),
    /// A `struct`.
    Struct(&'a str),
    /// Some other type.
    Unknown(&'a str),
}

/// A C integer type.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum IntKind {
    /// An `int`.
    Int,
    /// A `u_long`.
    ULong,
    /// A `char`.
    Char,
    /// A `uintptr_t`.
    Uintptr,
}

impl<'a> TypeKind<'a> {
    fn parse(s: &'a str) -> Result<Self> {
        println!("---");
        println!("PARSE: '{s}'");
        let mut fields = s.split_ascii_whitespace();

        let mut field = fields.next().context("expected at least one field")?;

        let const_ = field == "const";
        if const_ {
            field = fields.next().context("TODO")?;
        }
        println!("const = {const_}");

        let mut kind = Self::Unknown("???");

        if field == "struct" {
            kind = Self::Struct("");
            field = fields.next().context("TODO")?;
        }
        println!("struct = {}", kind == Self::Struct(""));

        let name = field;

        // while let Some(s) = name.strip_prefix("*") {
        //     name = s;
        //     kind = Self::Pointer(Box::new(kind))
        // }
        println!("name = {name}");
        println!("---");
        println!("");

        Ok(kind)
    }
}

/// A SAL annotation.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[allow(missing_docs)] // TODO
pub enum Annotation<'a> {
    In,
    InOpt,
    Out,
    OutOpt,
    InOut,
    InOutOpt,

    InZ,
    InZOpt,
    OutZ,
    OutZOpt,
    InOutZ,
    InOutZOpt,

    InReadsZ(&'a str),
    InReadsZOpt(&'a str),
    OutWritesZ(&'a str),
    OutWritesZOpt(&'a str),
    InOutUpdatesZ(&'a str),
    InOutUpdatesZOpt(&'a str),

    InReads(&'a str),
    InReadsOpt(&'a str),
    OutWrites(&'a str),
    OutWritesOpt(&'a str),
    InOutUpdates(&'a str),
    InOutUpdatesOpt(&'a str),

    InReadsBytes(&'a str),
    InReadsBytesOpt(&'a str),
    OutWritesBytes(&'a str),
    OutWritesBytesOpt(&'a str),
    InOutUpdatesBytes(&'a str),
    InOutUpdatesBytesOpt(&'a str),
}

impl<'a> Annotation<'a> {
    fn parse(s: &'a str) -> Result<Option<Self>> {
        let (name, rest) = s.split_once("(").unwrap_or((s, ""));
        let arg = || {
            let (n, rest) = rest
                .split_once(")")
                .context("malformed annotation: missing closing ')'")?;
            ensure!(rest.is_empty());
            Ok(n)
        };
        let v = match name {
            "_In_" => Self::In,
            "_In_opt_" => Self::InOpt,
            "_Out_" => Self::Out,
            "_Out_opt_" => Self::OutOpt,
            "_Inout_opt_" => Self::InOutOpt,
            "_Inout_" => Self::InOut,

            "_In_z_" => Self::InZ,
            "_In_z_opt_" => Self::InZOpt,
            "_Out_z_" => Self::OutZ,
            "_Out_z_opt_" => Self::OutZOpt,
            "_Inout_z_" => Self::InOutZ,
            "_Inout_z_opt_" => Self::InOutZOpt,

            "_In_reads_z_" => Self::InReadsZ(arg()?),
            "_In_reads_z_opt_" => Self::InReadsZOpt(arg()?),
            "_Out_writes_z_" => Self::OutWritesZ(arg()?),
            "_Out_writes_z_opt_" => Self::OutWritesZOpt(arg()?),
            "_Inout_updates_z_" => Self::InOutUpdatesZ(arg()?),
            "_Inout_updates_z_opt_" => Self::InOutUpdatesZOpt(arg()?),

            "_In_reads_" => Self::InReads(arg()?),
            "_In_reads_opt_" => Self::InReadsOpt(arg()?),
            "_Out_writes_" => Self::OutWrites(arg()?),
            "_Out_writes_opt_" => Self::OutWritesOpt(arg()?),
            "_Inout_updates_" => Self::InOutUpdates(arg()?),
            "_Inout_updates_opt_" => Self::InOutUpdatesOpt(arg()?),

            "_In_reads_bytes_" => Self::InReadsBytes(arg()?),
            "_In_reads_bytes_opt_" => Self::InReadsBytesOpt(arg()?),
            "_Out_writes_bytes_" => Self::OutWritesBytes(arg()?),
            "_Out_writes_bytes_opt_" => Self::OutWritesBytesOpt(arg()?),
            "_Inout_updates_bytes_" => Self::InOutUpdatesBytes(arg()?),
            "_Inout_updates_bytes_opt_" => Self::InOutUpdatesBytesOpt(arg()?),
            _ => return Ok(None),
        };
        Ok(Some(v))
    }
}

impl fmt::Display for Annotation<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Annotation::*;
        match self {
            In => "_In_".fmt(f),
            InOpt => "_In_opt_".fmt(f),
            Out => "_Out_".fmt(f),
            OutOpt => "_Out_opt_".fmt(f),
            InOut => "_Inout_".fmt(f),
            InOutOpt => "_Inout_opt_".fmt(f),

            InZ => "_In_z_".fmt(f),
            InZOpt => "_In_z_opt_".fmt(f),
            OutZ => "_Out_z_".fmt(f),
            OutZOpt => "_Out_z_opt_".fmt(f),
            InOutZ => "_Inout_z_".fmt(f),
            InOutZOpt => "_Inout_z_opt_".fmt(f),

            InReadsZ(n) => write!(f, "_In_reads_z_({n})"),
            InReadsZOpt(n) => write!(f, "_In_reads_z_opt_({n})"),
            OutWritesZ(n) => write!(f, "_Out_writes_z_({n})"),
            OutWritesZOpt(n) => write!(f, "_Out_writes_z_opt_({n})"),
            InOutUpdatesZ(n) => write!(f, "_Inout_updates_z_({n})"),
            InOutUpdatesZOpt(n) => write!(f, "_Inout_updates_z_opt_({n})"),

            InReads(n) => write!(f, "_In_reads_({n})"),
            InReadsOpt(n) => write!(f, "_In_reads_opt_({n})"),
            OutWrites(n) => write!(f, "_Out_writes_({n})"),
            OutWritesOpt(n) => write!(f, "_Out_writes_opt_({n})"),
            InOutUpdates(n) => write!(f, "_Inout_updates_({n})"),
            InOutUpdatesOpt(n) => write!(f, "_Inout_updates_opt_({n})"),

            InReadsBytes(n) => write!(f, "_In_reads_bytes_({n})"),
            InReadsBytesOpt(n) => write!(f, "_In_reads_bytes_opt_({n})"),
            OutWritesBytes(n) => write!(f, "_Out_writes_bytes({n})"),
            OutWritesBytesOpt(n) => write!(f, "_Out_writes_bytes_opt_({n})"),
            InOutUpdatesBytes(n) => write!(f, "_Inout_updates_bytes_({n})"),
            InOutUpdatesBytesOpt(n) => write!(f, "_Inout_updates_bytes_opt_({n})"),
        }
    }
}

/// `_Contains_` annotation.
#[derive(Copy, Clone, Default, Debug, Eq, PartialEq)]
pub struct Contains(u64);

bitflags! {
    impl Contains: u64 {
        /// Contains `long`.
        const LONG = 1 << 0;
        /// Contains a pointer.
        const PTR = 1 << 1;
        /// Contains `time_t`.
        const TIME_T = 1 << 2;
    }
}

impl Contains {
    fn parse(s: &str) -> Result<Option<Self>> {
        let Some(types) = s.strip_prefix("_Contains_") else {
            return Ok(None);
        };
        let mut flags = Self::default();
        for typ in strip_suffix(types, "_").split("_") {
            match typ {
                "long" => flags |= Self::LONG,
                "ptr" => flags |= Self::PTR,
                "timet" => flags |= Self::TIME_T,
                _ => return Err(anyhow!("unknown type: '{typ}'")),
            }
        }
        Ok(Some(flags))
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
    fn test_annotation() {
        use Annotation::*;

        const ANNOTATIONS: [Annotation<'static>; 30] = [
            In,
            InOpt,
            Out,
            OutOpt,
            InOut,
            InOutOpt,
            InZ,
            InZOpt,
            OutZ,
            OutZOpt,
            InOutZ,
            InOutZOpt,
            InReadsZ("len"),
            InReadsZOpt("len"),
            OutWritesZ("len"),
            OutWritesZOpt("len"),
            InOutUpdatesZ("nfds"),
            InOutUpdatesZOpt("nfds"),
            InReads("n"),
            InReadsOpt("n"),
            OutWrites("n"),
            OutWritesOpt("n"),
            InOutUpdates("x"),
            InOutUpdatesOpt("x"),
            InReadsBytes("foo"),
            InReadsBytesOpt("foo"),
            OutWritesBytesOpt("bar"),
            OutWritesBytesOpt("bar"),
            InOutUpdatesBytesOpt("baz"),
            InOutUpdatesBytesOpt("baz"),
        ];
        for (i, want) in ANNOTATIONS.into_iter().enumerate() {
            let s = format!("{want}");
            let got = Annotation::parse(&s).unwrap();
            assert_eq!(got, Some(want), "#{i}");
        }
    }

    #[test]
    #[ignore]
    fn test_parse() {
        const SYSCALLS_MASTER: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/testdata/syscalls.master",
        ));

        let got = parse(SYSCALLS_MASTER);
        //println!("{got}");
        let mut buf = Vec::new();
        got.write(&mut buf).unwrap();
        println!("{}", String::from_utf8(buf).unwrap());
    }

    #[test]
    fn test_type() {
        let cases = [
            (
                "const* char",
                TypeKind::Pointer(Box::new(TypeKind::Int(IntKind::Char))),
            ),
            ("struct foo", TypeKind::Struct("foo")),
        ];
        for (i, (input, want)) in cases.into_iter().enumerate() {
            let got = TypeKind::parse(input).unwrap();
            assert_eq!(got, want, "#{i}");
        }
    }
}
