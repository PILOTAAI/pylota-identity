//! `deploy`, `upgrade` and `wrangler`.

use crate::cli::{DeployArgs, UpgradeArgs, WranglerArgs};
use crate::{CliError, Ctx, Output};

pub async fn deploy(_ctx: &Ctx, _a: DeployArgs) -> Result<Output, CliError> {
    Err(CliError::internal("deploy is not built yet"))
}

pub async fn upgrade(_ctx: &Ctx, _a: UpgradeArgs) -> Result<Output, CliError> {
    Err(CliError::internal("upgrade is not built yet"))
}

pub async fn wrangler(_ctx: &Ctx, _a: WranglerArgs) -> Result<Output, CliError> {
    Err(CliError::internal("wrangler is not built yet"))
}
