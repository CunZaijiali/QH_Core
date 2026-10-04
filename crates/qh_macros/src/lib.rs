//! Procedural macros shared by the qh workspace crates.
//!
//! This crate intentionally does not depend on `qh_core`. Runtime crates may depend on this
//! crate without creating a dependency cycle.

use proc_macro::TokenStream;

use quote::quote;
use syn::{Ident, Token, Type, Visibility, parse::Parse, parse::ParseStream, parse_macro_input};

struct IdDefinition {
    visibility: Visibility,
    name: Ident,
    inner: Type,
}

impl Parse for IdDefinition {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let visibility = input.parse::<Visibility>()?;
        let struct_form = input.peek(Token![struct]);
        if struct_form {
            input.parse::<Token![struct]>()?;
        }

        let name = input.parse::<Ident>()?;
        let inner = if struct_form {
            let content;
            syn::parenthesized!(content in input);
            let inner = content.parse::<Type>()?;
            if input.peek(Token![;]) {
                input.parse::<Token![;]>()?;
            }
            inner
        } else {
            input.parse::<Token![,]>()?;
            input.parse::<Type>()?
        };

        if !input.is_empty() {
            return Err(input.error("unexpected tokens after ID definition"));
        }

        Ok(Self {
            visibility,
            name,
            inner,
        })
    }
}

fn is_copy_type(inner: &Type) -> bool {
    matches!(
        inner,
        Type::Path(path)
            if path.qself.is_none()
                && path.path.segments.len() == 1
                && matches!(
                    path.path.segments.first().map(|segment| &segment.ident),
                    Some(ident)
                        if matches!(ident.to_string().as_str(),
                            "bool" | "char" | "i8" | "i16" | "i32" | "i64" | "i128"
                                | "isize" | "u8" | "u16" | "u32" | "u64" | "u128" | "usize"
                        )
                )
    )
}

fn is_string_type(inner: &Type) -> bool {
    matches!(
        inner,
        Type::Path(path)
            if path.qself.is_none()
                && path.path.segments.len() == 1
                && path.path.segments.first().is_some_and(|segment| segment.ident == "String")
    )
}

/// Defines a strongly typed identifier.
///
/// Both forms are supported:
///
/// ```ignore
/// define_id!(pub SessionId, u64);
/// define_id!(pub struct PluginId(String));
/// ```
///
/// The generated type is transparent for serde, hashable, orderable and provides constructors,
/// conversion to/from its inner value and `Display`. String IDs additionally implement
/// `From<&str>`.
#[proc_macro]
pub fn define_id(input: TokenStream) -> TokenStream {
    let definition = parse_macro_input!(input as IdDefinition);
    let IdDefinition {
        visibility,
        name,
        inner,
    } = definition;
    let copy_derive = if is_copy_type(&inner) {
        quote!(Copy,)
    } else {
        quote!()
    };
    let string_conversion = is_string_type(&inner).then(|| {
        quote! {
            impl From<&str> for #name {
                fn from(value: &str) -> Self {
                    Self(value.to_owned())
                }
            }
        }
    });

    quote! {
        #[derive(Debug, Clone, #copy_derive PartialEq, Eq, Hash, PartialOrd, Ord,
            ::serde::Serialize, ::serde::Deserialize)]
        #[serde(transparent)]
        #visibility struct #name(pub #inner);

        impl #name {
            pub fn new(value: impl Into<#inner>) -> Self {
                Self(value.into())
            }

            pub fn value(&self) -> #inner
            where
                #inner: Clone,
            {
                self.0.clone()
            }

            pub fn into_inner(self) -> #inner {
                self.0
            }
        }

        impl From<#inner> for #name {
            fn from(value: #inner) -> Self {
                Self(value)
            }
        }

        impl From<#name> for #inner {
            fn from(value: #name) -> Self {
                value.0
            }
        }

        #string_conversion

        impl ::std::fmt::Display for #name {
            fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                self.0.fmt(formatter)
            }
        }
    }
    
    .into()
}

/// Marker attribute for future extension declarations.
///
/// The first version is intentionally transparent. It establishes the public macro namespace
/// without imposing a runtime representation before the extension contracts are stable.
#[proc_macro_attribute]
pub fn qh_extension(_attributes: TokenStream, item: TokenStream) -> TokenStream {
    item
}
