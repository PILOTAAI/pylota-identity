#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| pylota_identity_core::fuzz::mime_parse(data));
