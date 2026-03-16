pub mod active_rule {
    include!(env!("MOCK_RULE_FILE"));
}

pub use active_rule::handle_shasta_request;
