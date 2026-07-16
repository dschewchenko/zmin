use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use syn::{Expr, ExprArray, ExprLit, File, Item, ItemEnum, Lit, Meta};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let schema_path = manifest_dir.join("../zmin-cli-schema/src/lib.rs");
    println!("cargo:rerun-if-changed={}", schema_path.display());

    let schema_source = fs::read_to_string(&schema_path).expect("read schema source");
    let schema = syn::parse_file(&schema_source).expect("parse schema source");
    let command_enum = find_command_enum(&schema).expect("top-level Command enum");
    let known_commands = collect_known_commands(command_enum);

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("out dir"));
    let generated = render_known_commands(&known_commands);
    fs::write(out_dir.join("known_commands.rs"), generated).expect("write known commands");

    if let Some(rustc_version) = rustc_version_string() {
        println!("cargo:rustc-env=ZMIN_RUSTC_VERSION={rustc_version}");
    }
}

fn rustc_version_string() -> Option<String> {
    let rustc = env::var("RUSTC").ok()?;
    let output = Command::new(rustc).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8(output.stdout).ok()?;
    Some(version.trim().to_owned())
}

fn find_command_enum(schema: &File) -> Option<&ItemEnum> {
    schema.items.iter().find_map(|item| match item {
        Item::Enum(item_enum) if item_enum.ident == "Command" => Some(item_enum),
        _ => None,
    })
}

fn collect_known_commands(command_enum: &ItemEnum) -> Vec<String> {
    let mut names = BTreeSet::new();
    for variant in &command_enum.variants {
        let mut canonical = to_kebab_case(&variant.ident.to_string());
        let mut aliases = Vec::new();
        for attr in &variant.attrs {
            if !attr.path().is_ident("command") {
                continue;
            }
            if let Meta::List(_) = &attr.meta {
                let _ = attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("name") {
                        canonical = parse_lit_str(meta.value()?.parse::<Expr>()?)
                            .expect("command name string");
                    } else if meta.path.is_ident("alias") || meta.path.is_ident("visible_alias") {
                        aliases.push(
                            parse_lit_str(meta.value()?.parse::<Expr>()?)
                                .expect("command alias string"),
                        );
                    } else if meta.path.is_ident("aliases") || meta.path.is_ident("visible_aliases")
                    {
                        aliases.extend(parse_lit_str_array(meta.value()?.parse::<Expr>()?));
                    }
                    Ok(())
                });
            }
        }
        names.insert(canonical);
        names.extend(aliases);
    }
    names.into_iter().collect()
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

fn render_known_commands(commands: &[String]) -> String {
    let mut body = String::from(
        "pub(crate) fn is_known_top_level_command_name(command: &str) -> bool {\n    matches!(command,\n",
    );
    for command in commands {
        body.push_str("        ");
        body.push('"');
        body.push_str(&escape_rust_string(command));
        body.push_str("\" |\n");
    }
    body.push_str("        \"__zmin_known_command_sentinel__\"\n    )\n}\n");
    body
}

fn escape_rust_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(ch),
        }
    }
    out
}
