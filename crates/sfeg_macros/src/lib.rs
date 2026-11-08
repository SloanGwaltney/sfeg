//! SFEG's derive macros. Games use them through the `sfeg` crate, such as
//! `sfeg::input::Action`.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{DeriveInput, Ident, LitStr, parse_macro_input};

/// The action kinds a derive can name, as their type names in
/// `sfeg::input`.
const KINDS: [&str; 2] = ["Button", "Axis2d"];

/// Declares an input action (ADR 0016):
///
/// ```ignore
/// #[derive(Action)]
/// #[action(name = "jump", kind = Button)]
/// struct Jump;
/// ```
///
/// `name` is the action's name in binding files: lowercase letters, digits,
/// and underscores, starting with a letter. It is never taken from the type
/// name, so renaming the type never breaks saved bindings. `kind` is one
/// of `Button` or `Axis2d`.
#[proc_macro_derive(Action, attributes(action))]
pub fn derive_action(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_action(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_action(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "an action can't be generic: each action is one concrete type",
        ));
    }

    let mut attributes = input
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("action"));
    let Some(attribute) = attributes.next() else {
        return Err(syn::Error::new(
            Span::call_site(),
            "an action needs `#[action(name = \"...\", kind = ...)]`",
        ));
    };
    if let Some(extra) = attributes.next() {
        return Err(syn::Error::new_spanned(
            extra,
            "an action has exactly one `#[action(...)]` attribute",
        ));
    }

    let mut name: Option<LitStr> = None;
    let mut kind: Option<Ident> = None;
    attribute.parse_nested_meta(|meta| {
        if meta.path.is_ident("name") {
            if name.is_some() {
                return Err(meta.error("`name` is given twice"));
            }
            name = Some(meta.value()?.parse()?);
            Ok(())
        } else if meta.path.is_ident("kind") {
            if kind.is_some() {
                return Err(meta.error("`kind` is given twice"));
            }
            kind = Some(meta.value()?.parse()?);
            Ok(())
        } else {
            Err(meta.error("expected `name` or `kind`"))
        }
    })?;

    let name = name.ok_or_else(|| {
        syn::Error::new_spanned(
            attribute,
            "an action needs an explicit `name = \"...\"` for binding files",
        )
    })?;
    if !is_valid_action_name(&name.value()) {
        return Err(syn::Error::new_spanned(
            &name,
            "an action name is lowercase letters, digits, and underscores, starting with a letter",
        ));
    }
    let kind = kind.ok_or_else(|| {
        syn::Error::new_spanned(attribute, "an action needs a `kind`: `Button` or `Axis2d`")
    })?;
    if !KINDS.iter().any(|known| kind == known) {
        return Err(syn::Error::new_spanned(
            &kind,
            "`kind` must be `Button` or `Axis2d`",
        ));
    }

    let ident = &input.ident;
    Ok(quote! {
        impl ::sfeg::input::Action for #ident {
            const NAME: &'static str = #name;
            type Kind = ::sfeg::input::#kind;
        }
    })
}

/// Whether `name` is lowercase letters, digits, and underscores, starting
/// with a letter. Matches the check in `ActionRegistry::register`.
fn is_valid_action_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}
