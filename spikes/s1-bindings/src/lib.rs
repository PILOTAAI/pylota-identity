//! Spike S1's Worker: the entry glue exported from a crate that does not depend on `worker`, exactly
//! as `crates/worker` does, with the spike's logic in `pylota_identity_platform::cf::spike_s1`.
//! Deployed by `spikes/s1-bindings/run.sh`; see `RESULT.md`.
#![forbid(unsafe_code)]

#[cfg(target_arch = "wasm32")]
pylota_identity_platform::export_worker! {
    app: pylota_identity_platform::cf::spike_s1::SpikeApp,
    durable_objects: {
        SpikeObject => pylota_identity_platform::cf::spike_s1::SpikeObject,
    }
}
