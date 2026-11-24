use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use rudder_core::Error;
use rudder_node::ops::{BuildCacheEntry, BuildCachePrune, BuildCachePruned, BuildRequest};

use super::{checked, identifier, invalid, node, path_string};

const META: &str = "rudder-cache.json";

fn service_dir(root: &Path, service: &str) -> Result<PathBuf, Error> {
    identifier(service)?;
    checked(&checked(root, "cache")?, service)
}

fn entry_dir(root: &Path, service: &str, key: &str) -> Result<PathBuf, Error> {
    identifier(key)?;
    checked(&service_dir(root, service)?, key)
}

fn read_entry(path: &Path, service: &str, key: &str) -> Result<BuildCacheEntry, Error> {
    let metadata = checked(path, META)?;
    let bytes = fs::read(metadata).map_err(|_| node("could not read build cache metadata"))?;
    let mut entry: BuildCacheEntry =
        serde_json::from_slice(&bytes).map_err(|_| node("invalid build cache metadata"))?;
    if entry.service_id != service || entry.cache_key != key {
        return Err(invalid("build cache metadata does not match its path"));
    }
    entry.size_bytes = size(path)?;
    Ok(entry)
}

fn write_entry(path: &Path, entry: &BuildCacheEntry) -> Result<(), Error> {
    let bytes = serde_json::to_vec(entry).map_err(|_| node("could not encode cache metadata"))?;
    let temp = checked(path, "rudder-cache.json.tmp")?;
    fs::write(&temp, bytes).map_err(|_| node("could not write cache metadata"))?;
    fs::rename(temp, checked(path, META)?).map_err(|_| node("could not publish cache metadata"))
}

fn children(path: &Path) -> Result<Vec<String>, Error> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    fs::read_dir(path)
        .map_err(|_| node("could not list build caches"))?
        .map(|entry| {
            let entry = entry.map_err(|_| node("could not inspect build cache"))?;
            entry
                .file_name()
                .into_string()
                .map_err(|_| invalid("cache path must be UTF-8"))
        })
        .collect()
}

pub(super) fn usage(root: &Path, service: Option<&str>) -> Result<Vec<BuildCacheEntry>, Error> {
    let cache = checked(root, "cache")?;
    let services = match service {
        Some(service) => vec![service.into()],
        None => children(&cache)?,
    };
    let mut result = Vec::new();
    for service in services {
        let directory = service_dir(root, &service)?;
        for key in children(&directory)? {
            if key.starts_with('.') {
                continue;
            }
            let path = entry_dir(root, &service, &key)?;
            result.push(read_entry(&path, &service, &key)?);
        }
    }
    result.sort_by(|a, b| (&a.service_id, &a.cache_key).cmp(&(&b.service_id, &b.cache_key)));
    Ok(result)
}

pub(super) fn prepare(
    root: &Path,
    request: &BuildRequest,
) -> Result<(Vec<String>, PathBuf), Error> {
    let directory = service_dir(root, &request.service_id)?;
    fs::create_dir_all(&directory).map_err(|_| node("could not create build cache directory"))?;
    let mut imports = Vec::new();
    let mut seen = HashSet::new();
    for key in std::iter::once(&request.cache_key).chain(&request.cache_fallback_keys) {
        if !seen.insert(key) {
            continue;
        }
        let path = entry_dir(root, &request.service_id, key)?;
        if !path.exists() {
            continue;
        }
        let mut entry = read_entry(&path, &request.service_id, key)?;
        if !checked(&path, "index.json")?.is_file() {
            return Err(invalid("build cache is missing its OCI index"));
        }
        entry.last_used_at = chrono::Utc::now().timestamp_millis();
        write_entry(&path, &entry)?;
        imports.push(path_string(&path)?);
    }
    let stage = checked(&directory, &format!(".tmp-{}", request.job_id))?;
    if stage.exists() {
        fs::remove_dir_all(&stage).map_err(|_| node("could not remove stale cache export"))?;
    }
    fs::create_dir(&stage).map_err(|_| node("could not prepare cache export"))?;
    Ok((imports, stage))
}

