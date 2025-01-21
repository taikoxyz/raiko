use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

#[proc_macro_derive(RedisValue)]
pub fn redis_value_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    let expanded = quote! {
        impl redis::ToRedisArgs for #name {
            fn write_redis_args<W>(&self, out: &mut W)
            where
                W: ?Sized + redis::RedisWrite,
            {
                let serialized = serde_json::to_string(self)
                    .expect(&format!("Failed to serialize {}", stringify!(#name)));
                out.write_arg(serialized.as_bytes());
            }
        }

        impl redis::FromRedisValue for #name {
            fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
                let serialized = String::from_redis_value(v)?;
                serde_json::from_str(&serialized).map_err(|e| {
                    redis::RedisError::from((
                        redis::ErrorKind::TypeError,
                        "Deserialization failed",
                        e.to_string(),
                    ))
                })
            }
        }
    };

    TokenStream::from(expanded)
}
