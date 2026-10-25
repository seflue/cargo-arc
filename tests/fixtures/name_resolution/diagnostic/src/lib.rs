use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn on_unimplemented(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
