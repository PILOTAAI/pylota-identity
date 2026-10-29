//! `setup` and `domains subscribe`.

use crate::cli::SetupArgs;
use crate::{CliError, Ctx, Output};

pub async fn run(_ctx: &Ctx, _a: SetupArgs) -> Result<Output, CliError> {
    Err(CliError::internal("setup is not built yet"))
}

pub async fn domains_subscribe(_ctx: &Ctx, _domain: &str, _dir: &str) -> Result<Output, CliError> {
    Err(CliError::internal("domains subscribe is not built yet"))
}
