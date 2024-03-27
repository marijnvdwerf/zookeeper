use anyhow::Result;
use vergen::EmitBuilder;

pub fn main() -> Result<()> {
    EmitBuilder::builder().all_build().all_rustc().emit()?;
    Ok(())
}
