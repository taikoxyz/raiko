extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Item, ItemFn, ItemMod};

#[cfg(any(feature = "sp1", feature = "risc0"))]
use syn::{punctuated::Punctuated, Ident, Path, Token};

// Helper struct to parse input
#[cfg(any(feature = "sp1", feature = "risc0"))]
struct EntryArgs {
    main_entry: Ident,
    test_modules: Option<Punctuated<Path, Token![,]>>,
}

#[cfg(any(feature = "sp1", feature = "risc0"))]
impl syn::parse::Parse for EntryArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let main_entry: Ident = input.parse()?;
        let test_modules: Option<Punctuated<Path, Token![,]>> = if input.peek(Token![,]) {
            input.parse::<Token![,]>()?; // Parse and consume the comma
                                         // Now parse a list of module paths if they are present
            Some(input.parse_terminated(Path::parse, Token![,])?)
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
#[cfg(any(feature = "sp1", feature = "risc0"))]
pub fn entrypoint(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as EntryArgs);
    let main_entry = input.main_entry;
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
                use harness_core::*;
                harness_core::ASSERTION_LOG.get_or_init(|| std::sync::Mutex::new(AssertionLog::new()));
                harness_core::TESTS_SUIT.get_or_init(|| std::sync::Mutex::new(TestSuite::new()));
                #(#injections)*
            }
        };
    }

    let output = if cfg!(feature = "sp1") {
        quote! {
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
        }
    } else if cfg!(feature = "risc0") {
        quote! {
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
        }
    } else {
        quote! {}
    };

    output.into()
}

#[proc_macro]
#[cfg(not(any(feature = "sp1", feature = "risc0")))]
pub fn entrypoint(_input: TokenStream) -> TokenStream {
    quote! {
        mod generated_main {
            #[no_mangle]
            fn main() {
                super::ENTRY()
            }
        }
    }
    .into()
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
            let mut test_suite = harness_core::TESTS_SUIT.get_or_init(
                || std::sync::Mutex::new(harness_core::TestSuite::new())
            );
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
