//! Local analysis of verified Agent evidence archives.
#![forbid(unsafe_code)]

mod archive;
mod graph;
mod openclaw;

use std::{collections::BTreeSet, env, ffi::OsStr, fs, io, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<PathBuf> = env::args_os().skip(1).map(PathBuf::from).collect();
    if let [mode, gateway, registry] = paths.as_slice()
        && mode.as_os_str() == OsStr::new("--openclaw-handoff")
    {
        let gateway = fs::read_to_string(gateway)?;
        let registry = fs::read_to_string(registry)?;
        let result = openclaw::analyze(&gateway, &registry)?;
        serde_json::to_writer_pretty(io::stdout().lock(), &result)?;
        return Ok(());
    }
    if let [mode, capture, database] = paths.as_slice()
        && mode.as_os_str() == OsStr::new("--a2a")
    {
        let input = fs::read_to_string(capture)?;
        let result = as_middleware::agent_graph::analyze_archive(&input, database)?;
        serde_json::to_writer_pretty(io::stdout().lock(), &result)?;
        return Ok(());
    }
    if paths.is_empty() {
        return Err(
            io::Error::other("usage: as-agent-graph <result.json> [<result.json> ...] or --a2a <capture.json> <business.sqlite> or --openclaw-handoff <gateway.jsonl> <registry.json>").into(),
        );
    }
    let inputs = paths
        .iter()
        .map(fs::read_to_string)
        .collect::<Result<Vec<_>, _>>()?;
    let archives = inputs
        .iter()
        .zip(&paths)
        .map(|(input, path)| {
            serde_json::from_str::<archive::Archive<'_>>(input)
                .map_err(|error| io::Error::other(format!("{}: {error}", path.display())))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut seen = BTreeSet::new();
    let mut sources = Vec::with_capacity(archives.len());
    let mut coverage = graph::Coverage::default();
    for (archive, path) in archives.iter().zip(&paths) {
        if !seen.insert(&archive.runtime.capture) {
            return Err(
                io::Error::other(format!("{}: duplicate capture source", path.display())).into(),
            );
        }
        let view = graph::analyze(path, archive)
            .map_err(|error| io::Error::other(format!("{}: {error}", path.display())))?;
        coverage.add(view.coverage);
        sources.push(view);
    }
    let stdout = io::stdout().lock();
    serde_json::to_writer_pretty(stdout, &graph::ReportView { sources, coverage })?;
    Ok(())
}
