//! Log lines: `console.log`/`console.error` on wasm32 (Workers Logs), captured in memory natively so
//! tests can assert on them. The worker's `log` module builds the typed line; nothing here formats.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    static LINES: std::cell::RefCell<Vec<(Stream, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Writes one line.
pub fn write(stream: Stream, line: &str) {
    #[cfg(target_arch = "wasm32")]
    match stream {
        Stream::Out => worker::console_log!("{line}"),
        Stream::Err => worker::console_error!("{line}"),
    }
    #[cfg(not(target_arch = "wasm32"))]
    LINES.with(|l| l.borrow_mut().push((stream, line.to_owned())));
}

/// The lines written on this thread so far, removed (native only).
#[cfg(not(target_arch = "wasm32"))]
pub fn take() -> Vec<(Stream, String)> {
    LINES.with(|l| std::mem::take(&mut *l.borrow_mut()))
}
