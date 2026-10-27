#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| pylota_identity_core::fuzz::dsn_parse(data));
