//! Work after the response (`ctx.waitUntil`): `last_used_at` writes and metrics flushes. The future
//! logs its own errors.

use core::future::Future;

pub trait Background {
    fn wait_until(&self, fut: impl Future<Output = ()> + 'static);
}
