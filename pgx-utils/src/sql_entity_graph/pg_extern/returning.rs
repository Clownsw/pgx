/*
Portions Copyright 2019-2021 ZomboDB, LLC.
Portions Copyright 2021-2022 Technology Concepts & Design, Inc. <support@tcdi.com>

All rights reserved.

Use of this source code is governed by the MIT license that can be found in the LICENSE file.
*/
use crate::sql_entity_graph::UsedType;
use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, ToTokens, TokenStreamExt};
use std::convert::TryFrom;
use syn::{
    parse::{Parse, ParseStream},
    spanned::Spanned,
    Token,
};

#[derive(Debug, Clone)]
pub struct ReturningIteratedItem {
    used_ty: UsedType,
    name: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Returning {
    None,
    Type(UsedType),
    SetOf(UsedType),
    Iterated(Vec<ReturningIteratedItem>),
    /// `pgx_pg_sys::Datum`
    Trigger,
}

impl Returning {
    fn parse_trait_bound(trait_bound: &mut syn::TraitBound) -> Result<Returning, syn::Error> {
        let last_path_segment = trait_bound.path.segments.last_mut().unwrap();
        match last_path_segment.ident.to_string().as_str() {
            "Iterator" => match &mut last_path_segment.arguments {
                syn::PathArguments::AngleBracketed(args) => match args.args.first_mut().unwrap() {
                    syn::GenericArgument::Binding(binding) => match &mut binding.ty {
                        syn::Type::Tuple(tuple_type) => Ok(Self::parse_type_tuple(tuple_type)?),
                        syn::Type::Path(path) => {
                            let used_ty = UsedType::new(syn::Type::Path(path.clone()))?;
                            Ok(Returning::SetOf(used_ty))
                        }
                        syn::Type::Reference(type_ref) => match &*type_ref.elem {
                            syn::Type::Path(path) => {
                                let used_ty = UsedType::new(syn::Type::Path(path.clone()))?;
                                Ok(Returning::SetOf(used_ty))
                            }
                            _ => unimplemented!("Expected path"),
                        },
                        ty => unimplemented!("Only iters with tuples, got {:?}.", ty),
                    },
                    _ => unimplemented!(),
                },
                _ => unimplemented!(),
            },
            _ => unimplemented!(),
        }
    }

    fn parse_type_tuple(type_tuple: &mut syn::TypeTuple) -> Result<Returning, syn::Error> {
        if type_tuple.elems.len() == 0 {
            return Ok(Returning::None);
        }
        let mut returns: Vec<ReturningIteratedItem> = vec![];
        for elem in &type_tuple.elems {
            let elem = elem.clone();

            let return_ty = match elem {
                syn::Type::Macro(ref macro_pat) => {
                    // This is essentially a copy of `parse_type_macro` but it returns items instead of `Returning`
                    let mac = &macro_pat.mac;
                    let archetype = mac.path.segments.last().unwrap();
                    match archetype.ident.to_string().as_str() {
                        "name" => {
                            let out: NameMacro = mac.parse_body()?;
                            Some(ReturningIteratedItem {
                                name: Some(out.ident),
                                used_ty: out.used_ty,
                            })
                        }
                        "composite_type" => {
                            let used_ty = UsedType::new(elem)?;
                            Some(ReturningIteratedItem {
                                used_ty,
                                name: None,
                            })
                        }
                        _ => unimplemented!(
                            "Don't support anything other than `name!()` and `composite_type!()`"
                        ),
                    }
                }
                ty => Some(ReturningIteratedItem {
                    used_ty: UsedType::new(ty)?,
                    name: None,
                }),
            };
            if let Some(return_ty) = return_ty {
                returns.push(return_ty);
            }
        }
        Ok(Returning::Iterated(returns))
    }

    fn parse_impl_trait(impl_trait: &mut syn::TypeImplTrait) -> Result<Returning, syn::Error> {
        match impl_trait.bounds.first_mut().unwrap() {
            syn::TypeParamBound::Trait(trait_bound) => Self::parse_trait_bound(trait_bound),
            _ => Ok(Returning::None),
        }
    }

