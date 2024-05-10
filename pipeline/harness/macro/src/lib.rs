extern crate proc_macro;
use proc_macro::{TokenStream};
use quote::{quote};
use syn::{parse_macro_input, punctuated::Punctuated, Item, ItemFn, ItemMod, Path, Token};

#[proc_macro]
pub fn entrypoint(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as EntryPointInput);
    let main_fn = input.main_fn;
    let module_paths = input.module_paths;

    let injections = module_paths.iter().map(|path| {
        quote! {
            #[cfg(test)]
            #path::inject();
        }
    });

    let run_tests_fn = quote! {
        fn run_tests() {
            #(#injections)*
        }
    };

    let output = quote! {

        // Set up a global allocator
        use sp1_zkvm::heap::SimpleAlloc;
        #[global_allocator]
        static HEAP: SimpleAlloc = SimpleAlloc;

        #[cfg(test)]
        #run_tests_fn

        #[cfg(not(test))]
        const ZKVM_ENTRY: fn() = #main_fn;
        #[cfg(test)]
        const ZKVM_ENTRY: fn() = run_tests;

        mod zkvm_generated_main {
            #[no_mangle]
            fn main() {
                super::ZKVM_ENTRY()
            }
        }

    };

    output.into()
}

// Custom parsing for the macro input
struct EntryPointInput {
    main_fn: syn::Ident,
    module_paths: Punctuated<Path, Token![,]>,
}

impl syn::parse::Parse for EntryPointInput {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let main_fn: syn::Ident = input.parse()?;
        input.parse::<Token![,]>()?;
        let module_paths = input.parse_terminated(Path::parse)?;

        Ok(EntryPointInput {
            main_fn,
            module_paths,
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
    mod_content.push(Item::Use(assert_import));
    mod_content.push(Item::Use(harness_import));

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
