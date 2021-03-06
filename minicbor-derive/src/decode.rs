use crate::Mode;
use crate::{add_bound_to_type_params, collect_type_params, is_cow, is_option, is_str, is_byte_slice};
use crate::attrs::{Attributes, CustomCodec, Encoding, Level};
use crate::fields::Fields;
use crate::variants::Variants;
use crate::lifetimes::{gen_lifetime, lifetimes_to_constrain, add_lifetime};
use quote::quote;
use std::collections::HashSet;
use syn::spanned::Spanned;

/// Entry point to derive `minicbor::Decode` on structs and enums.
pub fn derive_from(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let mut input = syn::parse_macro_input!(input as syn::DeriveInput);
    let result = match &input.data {
        syn::Data::Struct(_) => on_struct(&mut input),
        syn::Data::Enum(_)   => on_enum(&mut input),
        syn::Data::Union(u)  => {
            let msg = "deriving `minicbor::Decode` for a `union` is not supported";
            Err(syn::Error::new(u.union_token.span(), msg))
        }
    };
    proc_macro::TokenStream::from(result.unwrap_or_else(|e| e.to_compile_error()))
}

/// Create a `Decode` impl for (tuple) structs.
fn on_struct(inp: &mut syn::DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let data =
        if let syn::Data::Struct(data) = &inp.data {
            data
        } else {
            unreachable!("`derive_from` matched against `syn::Data::Struct`")
        };

    let name   = &inp.ident;
    let attrs  = Attributes::try_from_iter(Level::Struct, inp.attrs.iter())?;
    let fields = Fields::try_from(name.span(), data.fields.iter())?;

    let decode_fns: Vec<Option<CustomCodec>> = fields.attrs.iter()
        .map(|a| a.codec().cloned().filter(CustomCodec::is_decode))
        .collect();

    let mut lifetime = gen_lifetime()?;
    for l in lifetimes_to_constrain(fields.indices.iter().zip(fields.types.iter())) {
        if !lifetime.bounds.iter().any(|b| *b == l) {
            lifetime.bounds.push(l.clone())
        }
    }

    // Collect type parameters which should not have a `Decode` bound added,
    // i.e. from fields which have a custom decode function defined.
    let blacklist = {
        let iter = data.fields.iter()
            .zip(&decode_fns)
            .filter_map(|(f, ff)| ff.is_some().then(|| f));
        collect_type_params(&inp.generics, iter)
    };

    {
        let bound  = gen_decode_bound()?;
        let params = inp.generics.type_params_mut();
        add_bound_to_type_params(bound, params, &blacklist, &fields.attrs, Mode::Decode);
    }

    let g = add_lifetime(&inp.generics, lifetime);
    let (impl_generics, ..) = g.split_for_impl();
    let (_, typ_generics, where_clause) = inp.generics.split_for_impl();

    // If transparent, just forward the decode call to the inner type.
    if attrs.transparent() {
        if fields.len() != 1 {
            let msg = "#[cbor(transparent)] requires a struct with one field";
            return Err(syn::Error::new(inp.ident.span(), msg))
        }
        let f = data.fields.iter().next().expect("struct has 1 field");
        let a = fields.attrs.first().expect("struct has 1 field");
        return make_transparent_impl(&inp.ident, f, a, impl_generics, typ_generics, where_clause)
    }

    let field_str  = fields.idents.iter().map(|n| format!("{}::{}", name, n)).collect::<Vec<_>>();
    let statements = gen_statements(&fields, &decode_fns, attrs.encoding().unwrap_or_default())?;

    let Fields { indices, idents, .. } = fields;

    let result = if let syn::Fields::Named(_) = data.fields {
        quote! {
            Ok(#name {
                #(#idents : if let Some(x) = #idents {
                    x
                } else {
                    return Err(minicbor::decode::Error::MissingValue(#indices, #field_str))
                }),*
            })
        }
    } else if let syn::Fields::Unit = &data.fields {
        quote!(Ok(#name))
    } else {
        quote! {
            Ok(#name(#(if let Some(x) = #idents {
                x
            } else {
                return Err(minicbor::decode::Error::MissingValue(#indices, #field_str))
            }),*))
        }
    };

    Ok(quote! {
        impl #impl_generics minicbor::Decode<'bytes> for #name #typ_generics #where_clause {
            fn decode(__d777: &mut minicbor::Decoder<'bytes>) -> core::result::Result<#name #typ_generics, minicbor::decode::Error> {
                #statements
                #result
            }
        }
    })
}