pub(super) fn publish(root: &Path, request: &BuildRequest, stage: &Path) -> Result<(), Error> {
    if !checked(stage, "index.json")?.is_file() {
        return Err(node("builder did not export a local OCI cache"));
    }
    let target = entry_dir(root, &request.service_id, &request.cache_key)?;
    let old = if target.exists() {
        Some(read_entry(
            &target,
            &request.service_id,
            &request.cache_key,
        )?)
    } else {
        None
    };
    let now = chrono::Utc::now().timestamp_millis();
    let entry = BuildCacheEntry {
        service_id: request.service_id.clone(),
        cache_key: request.cache_key.clone(),
        size_bytes: size(stage)?,
        last_used_at: now,
        created_at: old.map_or(now, |entry| entry.created_at),
    };
    write_entry(stage, &entry)?;
    if target.exists() {
        exchange(stage, &target)?;
    } else {
        fs::rename(stage, target).map_err(|_| node("could not publish build cache"))?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn exchange(stage: &Path, target: &Path) -> Result<(), Error> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        stage,
        rustix::fs::CWD,
        target,
        rustix::fs::RenameFlags::EXCHANGE,
    )
    .map_err(|_| node("could not atomically replace build cache"))
}

#[cfg(not(target_os = "linux"))]
fn exchange(_stage: &Path, _target: &Path) -> Result<(), Error> {
    Err(node("atomic builder cache replacement requires Linux"))
}

pub(super) fn prune(
    root: &Path,
    request: &BuildCachePrune,
    active: &HashSet<String>,
) -> Result<BuildCachePruned, Error> {
    if let Some(service) = &request.service_id {
        identifier(service)?;
        if active.contains(service) {
            return Err(node("cannot prune a service while it is building"));
        }
    }
    if let Some(key) = &request.cache_key {
        identifier(key)?;
        if request.service_id.is_none() {
            return Err(invalid("cache key pruning requires a service"));
        }
    }
    let mut entries = usage(root, request.service_id.as_deref())?;
    entries.retain(|entry| {
        request
            .cache_key
            .as_ref()
            .is_none_or(|key| key == &entry.cache_key)
    });
    entries.sort_by(|a, b| {
        (a.last_used_at, &a.service_id, &a.cache_key).cmp(&(
            b.last_used_at,
            &b.service_id,
            &b.cache_key,
        ))
    });
    let mut remaining = entries
        .iter()
        .fold(0u64, |sum, entry| sum.saturating_add(entry.size_bytes));
    let mut reclaimed = 0u64;
    for entry in entries {
        if remaining <= request.limit_bytes.unwrap_or(0) {
            break;
        }
        if active.contains(&entry.service_id) {
            continue;
        }
        let path = entry_dir(root, &entry.service_id, &entry.cache_key)?;
        fs::remove_dir_all(path).map_err(|_| node("could not prune build cache"))?;
        remaining = remaining.saturating_sub(entry.size_bytes);
        reclaimed = reclaimed.saturating_add(entry.size_bytes);
    }
    Ok(BuildCachePruned {
        reclaimed_bytes: reclaimed,
        remaining_bytes: remaining,
        entries: usage(root, None)?,
    })
}

fn size(path: &Path) -> Result<u64, Error> {
    let metadata = fs::symlink_metadata(path).map_err(|_| node("could not measure build cache"))?;
    if metadata.file_type().is_symlink() {
        return Err(invalid("symlinks are not allowed in build caches"));
    }
    if metadata.is_dir() {
        let mut total = 0u64;
        for name in children(path)? {
            // Key contents (blobs/sha256) are generated by BuildKit, not request identifiers.
            let child = path.join(name);
            total = total.saturating_add(size(&child)?);
        }
        Ok(total)
    } else if metadata.is_file() {
        Ok(metadata.len())
    } else {
        Err(invalid("only regular files are allowed in build caches"))
    }
}
