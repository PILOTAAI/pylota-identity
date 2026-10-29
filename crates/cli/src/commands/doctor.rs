//! `doctor`.

use crate::cli::DoctorArgs;
use crate::{CliError, Ctx, Output};

pub async fn run(_ctx: &Ctx, _a: DoctorArgs) -> Result<Output, CliError> {
    Err(CliError::internal("doctor is not built yet"))
}