/// Create a `Decode` impl for enums.
fn on_enum(inp: &mut syn::DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let data =
        if let syn::Data::Enum(data) = &inp.data {
            data
        } else {
            unreachable!("`derive_from` matched against `syn::Data::Enum`")
        };

    let name          = &inp.ident;
    let enum_attrs    = Attributes::try_from_iter(Level::Enum, inp.attrs.iter())?;
    let enum_encoding = enum_attrs.encoding().unwrap_or_default();
    let index_only    = enum_attrs.index_only();
    let variants      = Variants::try_from(name.span(), data.variants.iter())?;

    let mut blacklist = HashSet::new();
    let mut field_attrs = Vec::new();
    let mut lifetime = gen_lifetime()?;
    let mut rows = Vec::new();
    for ((var, idx), attrs) in data.variants.iter().zip(variants.indices.iter()).zip(&variants.attrs) {
        let fields = Fields::try_from(var.ident.span(), var.fields.iter())?;
        let encoding = attrs.encoding().unwrap_or(enum_encoding);
        let con = &var.ident;
        let row = if let syn::Fields::Unit = &var.fields {
            if index_only {
                quote!(#idx => Ok(#name::#con),)
            } else {
                quote!(#idx => {
                    __d777.skip()?;
                    Ok(#name::#con)
                })
            }
        } else {
            for l in lifetimes_to_constrain(fields.indices.iter().zip(fields.types.iter())) {
                if !lifetime.bounds.iter().any(|b| *b == l) {
                    lifetime.bounds.push(l.clone())
                }
            }
            let decode_fns: Vec<Option<CustomCodec>> = fields.attrs.iter()
                .map(|a| a.codec().cloned().filter(CustomCodec::is_decode))
                .collect();
            let field_str = fields.idents.iter()
                .map(|n| format!("{}::{}::{}", name, con, n))
                .collect::<Vec<_>>();
            // Collect type parameters which should not have an `Decode` bound added,
            // i.e. from fields which have a custom decode function defined.
            blacklist.extend({
                let iter = var.fields.iter()
                    .zip(&decode_fns)
                    .filter_map(|(f, ff)| ff.is_some().then(|| f));
                collect_type_params(&inp.generics, iter)
            });
            let statements = gen_statements(&fields, &decode_fns, encoding)?;
            let Fields { indices, idents, .. } = fields;
            if let syn::Fields::Named(_) = var.fields {
                quote! {
                    #idx => {
                        #statements
                        Ok(#name::#con {
                            #(#idents : if let Some(x) = #idents {
                                x
                            } else {
                                return Err(minicbor::decode::Error::MissingValue(#indices, #field_str))
                            }),*
                        })
                    }
                }
            } else {
                quote! {
                    #idx => {
                        #statements
                        Ok(#name::#con(#(if let Some(x) = #idents {
                            x
                        } else {
                            return Err(minicbor::decode::Error::MissingValue(#indices, #field_str))
                        }),*))
                    }
                }
            }
        };
        field_attrs.extend_from_slice(&fields.attrs);
        rows.push(row)
    }

    {
        let bound  = gen_decode_bound()?;
        let params = inp.generics.type_params_mut();
        add_bound_to_type_params(bound, params, &blacklist, &field_attrs, Mode::Decode);
    }

    let g = add_lifetime(&inp.generics, lifetime);
    let (impl_generics , ..) = g.split_for_impl();
    let (_, typ_generics, where_clause) = inp.generics.split_for_impl();

    let check = if index_only {
        quote!()
    } else {
        quote! {
            if Some(2) != __d777.array()? {
                return Err(minicbor::decode::Error::Message("expected enum (2-element array)"))
            }
        }
    };

    Ok(quote! {
        impl #impl_generics minicbor::Decode<'bytes> for #name #typ_generics #where_clause {
            fn decode(__d777: &mut minicbor::Decoder<'bytes>) -> core::result::Result<#name #typ_generics, minicbor::decode::Error> {
                #check
                match __d777.u32()? {
                    #(#rows)*
                    n => Err(minicbor::decode::Error::UnknownVariant(n))
                }
            }
        }
    })
}

