use raiko_host::server::api::create_docs;
use utoipa_scalar::Scalar;

fn main() {
    println!("{}", Scalar::new(create_docs()).to_html());
}
