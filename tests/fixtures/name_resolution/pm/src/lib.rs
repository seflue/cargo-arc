use proc_macro::TokenStream;

#[proc_macro_derive(Shadow)]
pub fn derive_shadow(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}

#[proc_macro_derive(Shadow2)]
pub fn derive_shadow2(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}

#[proc_macro_derive(Marker, attributes(marker))]
pub fn derive_marker(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}

#[proc_macro_derive(Other)]
pub fn derive_other(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}
