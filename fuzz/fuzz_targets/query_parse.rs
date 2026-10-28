#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| pylota_identity_core::fuzz::query_parse(data));