    fn parse_type_macro(type_macro: &mut syn::TypeMacro) -> Result<Returning, syn::Error> {
        // This is essentially a copy of `parse_type_macro` but it returns items instead of `Returning`
        let mac = &type_macro.mac;
        let archetype = mac.path.segments.last().unwrap();
        match archetype.ident.to_string().as_str() {
            "name" => {
                let out: NameMacro = mac.parse_body()?;
                Ok(Returning::Iterated(vec![ReturningIteratedItem {
                    used_ty: out.used_ty,
                    name: Some(out.ident),
                }]))
            }
            "composite_type" => Ok(Returning::Type(UsedType::new(syn::Type::Macro(
                type_macro.clone(),
            ))?)),
            _ => unimplemented!(
                "Don't support anything other than `name!()` and `composite_type!()`"
            ),
        }
    }

    fn parse_dyn_trait(dyn_trait: &mut syn::TypeTraitObject) -> Result<Returning, syn::Error> {
        match dyn_trait.bounds.first_mut().unwrap() {
            syn::TypeParamBound::Trait(trait_bound) => Self::parse_trait_bound(trait_bound),
            _ => Ok(Returning::None),
        }
    }
}

impl TryFrom<&syn::ReturnType> for Returning {
    type Error = syn::Error;

    fn try_from(value: &syn::ReturnType) -> Result<Self, Self::Error> {
        match &value {
            syn::ReturnType::Default => Ok(Returning::None),
            syn::ReturnType::Type(_, ty) => {
                let mut ty = *ty.clone();

                match ty {
                    syn::Type::ImplTrait(mut impl_trait) => {
                        Returning::parse_impl_trait(&mut impl_trait)
                    }
                    syn::Type::TraitObject(mut dyn_trait) => {
                        Returning::parse_dyn_trait(&mut dyn_trait)
                    }
                    syn::Type::Path(mut typepath) => {
                        let path = &mut typepath.path;
                        let mut saw_pg_sys = false;
                        let mut saw_datum = false;
                        let mut saw_option_ident = false;
                        let mut saw_box_ident = false;
                        let mut maybe_inner_impl_trait = None;

                        for segment in &mut path.segments {
                            let ident_string = segment.ident.to_string();
                            match ident_string.as_str() {
                                "pg_sys" => saw_pg_sys = true,
                                "Datum" => saw_datum = true,
                                "Option" => saw_option_ident = true,
                                "Box" => saw_box_ident = true,
                                _ => (),
                            }
                            if saw_option_ident || saw_box_ident {
                                match &mut segment.arguments {
                                    syn::PathArguments::AngleBracketed(inside_brackets) => {
                                        match inside_brackets.args.first_mut() {
                                            Some(syn::GenericArgument::Type(
                                                syn::Type::ImplTrait(impl_trait),
                                            )) => {
                                                maybe_inner_impl_trait =
                                                    Some(Returning::parse_impl_trait(impl_trait)?);
                                            }
                                            Some(syn::GenericArgument::Type(
                                                syn::Type::TraitObject(dyn_trait),
                                            )) => {
                                                maybe_inner_impl_trait =
                                                    Some(Returning::parse_dyn_trait(dyn_trait)?)
                                            }
                                            _ => (),
                                        }
                                    }
                                    syn::PathArguments::None
                                    | syn::PathArguments::Parenthesized(_) => (),
                                }
                            }
                        }
                        if (saw_datum && saw_pg_sys) || (saw_datum && path.segments.len() == 1) {
                            Ok(Returning::Trigger)
                        } else if let Some(returning) = maybe_inner_impl_trait {
                            Ok(returning)
                        } else {
                            let used_ty = UsedType::new(syn::Type::Path(typepath.clone()))?;
                            Ok(Returning::Type(used_ty))
                        }
                    }
                    syn::Type::Reference(ty_ref) => {
                        let used_ty = UsedType::new(syn::Type::Reference(ty_ref.clone()))?;
                        Ok(Returning::Type(used_ty))
                    }
                    syn::Type::Tuple(ref mut tup) => Self::parse_type_tuple(tup),
                    syn::Type::Macro(ref mut type_macro) => Self::parse_type_macro(type_macro),
                    syn::Type::Paren(ref mut type_paren) => match &mut *type_paren.elem {
                        syn::Type::Macro(ref mut type_macro) => Self::parse_type_macro(type_macro),
                        other => {
                            return Err(syn::Error::new(
                                other.span(),
                                &format!("Got unknown return type: {type_paren:?}"),
                            ))
                        }
                    },
                    other => {
                        return Err(syn::Error::new(
                            other.span(),
                            &format!("Got unknown return type: {other:?}"),
                        ))
                    }
                }
            }
        }
    }
}

impl ToTokens for Returning {
    fn to_tokens(&self, tokens: &mut TokenStream2) {
        let quoted = match self {
            Returning::None => quote! {
                ::pgx::utils::sql_entity_graph::PgExternReturnEntity::None
            },
            Returning::Type(used_ty) => {
                let used_ty_entity_tokens = used_ty.entity_tokens();
                quote! {
                    ::pgx::utils::sql_entity_graph::PgExternReturnEntity::Type {
                        ty: #used_ty_entity_tokens,
                    }
                }
            }
            Returning::SetOf(used_ty) => {
                let used_ty_entity_tokens = used_ty.entity_tokens();
                quote! {
                    ::pgx::utils::sql_entity_graph::PgExternReturnEntity::SetOf {
                        ty: #used_ty_entity_tokens,
                    }
                }
            }
            Returning::Iterated(items) => {
                let quoted_items = items
                    .iter()
                    .map(|ReturningIteratedItem { used_ty, name }| {
                        let name_iter = name.iter();
                        let used_ty_entity_tokens = used_ty.entity_tokens();
                        quote! {
                            ::pgx::utils::sql_entity_graph::PgExternReturnEntityIteratedItem {
                                ty: #used_ty_entity_tokens,
                                name: None #( .unwrap_or(Some(stringify!(#name_iter))) )*,
                            }
                        }
                    })
                    .collect::<Vec<_>>();
                quote! {
                    ::pgx::utils::sql_entity_graph::PgExternReturnEntity::Iterated(vec![
                        #(#quoted_items),*
                    ])
                }
            }
            Returning::Trigger => quote! {
                ::pgx::utils::sql_entity_graph::PgExternReturnEntity::Trigger
            },
        };
        tokens.append_all(quoted);
    }
}

#[derive(Debug, Clone)]
pub struct NameMacro {
    pub(crate) ident: String,
    pub(crate) used_ty: UsedType,
}

impl Parse for NameMacro {
    fn parse(input: ParseStream) -> Result<Self, syn::Error> {
        let ident = input
            .parse::<syn::Ident>()
            .map(|v| v.to_string())
            // Avoid making folks unable to use rust keywords.
            .or_else(|_| {
                input
                    .parse::<syn::Token![type]>()
                    .map(|_| String::from("type"))
            })
            .or_else(|_| {
                input
                    .parse::<syn::Token![mod]>()
                    .map(|_| String::from("mod"))
            })
            .or_else(|_| {
                input
                    .parse::<syn::Token![extern]>()
                    .map(|_| String::from("extern"))
            })
            .or_else(|_| {
                input
                    .parse::<syn::Token![async]>()
                    .map(|_| String::from("async"))
            })
            .or_else(|_| {
                input
                    .parse::<syn::Token![crate]>()
                    .map(|_| String::from("crate"))
            })
            .or_else(|_| {
                input
                    .parse::<syn::Token![use]>()
                    .map(|_| String::from("use"))
            })?;
        let _comma: Token![,] = input.parse()?;
        let ty: syn::Type = input.parse()?;

        let used_ty = UsedType::new(ty)?;

        Ok(Self { ident, used_ty })
    }
}