/// Generate decoding statements for every item.
//
// For every name `n`, type `t` and index `i` we declare a local mutable
// variable `n` with type `Option<t>` and set it to `None` if `t` is not
// an `Option`, otherwise to `Some(None)`. [1]
//
// Then -- depending on the selected encoding -- we iterate over all CBOR
// map or array elements and if an index `j` equal to `i` is found, we
// attempt to decode the next CBOR item as a value `v` of type `t`. If
// successful, we assign the result to `n` as `Some(v)`, otherwise we
// error, or -- if `t` is an option and the decoding failed because an
// unknown enum variant was decoded -- we skip the variant value and
// continue decoding.
//
// --------------------------------------------------------------------
// [1]: These variables will later be deconstructed in `on_enum` and
// `on_struct` and their inner value will be used to initialise a field.
// If not present, an error will be produced.
fn gen_statements(fields: &Fields, decode_fns: &[Option<CustomCodec>], encoding: Encoding) -> syn::Result<proc_macro2::TokenStream> {
    assert_eq!(fields.len(), decode_fns.len());

    let default_decode_fn: syn::ExprPath = syn::parse_str("minicbor::Decode::decode")?;

    let inits = fields.types.iter().map(|ty| {
        if is_option(ty, |_| true) {
            quote!(Some(None))
        } else {
            quote!(None)
        }
    });

    let actions = fields.indices.iter().zip(fields.idents.iter().zip(fields.types.iter().zip(decode_fns)))
        .map(|(ix, (name, (ty, ff)))| {
            let decode_fn = ff.as_ref()
                .and_then(|ff| ff.to_decode_path())
                .unwrap_or_else(|| default_decode_fn.clone());
            if is_option(ty, |_| true) {
                return quote! {
                    match #decode_fn(__d777) {
                        Ok(__v777) => #name = Some(__v777),
                        Err(minicbor::decode::Error::UnknownVariant(_)) => { __d777.skip()? }
                        Err(e) => return Err(e)
                    }
                }
            }
            if ix.is_b() && is_cow(ty, |t| is_str(t) || is_byte_slice(t)) {
                return quote! {
                    match #decode_fn(__d777) {
                        Ok(__v777) => #name = Some(std::borrow::Cow::Borrowed(__v777)),
                        Err(minicbor::decode::Error::UnknownVariant(_)) => { __d777.skip()? }
                        Err(e) => return Err(e)
                    }
                }
            }
            quote!({ #name = Some(#decode_fn(__d777)?) })
    })
    .collect::<Vec<_>>();

    let Fields { idents, types, indices, .. } = fields;

    Ok(match encoding {
        Encoding::Array => quote! {
            #(let mut #idents : core::option::Option<#types> = #inits;)*

            if let Some(__len777) = __d777.array()? {
                for __i777 in 0 .. __len777 {
                    match __i777 {
                        #(#indices => #actions)*
                        _          => __d777.skip()?
                    }
                }
            } else {
                let mut __i777 = 0;
                while minicbor::data::Type::Break != __d777.datatype()? {
                    match __i777 {
                        #(#indices => #actions)*
                        _          => __d777.skip()?
                    }
                    __i777 += 1
                }
                __d777.skip()?
            }
        },
        Encoding::Map => quote! {
            #(let mut #idents : core::option::Option<#types> = #inits;)*

            if let Some(__len777) = __d777.map()? {
                for _ in 0 .. __len777 {
                    match __d777.u32()? {
                        #(#indices => #actions)*
                        _          => __d777.skip()?
                    }
                }
            } else {
                while minicbor::data::Type::Break != __d777.datatype()? {
                    match __d777.u32()? {
                        #(#indices => #actions)*
                        _          => __d777.skip()?
                    }
                }
                __d777.skip()?
            }
        }
    })
}

/// Forward the decoding because of a `#[cbor(transparent)]` attribute.
fn make_transparent_impl
    ( name: &syn::Ident
    , field: &syn::Field
    , attrs: &Attributes
    , impl_generics: syn::ImplGenerics
    , typ_generics: syn::TypeGenerics
    , where_clause: Option<&syn::WhereClause>
    ) -> syn::Result<proc_macro2::TokenStream>
{
    if attrs.codec().map(CustomCodec::is_decode).unwrap_or(false) {
        let msg  = "`decode_with` or `with` not allowed with #[cbor(transparent)]";
        let span = field.ident.as_ref().map(|i| i.span()).unwrap_or_else(|| field.ty.span());
        return Err(syn::Error::new(span, msg))
    }

    let call =
        if let Some(id) = &field.ident {
            quote! {
                Ok(#name { #id: minicbor::Decode::decode(__d777)? })
            }
        } else {
            quote! {
                Ok(#name(minicbor::Decode::decode(__d777)?))
            }
        };

    Ok(quote! {
        impl #impl_generics minicbor::Decode<'bytes> for #name #typ_generics #where_clause {
            fn decode(__d777: &mut minicbor::Decoder<'bytes>) -> core::result::Result<#name #typ_generics, minicbor::decode::Error> {
                #call
            }
        }
    })
}

fn gen_decode_bound() -> syn::Result<syn::TypeParamBound> {
    syn::parse_str("minicbor::Decode<'bytes>")
}
