use raiko_host::server::api::create_docs;
use utoipa_scalar::Scalar;

fn main() {
    let docs = create_docs();
    let scalar = Scalar::new(docs);
    let html = scalar.to_html();
    println!("{html}");
}
