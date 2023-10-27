// Copyright (c) Fortanix, Inc.
//
// Licensed under the GNU General Public License, version 2 <LICENSE-GPL or
// https://www.gnu.org/licenses/gpl-2.0.html> or the Apache License, Version
// 2.0 <LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0>, at your
// option. This file may not be copied, modified, or distributed except
// according to those terms.

#![feature(c_size_t)]

// needed to have common code for `mod support` in unit and integrations tests
extern crate mbedtls;

use core::ffi::{c_int, c_size_t, c_uchar};
use std::{
    fs::File,
    io::{prelude::*, BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
};

use mbedtls::{
    alloc::List as MbedtlsList,
    pk::Pk,
    rng::CtrDrbg,
    ssl::{
        config::{Endpoint, Preset, Transport},
        Config, Context,
    },
    x509::Certificate,
    Result as TlsResult,
};
// #[path = "./support/mod.rs"]
// mod support;
// use support::entropy::entropy_new;
// use support::keys;
// use support::rand::test_rng;

// Define functions exported from Gramine's libra_tls_attest.so (RA-TLS)
#[link(name = "ra_tls_attest")]
extern "C" {
    fn ra_tls_create_key_and_crt_der(
        der_key: *mut *mut c_uchar,
        der_key_size: *mut c_size_t,
        der_crt: *mut *mut c_uchar,
        der_crt_size: *mut c_size_t,
    ) -> c_int;
}

fn listen<E, F: FnMut(TcpStream) -> Result<(), E>>(mut handle_client: F) -> Result<(), E> {
    let sock = TcpListener::bind("127.0.0.1:8080").unwrap();
    for conn in sock.incoming().map(Result::unwrap) {
        println!("Connection from {}", conn.peer_addr().unwrap());
        handle_client(conn)?;
    }

    Ok(())
}

pub fn result_main() -> TlsResult<()> {
    // TODO
    //
    // https://gramine.readthedocs.io/en/latest/devel/debugging.html?highlight=meson#debugging-gramine-with-gdb
    // meson setup build-debug/ --werror  -Ddirect=enabled -Ddcap=enabled -Dsgx=enabled
    // ninja -C build-debug/
    // cargo build --example server-sgx --verbose
    // ubuntu@VM-0-6-ubuntu:~/rust-mbedtls/target/debug/examples$ gramine-manifest
    // -Dlog_level=error -Darch_libdir=/lib/x86_64-linux-gnu/ ./server-sgx.manifest.template
    // ./server-sgx.manifest ubuntu@VM-0-6-ubuntu:~/rust-mbedtls/target/debug/examples$
    // gramine-sgx-sign --manifest ./server-sgx.manifest --output ./server-sgx.manifest.sgx
    // ubuntu@VM-0-6-ubuntu:~/rust-mbedtls/target/debug/examples$ sudo gramine-sgx
    // ./server-sgx ubuntu@VM-0-6-ubuntu:~/gramine$ nm -D
    // ./build-debug/tools/sgx/ra-tls/libra_tls_attest.so ubuntu@VM-0-6-ubuntu:~/
    // rust-mbedtls/target/debug/examples$ gramine-sgx-sigstruct-view  ./server-sgx.sig
    // ubuntu@VM-0-6-ubuntu:~/rust-mbedtls/target/debug/examples$ nm -D
    // /home/ubuntu/rust-mbedtls/mbedtls/examples/libra_tls_attest.so https://github.com/gramineproject/gramine/blob/master/CI-Examples/ra-tls-mbedtls/src/server.c
    // https://stackoverflow.com/questions/40602708/linking-rust-application-with-a-dynamic-library-not-in-the-runtime-linker-search

    // assert /dev/attestation/attestation_type == "dcap"

    if let Ok(mut attestation_type_file) = File::open("/dev/attestation/attestation_type") {
        let mut attestation_type = String::new();
        if let Ok(_) = attestation_type_file.read_to_string(&mut attestation_type) {
            println!("Detected attestation type: {}", attestation_type.trim());
        }
        assert_eq!(attestation_type, "dcap");
    }

    // Seeding the random number generator:
    //  4. ret = mbedtls_ctr_drbg_seed(&ctr_drbg, mbedtls_entropy_func, &entropy, (const
    //     unsigned char*)pers, strlen(pers));

    //     let entropy = entropy_new();  // mbedtls::rng::Rdseed or
    // mbedtls::rng::OsEntropy::new()
    let entropy = mbedtls::rng::OsEntropy::new();
    let rng = Arc::new(CtrDrbg::new(Arc::new(entropy), None)?);

    // Creating the RA-TLS server cert and key:
    //  5. ret = (*ra_tls_create_key_and_crt_der_f)(&der_key, &der_key_size, &der_crt,
    //     &der_crt_size);

    let mut der_key: *mut c_uchar = std::ptr::null_mut();
    let der_key_ptr: *mut *mut c_uchar = &mut der_key;
    let mut der_key_size: c_size_t = 0;
    let mut der_crt: *mut c_uchar = std::ptr::null_mut();
    let der_crt_ptr: *mut *mut c_uchar = &mut der_crt;
    let mut der_crt_size: c_size_t = 0;

    // throws MBEDTLS_ERR_X509_FILE_IO_ERROR (-10496) if you run it without gramine-sgx
    let result = unsafe {
        ra_tls_create_key_and_crt_der(
            der_key_ptr,
            &mut der_key_size,
            der_crt_ptr,
            &mut der_crt_size,
        )
    };

    if result != 0 {
        panic!(
            "Failed to obtain key and certificate data (error code: {})",
            result
        );
    }

    println!("Successfully obtained key and certificate data.");

    // Convert the raw pointers and sizes to slices
    let der_key_slice = unsafe { std::slice::from_raw_parts(der_key, der_key_size as usize) };
    let der_crt_slice = unsafe { std::slice::from_raw_parts(der_crt, der_crt_size as usize) };

    // Print or use the key and certificate data as needed
    println!("DER Key: {:?}", der_key_slice);
    println!("DER Certificate: {:?}", der_crt_slice);

    // Ensure to free the allocated memory in the C function
    // unsafe {
    //     libc::free(der_key_ptr as *mut c_void);
    //     libc::free(der_crt_ptr as *mut c_void);
    // }

    //  6. ret = mbedtls_x509_crt_parse(&srvcert, (unsigned char*)der_crt, der_crt_size);
    let cert = Certificate::from_der(der_crt_slice)?; // generate using libra_tls_attest.so instead (ra_tls_create_key_and_crt_der function)
                                                      // cert.extensions_raw().unwrap()
    println!("raz dwa {}", hex::encode(cert.extensions_raw().unwrap()));
    //  7. ret = mbedtls_pk_parse_key(&pkey, (unsigned char*)der_key, der_key_size,
    //     /*pwd=*/NULL, 0, mbedtls_ctr_drbg_random, &ctr_drbg);

    // let key = Pk::from_private_key(&mut test_rng(), der_key_slice, None)?;
    let key = Pk::from_private_key(der_key_slice, None)?;

    // Bind on https://localhost:4433/:
    //  8. ret = mbedtls_net_bind(&listen_fd, NULL, "4433", MBEDTLS_NET_PROTO_TCP);
    // Setting up the SSL data:
    //  9. ret = mbedtls_ssl_config_defaults(&conf, MBEDTLS_SSL_IS_SERVER,
    //     MBEDTLS_SSL_TRANSPORT_STREAM, MBEDTLS_SSL_PRESET_DEFAULT);
    //  10. mbedtls_ssl_conf_rng(&conf, mbedtls_ctr_drbg_random, &ctr_drbg);
    //  11. mbedtls_ssl_conf_dbg(&conf, my_debug, stdout);
    //  12. ret = mbedtls_ssl_conf_own_cert(&conf, &srvcert, &pkey);
    //  13. ret = mbedtls_ssl_setup(&ssl, &conf);

    let mut config = Config::new(Endpoint::Server, Transport::Stream, Preset::Default);
    config.set_rng(rng);
    let mut cert_list = MbedtlsList::<Certificate>::new();
    cert_list.push(cert);
    let arc_cert_list = Arc::new(cert_list);
    config.push_cert(arc_cert_list, key.into())?;

    let rc_config = Arc::new(config);

    // Waiting for a remote connection:
    //  14. ret = mbedtls_net_accept(&listen_fd, &client_fd, NULL, 0, NULL);
    //  15. mbedtls_ssl_set_bio(&ssl, &client_fd, mbedtls_net_send, mbedtls_net_recv, NULL);

    listen(move |conn| {
        let mut ctx = Context::new(rc_config.clone());
        ctx.establish(conn, None)?;
        let mut session = BufReader::new(ctx);
        let mut line = Vec::new();
        session.read_until(b'\n', &mut line).unwrap();
        let s = String::from_utf8(line.clone()).expect("Found invalid UTF-8");
        println!("result: {}", s);
        session.get_mut().write_all(&line).unwrap();
        Ok(())
    })
}
