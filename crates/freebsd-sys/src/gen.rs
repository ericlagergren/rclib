use anyhow::Result;
use proc_macro2::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::{Ident, Type};

use super::parse::{Flags, IntKind, Syscall, Syscalls, TypeKind};

impl Syscalls<'_> {
    pub(super) fn to_tokens(&self) -> Result<TokenStream> {
        let syscalls = self.into_iter().take(42).collect::<Result<Vec<_>>>()?;
        Ok(quote! {
            use core::ffi::{c_char, c_int, c_ulong, c_void};

            #(#syscalls)*
        })
    }
}

impl ToTokens for Syscall<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let number = self.number;

        if self.flags.contains(Flags::RESERVED) {
            tokens.extend(quote! {
                // Reserved: #number
            });
            return;
        }
        let name = fmt_name(self.name);
        let sys_name = format_ident!("SYS_{}", self.name.to_uppercase());
        let arg_names = self
            .args
            .iter()
            .map(|arg| fmt_name(arg.name))
            .collect::<Vec<_>>();
        let arg_types = self.args.iter().map(|arg| rtype(&arg.typ));

        tokens.extend(quote! {
            pub const #sys_name: u64 = #number;
            pub unsafe fn #name(#(#arg_names: #arg_types),*) -> Result<(i64, i64), Errno> {
                syscall!(#sys_name, #(#arg_names),*)
            }
        });
    }
}

fn fmt_name(name: &str) -> Ident {
    println!("fmt_name: '{name}'");
    syn::parse_str::<Ident>(name)
        .or_else(|err| syn::parse_str::<Ident>(&format!("r#{}", name)).map_err(|_| err))
        .or_else(|err| syn::parse_str::<Ident>(&format!("{}_", name)).map_err(|_| err))
        .expect("invalid identifier")
}

fn fmt_type(name: &str) -> Type {
    println!("fmt_type: '{name}'");
    syn::parse_str::<Type>(name)
        .or_else(|err| syn::parse_str::<Type>(&format!("r#{}", name)).map_err(|_| err))
        .or_else(|err| syn::parse_str::<Type>(&format!("{}_", name)).map_err(|_| err))
        .expect("invalid type")
}

fn rtype(ctype: &TypeKind<'_>) -> Type {
    let rtype = match ctype {
        TypeKind::Void => "c_void",
        TypeKind::Int(v) => match v {
            IntKind::Int => "c_int",
            IntKind::ULong => "c_ulong",
            IntKind::Char => "c_char",
            IntKind::Uintptr => "usize",
        },
        TypeKind::Pointer(v) => return rtype(v),
        TypeKind::Struct(v) => v,
        TypeKind::Unknown(v) => v,
    };
    fmt_type(rtype)
}
