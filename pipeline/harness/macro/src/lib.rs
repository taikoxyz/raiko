extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, punctuated::Punctuated, Ident, Item, ItemFn, ItemMod, Path, Token};

#[proc_macro]
pub fn entrypoint(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as EntryArgs);
    let _main_entry = input.main_entry;
    let mut tests_entry = quote! {
        fn run_tests() {}
    };
    if let Some(test_modules) = input.test_modules {
        let injections = test_modules.iter().map(|path| {
            quote! {
                #[cfg(test)]
                #path::inject();
            }
        });
        tests_entry = quote! {
            fn run_tests() {
                #(#injections)*
            }
        };
    }

    #[cfg(feature = "sp1")]
    let output = quote! {

        // Set up a global allocator
        use sp1_zkvm::heap::SimpleAlloc;
        #[global_allocator]
        static HEAP: SimpleAlloc = SimpleAlloc;

        #[cfg(test)]
        #tests_entry

        #[cfg(not(test))]
        const ZKVM_ENTRY: fn() = #main_entry;
        #[cfg(test)]
        const ZKVM_ENTRY: fn() = run_tests;

        mod zkvm_generated_main {
            #[no_mangle]
            fn main() {
                super::ZKVM_ENTRY()
            }
        }

    };

    #[cfg(feature = "risc0")]
    let output = quote! {

        #[cfg(test)]
        #tests_entry

        #[cfg(not(test))]
        const ZKVM_ENTRY: fn() = #main_entry;
        #[cfg(test)]
        const ZKVM_ENTRY: fn() = run_tests;

        mod zkvm_generated_main {
            #[no_mangle]
            fn main() {
                super::ZKVM_ENTRY()
            }
        }

    };

    #[cfg(all(not(feature = "sp1"), not(feature = "risc0")))]
    let output = quote! {
        mod generated_main {
            #[no_mangle]
            fn main() {
                super::ENTRY()
            }
        }
    };

    output.into()
}

// Helper struct to parse input
struct EntryArgs {
    main_entry: Ident,
    test_modules: Option<Punctuated<Path, Token![,]>>,
}

impl syn::parse::Parse for EntryArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let main_entry: Ident = input.parse()?;
        let test_modules: Option<Punctuated<Path, Token![,]>> = if input.peek(Token![,]) {
            input.parse::<Token![,]>()?; // Parse and consume the comma
                                         // Now parse a list of module paths if they are present
            Some(input.parse_terminated(Path::parse)?)
        } else {
            None
        };

        Ok(EntryArgs {
            main_entry,
            test_modules,
        })
    }
}

#[proc_macro]
pub fn zk_suits(input: TokenStream) -> TokenStream {
    let mut input_module = parse_macro_input!(input as ItemMod);
    let mod_content = &mut input_module.content.as_mut().unwrap().1;

    // Inject additional imports
    let assert_import = syn::parse_quote! {
        use harness_core::{assert_eq, assert};
    };
    let harness_import = syn::parse_quote! {
        use harness_core::*;
    };
    mod_content.insert(0, Item::Use(assert_import));
    mod_content.insert(0, Item::Use(harness_import));

    // Collect test function names
    let test_fns = mod_content
        .iter()
        .filter_map(|item| {
            if let syn::Item::Fn(fn_item) = item {
                Some(fn_item.sig.ident.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    // Generate registration blocks for each test function
    let registration_blocks = test_fns.iter().map(|ident| {
        let test_ident = ident;
        let stringified_name = ident.to_string();
        quote! {
            #[cfg(test)]
            test_suite.lock().unwrap().add_test(#stringified_name, #test_ident);
        }
    });

    // Create the `inject` function inside the module
    let inject_fn = quote! {
        pub fn inject() {
            let mut test_suite = harness_core::TESTS_SUITE.get().unwrap();
            #(#registration_blocks)*
        }
    };

    // Parse the `inject` function and add it to the module content
    let inject_item_fn: ItemFn = syn::parse2(inject_fn).unwrap();
    mod_content.push(Item::Fn(inject_item_fn));

    // Generate the final output
    let output = quote! {
        #[allow(unused_imports)]
        #[allow(clippy::unused_imports)]
        #input_module
    };

    TokenStream::from(output)
}
