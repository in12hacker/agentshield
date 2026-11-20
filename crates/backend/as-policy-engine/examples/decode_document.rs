//! Optional offline schema check. Rule adoption remains the policy owner's job.
use as_policy_engine::model::PolicyDocument;
use std::{env, error::Error, fs, io};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let path = args
        .next()
        .ok_or_else(|| io::Error::other("expected policy JSON path"))?;
    if args.next().is_some() {
        return Err(io::Error::other("expected one policy JSON path").into());
    }
    let document: PolicyDocument = serde_json::from_slice(&fs::read(path)?)?;
    println!(
        "PolicyDocument schema decoded: {} groups; adoption not executed",
        document.groups.len()
    );
    Ok(())
}
