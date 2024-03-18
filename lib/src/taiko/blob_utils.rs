use c_kzg::{Blob, KzgCommitment, KzgSettings};
use sha2::{Digest, Sha256};
use libflate::zlib::Decoder as zlibDecoder;
use anyhow::Result;
use std::io::Read;

const BLOB_FIELD_ELEMENT_NUM: usize = 4096;
const BLOB_FIELD_ELEMENT_BYTES: usize = 32;
const BLOB_DATA_CAPACITY: usize = BLOB_FIELD_ELEMENT_NUM * BLOB_FIELD_ELEMENT_BYTES;
const BLOB_VERSION_OFFSET: usize = 1;
const BLOB_ENCODING_VERSION: u8 = 0;
const MAX_BLOB_DATA_SIZE: usize = (4 * 31 + 3) * 1024 - 4;

const VERSIONED_HASH_VERSION_KZG: u8 = 1u8;

pub fn kzg_to_versioned_hash(commitment: c_kzg::KzgCommitment) -> [u8; 32] {
    let mut res = Sha256::digest(commitment.as_slice());
    res[0] = VERSIONED_HASH_VERSION_KZG;
    res.into()
}

pub fn decode_blob_hex_string(blob_str: &str) -> Vec<u8> {
    let blob_buf: Vec<u8> = match hex::decode(blob_str.to_lowercase().trim_start_matches("0x")) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };
    decode_blob_data(&blob_buf)
}

// decoding https://github.com/ethereum-optimism/optimism/blob/develop/op-service/eth/blob.go
pub fn decode_blob_data(blob_bytes: &Vec<u8>) -> Vec<u8> {
    let blob_buf = blob_bytes.as_slice();
    // check the version
    if blob_buf[BLOB_VERSION_OFFSET] != BLOB_ENCODING_VERSION {
        return Vec::new();
    }

    // decode the 3-byte big-endian length value into a 4-byte integer
    let output_len =
        ((blob_buf[2] as u32) << 16 | (blob_buf[3] as u32) << 8 | (blob_buf[4] as u32)) as usize;
    if output_len > MAX_BLOB_DATA_SIZE {
        return Vec::new();
    }

    // round 0 is special cased to copy only the remaining 27 bytes of the first field element
    // into the output due to version/length encoding already occupying its first 5 bytes.
    let mut output = [0; MAX_BLOB_DATA_SIZE];
    output[0..27].copy_from_slice(&blob_buf[5..32]);

    // now process remaining 3 field elements to complete round 0
    let mut opos: usize = 28; // current position into output buffer
    let mut ipos: usize = 32; // current position into the input blob
    let mut encoded_byte: [u8; 4] = [0; 4]; // buffer for the 4 6-bit chunks
    encoded_byte[0] = blob_buf[0];
    for encoded_byte_i in encoded_byte.iter_mut().skip(1) {
        (*encoded_byte_i, opos, ipos) =
            match decode_field_element(&blob_buf, opos, ipos, &mut output) {
                Ok(res) => res,
                Err(_) => return Vec::new(),
            }
    }
    opos = reassemble_bytes(opos, &encoded_byte, &mut output);

    // in each remaining round we decode 4 field elements (128 bytes) of the input into 127
    // bytes of output
    for _ in 1..1024 {
        if opos < output_len {
            for encoded_byte_j in &mut encoded_byte {
                // save the first byte of each field element for later re-assembly
                (*encoded_byte_j, opos, ipos) =
                    match decode_field_element(&blob_buf, opos, ipos, &mut output) {
                        Ok(res) => res,
                        Err(_) => return Vec::new(),
                    }
            }
            opos = reassemble_bytes(opos, &encoded_byte, &mut output)
        }
    }
    for otailing in output.iter().skip(output_len) {
        if *otailing != 0 {
            return Vec::new();
        }
    }
    for itailing in blob_buf.iter().take(BLOB_DATA_CAPACITY).skip(ipos) {
        if *itailing != 0 {
            return Vec::new();
        }
    }
    output[0..output_len].to_vec()
}

fn decode_field_element(
    b: &[u8],
    opos: usize,
    ipos: usize,
    output: &mut [u8],
) -> Result<(u8, usize, usize)> {
    // two highest order bits of the first byte of each field element should always be 0
    if b[ipos] & 0b1100_0000 != 0 {
        return Err(anyhow::anyhow!(
            "ErrBlobInvalidFieldElement: field element: {}",
            ipos
        ));
    }
    // copy(output[opos:], b[ipos+1:ipos+32])
    output[opos..opos + 31].copy_from_slice(&b[ipos + 1..ipos + 32]);
    Ok((b[ipos], opos + 32, ipos + 32))
}

fn reassemble_bytes(
    opos: usize,
    encoded_byte: &[u8; 4],
    output: &mut [u8; MAX_BLOB_DATA_SIZE],
) -> usize {
    // account for fact that we don't output a 128th byte
    let opos = opos - 1;
    let x = (encoded_byte[0] & 0b0011_1111) | ((encoded_byte[1] & 0b0011_0000) << 2);
    let y = (encoded_byte[1] & 0b0000_1111) | ((encoded_byte[3] & 0b0000_1111) << 4);
    let z = (encoded_byte[2] & 0b0011_1111) | ((encoded_byte[3] & 0b0011_0000) << 2);
    // put the re-assembled bytes in their appropriate output locations
    output[opos - 32] = z;
    output[opos - (32 * 2)] = y;
    output[opos - (32 * 3)] = x;
    opos
}

pub fn zlib_decompress_blob(blob: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = zlibDecoder::new(blob)?;
    let mut decoded_buf = Vec::new();
    decoder.read_to_end(&mut decoded_buf)?;
    Ok(decoded_buf)
}

pub const KZG_TRUST_SETUP_DATA: &[u8] = include_bytes!("../../../kzg_settings_raw.bin");

// hold data to avoid drop
pub struct KzgSettingsHolder(pub KzgSettings, Vec<u8>);

// TODO: lazy_static?
pub fn get_kzg_settings() -> KzgSettingsHolder {
    let mut data = Vec::<u8>::from(KZG_TRUST_SETUP_DATA);
    KzgSettingsHolder(KzgSettings::from_u8_slice(&mut data), data)
}

pub fn calc_hex_blob_versioned_hash(blob_str: &str) -> [u8; 32] {
    let blob_bytes: Vec<u8> =
        hex::decode(blob_str.to_lowercase().trim_start_matches("0x")).unwrap();
    calc_blob_versioned_hash(&blob_bytes)
}

pub fn calc_blob_versioned_hash(blob_bytes: &[u8]) -> [u8; 32] {
    let blob = Blob::from_bytes(&blob_bytes).unwrap();
    let kzg_settings_holder = get_kzg_settings();
    let kzg_settings = kzg_settings_holder.0;
    let kzg_commit = KzgCommitment::blob_to_kzg_commitment(&blob, &kzg_settings).unwrap();
    let version_hash: [u8; 32] = kzg_to_versioned_hash(kzg_commit);
    version_hash
}