//! The `req_` ID, generated at the start of every invocation and carried through internal calls and
//! logs (Design conventions § 3).

use pylota_identity_platform::ids::IdPrefix;
use pylota_identity_platform::{Ids as _, Platform};

pub fn new<P: Platform>(p: &P) -> String {
    p.ids().new_id(IdPrefix::Request)
}
