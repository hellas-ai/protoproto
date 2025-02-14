use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DataEnum, DeriveInput, Fields};

#[proc_macro_derive(ModelState)]
pub fn model_state_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;

    match input.data {
        Data::Struct(s) => {
            if let Fields::Named(fields) = s.fields {
                let mut state_code = quote! { panic!("Unsupported type") };
                let mut state_mut_code = state_code.clone();

                for field in fields.named.iter() {
                    let field_name = &field.ident;
                    let prev_state_code = state_code;
                    let prev_state_mut_code = state_mut_code;

                    state_code = quote! {
                        <dyn Any>::downcast_ref::<T>(&self.#field_name).unwrap_or_else(|| #prev_state_code)
                    };

                    state_mut_code = quote! {
                        <dyn Any>::downcast_mut::<T>(&mut self.#field_name).unwrap_or_else(|| #prev_state_mut_code)
                    };
                }

                let expanded = quote! {
                    impl ModelState for #name {
                        fn state<T: 'static + Any>(&self) -> &T {
                            #state_code
                        }

                        fn state_mut<T: 'static + Any>(&mut self) -> &mut T {
                            #state_mut_code
                        }
                    }
                };

                return TokenStream::from(expanded);
            }
        }
        Data::Enum(DataEnum { variants, .. }) => {
            let state_arms = variants
                .iter()
                .map(|variant| {
                    let variant_ident = &variant.ident;
                    quote! {
                        #name::#variant_ident(inner) => inner.state()
                    }
                })
                .collect::<Vec<_>>();

            let state_mut_arms = variants
                .iter()
                .map(|variant| {
                    let variant_ident = &variant.ident;
                    quote! {
                        #name::#variant_ident(inner) => inner.state_mut()
                    }
                })
                .collect::<Vec<_>>();

            let expanded = quote! {
                impl ModelState for #name {
                    fn state<T: 'static + Any>(&self) -> &T {
                        match self {
                            #(#state_arms),*
                        }
                    }
                    fn state_mut<T: 'static + Any>(&mut self) -> &mut T {
                        match self {
                            #(#state_mut_arms),*
                        }
                    }
                }
            };

            return TokenStream::from(expanded);
        }
        _ => {}
    }

    panic!("ModelState can only be derived for structs with named fields or enums with tuple variants that have one field.")
}
