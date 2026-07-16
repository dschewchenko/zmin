use std::env;
use std::fs;
use std::path::PathBuf;

use quote::{format_ident, quote};
use syn::{Expr, ExprArray, ExprLit, Fields, Item, ItemEnum, Lit, Meta, Variant};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let schema_path = manifest_dir.join("src/lib.rs");
    println!("cargo:rerun-if-changed={}", schema_path.display());

    let source = fs::read_to_string(&schema_path).expect("read CLI schema");
    let file = syn::parse_file(&source).expect("parse CLI schema");
    let command = file
        .items
        .iter()
        .find_map(|item| match item {
            Item::Enum(item) if item.ident == "Command" => Some(item),
            _ => None,
        })
        .expect("Command enum");
    let generated = generate_command_only_parsers(command);
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("out dir"));
    fs::write(out_dir.join("command_only.rs"), generated).expect("write command-only parsers");
}

fn generate_command_only_parsers(command: &ItemEnum) -> String {
    let parsers = command.variants.iter().map(generate_parser);
    let arms = command.variants.iter().map(generate_match_arm);
    let definitions = command.variants.iter().map(generate_command_definition);
    quote! {
        #(#parsers)*

        pub fn parse_top_level_command_only(
            program: String,
            command_args: &[String],
        ) -> Option<Args> {
            let command_name = command_args.first()?.as_str();
            match command_name {
                #(#arms)*
                _ => None,
            }
        }

        pub fn top_level_command_definition() -> clap::Command {
            let mut command = clap::Command::new("zmin")
                .disable_help_flag(true)
                .disable_version_flag(true);
            #(#definitions)*
            command
        }
    }
    .to_string()
}

fn generate_command_definition(variant: &Variant) -> proc_macro2::TokenStream {
    let parser = parser_ident(variant);
    let canonical = canonical_name(variant);
    let aliases = command_names(variant)
        .into_iter()
        .filter(|name| name != &canonical)
        .collect::<Vec<_>>();
    let aliases = if aliases.is_empty() {
        quote! {}
    } else {
        quote! { .aliases([#(#aliases),*]) }
    };
    quote! {
        command = command.subcommand(
            <#parser as clap::CommandFactory>::command()
                .name(#canonical)
                #aliases,
        );
    }
}

fn generate_parser(variant: &Variant) -> proc_macro2::TokenStream {
    let parser = parser_ident(variant);
    let display_name = format!("zmin {}", canonical_name(variant));
    match &variant.fields {
        Fields::Named(fields) => {
            let fields = &fields.named;
            quote! {
                #[derive(clap::Parser, Debug)]
                #[command(
                    name = #display_name,
                    disable_help_flag = true,
                    disable_version_flag = true
                )]
                struct #parser {
                    #fields
                }
            }
        }
        Fields::Unnamed(fields) => {
            assert_eq!(fields.unnamed.len(), 1, "tuple command variant arity");
            let field = fields.unnamed.first().expect("tuple command field");
            let ty = &field.ty;
            quote! {
                #[derive(clap::Parser, Debug)]
                #[command(
                    name = #display_name,
                    disable_help_flag = true,
                    disable_version_flag = true
                )]
                struct #parser {
                    #[command(flatten)]
                    options: #ty,
                }
            }
        }
        Fields::Unit => quote! {
            #[derive(clap::Parser, Debug)]
            #[command(
                name = #display_name,
                disable_help_flag = true,
                disable_version_flag = true
            )]
            struct #parser;
        },
    }
}

fn generate_match_arm(variant: &Variant) -> proc_macro2::TokenStream {
    let parser = parser_ident(variant);
    let variant_ident = &variant.ident;
    let names = command_names(variant);
    let construct = match &variant.fields {
        Fields::Named(fields) => {
            let names = fields
                .named
                .iter()
                .map(|field| field.ident.as_ref().expect("named command field"));
            quote! { Command::#variant_ident { #(#names: parsed.#names),* } }
        }
        Fields::Unnamed(_) => quote! { Command::#variant_ident(parsed.options) },
        Fields::Unit => quote! { Command::#variant_ident },
    };
    quote! {
        #(#names)|* => {
            let parsed = #parser::try_parse_from(
                std::iter::once(format!("{program} {command_name}"))
                    .chain(command_args.iter().skip(1).cloned()),
            )
            .unwrap_or_else(|error| error.exit());
            let _ = &parsed;
            Some(Args { command: #construct })
        }
    }
}

fn parser_ident(variant: &Variant) -> syn::Ident {
    format_ident!("Generated{}OnlyArgs", variant.ident)
}

fn command_names(variant: &Variant) -> Vec<String> {
    let mut names = vec![canonical_name(variant)];
    for attr in &variant.attrs {
        if !attr.path().is_ident("command") {
            continue;
        }
        if let Meta::List(_) = &attr.meta {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("alias") || meta.path.is_ident("visible_alias") {
                    names
                        .push(parse_lit_str(meta.value()?.parse::<Expr>()?).expect("alias string"));
                } else if meta.path.is_ident("aliases") || meta.path.is_ident("visible_aliases") {
                    names.extend(parse_lit_str_array(meta.value()?.parse::<Expr>()?));
                } else if meta.input.peek(syn::Token![=]) {
                    let _ = meta.value()?.parse::<Expr>()?;
                }
                Ok(())
            });
        }
    }
    names.sort();
    names.dedup();
    names
}

fn canonical_name(variant: &Variant) -> String {
    for attr in &variant.attrs {
        if !attr.path().is_ident("command") {
            continue;
        }
        let mut name = None;
        if let Meta::List(_) = &attr.meta {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    name = parse_lit_str(meta.value()?.parse::<Expr>()?);
                } else if meta.input.peek(syn::Token![=]) {
                    let _ = meta.value()?.parse::<Expr>()?;
                }
                Ok(())
            });
        }
        if let Some(name) = name {
            return name;
        }
    }
    to_kebab_case(&variant.ident.to_string())
}

fn parse_lit_str(expr: Expr) -> Option<String> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(value),
            ..
        }) => Some(value.value()),
        _ => None,
    }
}

fn parse_lit_str_array(expr: Expr) -> Vec<String> {
    match expr {
        Expr::Array(ExprArray { elems, .. }) => {
            elems.into_iter().filter_map(parse_lit_str).collect()
        }
        _ => Vec::new(),
    }
}

fn to_kebab_case(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for (index, ch) in value.chars().enumerate() {
        if ch.is_uppercase() {
            if index > 0 {
                out.push('-');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}
