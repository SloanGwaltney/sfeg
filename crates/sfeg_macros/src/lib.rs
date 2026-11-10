//! SFEG's macros. Games use them through the `sfeg` crate, such as
//! `sfeg::input::Action` and `sfeg::scene_file_object`.

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

/// Lets scene files name a game object type (ADR 0014, 0026):
///
/// ```ignore
/// #[sfeg::scene_file_object(name = "Door")]
/// struct Door {
///     walker: Handle<Walker>,
/// }
/// ```
///
/// It derives `Deserialize` for the type through `sfeg::serde`, so the
/// type's fields are what a scene file writes under `data`, and
/// `#[serde(...)]` attributes work on them. It implements
/// `SceneFileObject` with `name`, which is never taken from the type name,
/// so renaming the type never breaks files. Register the type with
/// `SceneTypeRegistry::register_object`.
///
/// Put it above any `#[derive(...)]` on the type.
#[proc_macro_attribute]
pub fn scene_file_object(attribute: TokenStream, item: TokenStream) -> TokenStream {
    expand_scene_file_type(attribute, item, quote!(::sfeg::scene_file::SceneFileObject))
}

/// Lets scene files name a scene state type (ADR 0014, 0026):
///
/// ```ignore
/// #[sfeg::scene_file_state(name = "Level")]
/// struct LevelState {
///     level: Level,
/// }
/// ```
///
/// It derives `Deserialize` for the type through `sfeg::serde`, so the
/// type's fields are what a scene file writes under the scene state's
/// `data`, and `#[serde(...)]` attributes work on them. It implements
/// `SceneFileState` with `name`, which is never taken from the type name.
/// Register the type with `SceneTypeRegistry::register_scene_state`.
///
/// Put it above any `#[derive(...)]` on the type.
#[proc_macro_attribute]
pub fn scene_file_state(attribute: TokenStream, item: TokenStream) -> TokenStream {
    expand_scene_file_type(attribute, item, quote!(::sfeg::scene_file::SceneFileState))
}

fn expand_scene_file_type(
    attribute: TokenStream,
    item: TokenStream,
    naming_trait: proc_macro2::TokenStream,
) -> TokenStream {
    let mut name: Option<LitStr> = None;
    let name_parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("name") {
            if name.is_some() {
                return Err(meta.error("`name` is given twice"));
            }
            name = Some(meta.value()?.parse()?);
            Ok(())
        } else {
            Err(meta.error("expected `name = \"...\"`"))
        }
    });
    parse_macro_input!(attribute with name_parser);
    let input = parse_macro_input!(item as DeriveInput);

    let result = (|| {
        let Some(name) = name else {
            return Err(syn::Error::new(
                Span::call_site(),
                "a scene file type needs an explicit `name = \"...\"` for scene files",
            ));
        };
        if !is_valid_type_name(&name.value()) {
            return Err(syn::Error::new_spanned(
                &name,
                "a scene file type name is ASCII letters, digits, and underscores, starting with a letter",
            ));
        }
        if !input.generics.params.is_empty() {
            return Err(syn::Error::new_spanned(
                &input.generics,
                "a scene file type can't be generic: each name is one concrete type",
            ));
        }
        let ident = &input.ident;
        Ok(quote! {
            #[derive(::sfeg::serde::Deserialize)]
            #[serde(crate = "::sfeg::serde")]
            #input

            impl #naming_trait for #ident {
                const NAME: &'static str = #name;
            }
        })
    })();
    match result {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Whether `name` is ASCII letters, digits, and underscores, starting with
/// a letter, such as `"Door"` or `"level_2_boss"`.
fn is_valid_type_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Lets scene files hold a plain data type inside an object's or scene
/// state's fields, such as an enum (ADR 0026):
///
/// ```ignore
/// #[sfeg::scene_file_data]
/// enum DoorSide {
///     Left,
///     Right,
/// }
/// ```
///
/// It only derives `Deserialize` through `sfeg::serde`, so games don't
/// depend on serde themselves. Unlike `scene_file_object`, the type has no
/// name and can't be registered: it only appears inside another type's
/// `data`.
///
/// Put it above any `#[derive(...)]` on the type.
#[proc_macro_attribute]
pub fn scene_file_data(attribute: TokenStream, item: TokenStream) -> TokenStream {
    if !attribute.is_empty() {
        return syn::Error::new(
            Span::call_site(),
            "`scene_file_data` takes no arguments: the type has no name in scene files",
        )
        .to_compile_error()
        .into();
    }
    let input = parse_macro_input!(item as DeriveInput);
    quote! {
        #[derive(::sfeg::serde::Deserialize)]
        #[serde(crate = "::sfeg::serde")]
        #input
    }
    .into()
}
