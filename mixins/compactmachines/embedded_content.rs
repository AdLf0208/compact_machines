// compactmachines is one file on the registry: its content (the machine, the
// walls, the PSD, the modules and their recipes) is the content mod in this
// bundle's `data/` (compactmachines-content.hmod, built from content/ by
// tools/package.sh). It is written out once and loaded exactly as a dev mod
// is, so it is on whenever this native mod is built in. An installed copy of
// the mod wins, as the first read.
fn compactmachines_embedded_content(
    root: &std::path::Path,
    enabled: &std::collections::BTreeSet<String>,
    dev: &[std::path::PathBuf],
) -> core::ops::ControlFlow<std::path::PathBuf> {
    static CONTENT: &[u8] =
        horizon_client::native_data!("compactmachines", "compactmachines-content.hmod");
    let dir = std::env::current_dir()
        .unwrap_or_default()
        .join(".horizon-cache");
    let file = dir.join("compactmachines-embedded-content.hmod");
    log::info!("[compactmachines] content_root: dev len {} file {}", dev.len(), file.display());
    if dev.contains(&file) {
        log::info!("[compactmachines] embedded content already in the dev list");
        return core::ops::ControlFlow::Continue(());
    }
    let same = std::fs::read(&file).is_ok_and(|on_disk| on_disk == CONTENT);
    log::info!("[compactmachines] embedded content: dir {} exists {} same {same}", dir.display(), dir.exists());
    if !same
        && std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(&file, CONTENT))
            .is_err()
    {
        log::warn!(
            "Horizon: Compact Machines' content could not be written to {}",
            file.display()
        );
        return core::ops::ControlFlow::Continue(());
    }
    log::info!("[compactmachines] embedded content ready at {}", file.display());
    let mut with = dev.to_vec();
    with.push(file);
    core::ops::ControlFlow::Break(content_root(root, enabled, &with))
}
