use raiko_host::server::api::create_docs;

fn main() {
    let docs = create_docs();
    let json = docs.to_json().expect("Failed to serialize docs to json");
    println!("{json}");
}
