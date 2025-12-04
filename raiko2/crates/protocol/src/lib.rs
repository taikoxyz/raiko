//! Raiko V2 Protocol Types
//!
//! This crate provides Taiko Shasta protocol types and codecs.
//! These types are compatible with taiko-client-rs and used for:
//!
//! - Decoding Shasta inbox events (Proposed, Proved)
//! - Encoding/decoding derivation source manifests
//! - Block manifest structures for batch proposals
//!
//! ## Usage
//!
//! ```rust,ignore
//! use raiko2_protocol::shasta::{
//!     manifest::DerivationSourceManifest,
//!     codec::decode_proposed_event,
//! };
//!
//! // Decode a proposed event
//! let payload = decode_proposed_event(&event_data)?;
//!
//! // Decompress and decode a manifest
//! let manifest = DerivationSourceManifest::decompress_and_decode(&blob_data, 0)?;
//! ```

pub mod shasta;

pub use shasta::*;
