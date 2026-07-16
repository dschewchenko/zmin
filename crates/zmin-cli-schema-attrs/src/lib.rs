use proc_macro::TokenStream;

#[proc_macro_derive(CliSchema, attributes(arg, command))]
pub fn derive_cli_schema(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}
