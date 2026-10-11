//! Disk-space scan: top directories and files by size.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use shared::{DiskItem, DiskSpace};
use walkdir::WalkDir;

/// Filesystem subtrees that are never meaningful for "disk usage".
const EXCLUDES: &[&str] = &["/proc", "/sys", "/dev", "/run"];

pub fn scan(roots: &[String], depth: usize, top_n: usize) -> DiskSpace {
    let mut dir_sizes: HashMap<String, u64> = HashMap::new();
    let mut top_files: BinaryHeap<Reverse<(u64, String)>> = BinaryHeap::new();

    for root in roots {
        let walker = WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| !is_excluded(entry.path()));
        for entry in walker.flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(metadata) = entry.metadata() else { continue };
            let size = metadata.len();

            if top_files.len() < top_n {
                top_files.push(Reverse((size, entry.path().display().to_string())));
            } else if let Some(Reverse((smallest, _))) = top_files.peek() {
                if size > *smallest {
                    top_files.pop();
                    top_files.push(Reverse((size, entry.path().display().to_string())));
                }
            }

            if let Some(key) = ancestor_at_depth(entry.path(), root, depth) {
                *dir_sizes.entry(key).or_insert(0) += size;
            }
        }
    }

    let mut top_dirs: Vec<DiskItem> =
        dir_sizes.into_iter().map(|(path, bytes)| DiskItem { path, bytes }).collect();
    top_dirs.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.path.cmp(&b.path)));
    top_dirs.truncate(top_n);

    let mut top_files: Vec<DiskItem> = top_files
        .into_iter()
        .map(|Reverse((bytes, path))| DiskItem { path, bytes })
        .collect();
    top_files.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.path.cmp(&b.path)));

    DiskSpace {
        scanned_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        roots: roots.to_vec(),
        top_dirs,
        top_files,
    }
}

fn is_excluded(path: &Path) -> bool {
    EXCLUDES.iter().any(|excluded| path.starts_with(excluded))
}

/// Map a file path to the directory key `depth` levels below `root`.
///
/// e.g. root `/`, depth 2, file `/var/lib/docker/x` -> `/var/lib`.
/// Files sitting directly in `root` return `None`.
fn ancestor_at_depth(path: &Path, root: &str, depth: usize) -> Option<String> {
    let dir = path.parent()?;
    let relative = dir.strip_prefix(root).ok()?;
    let components: Vec<_> = relative.components().collect();
    if components.is_empty() {
        return None;
    }
    let take = depth.min(components.len());
    let mut result = PathBuf::from(root);
    for component in components.iter().take(take) {
        result.push(component);
    }
    Some(result.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_pseudo_filesystems() {
        assert!(is_excluded(Path::new("/proc/1")));
        assert!(is_excluded(Path::new("/sys")));
        assert!(!is_excluded(Path::new("/processes")));
    }

    #[test]
    fn scans_top_dirs_and_files_at_depth() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("var/lib")).unwrap();
        std::fs::create_dir_all(root.join("var/log")).unwrap();
        std::fs::write(root.join("var/lib/big.bin"), vec![0u8; 10_000]).unwrap();
        std::fs::write(root.join("var/lib/small.bin"), vec![0u8; 1_000]).unwrap();
        std::fs::write(root.join("var/log/x.log"), vec![0u8; 5_000]).unwrap();
        std::fs::write(root.join("top.txt"), vec![0u8; 100]).unwrap();

        let scan_result = scan(&[root.display().to_string()], 2, 10);

        let lib = scan_result.top_dirs.iter().find(|d| d.path.ends_with("var/lib")).unwrap();
        assert_eq!(lib.bytes, 11_000);
        let log = scan_result.top_dirs.iter().find(|d| d.path.ends_with("var/log")).unwrap();
        assert_eq!(log.bytes, 5_000);
        assert!(!scan_result.top_dirs.iter().any(|d| d.path.ends_with("top.txt")));
        assert_eq!(scan_result.top_files[0].bytes, 10_000);
        assert!(scan_result.top_files[0].path.ends_with("big.bin"));
        assert_eq!(scan_result.top_files.len(), 4);
    }

    #[test]
    fn truncates_top_files_to_n() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for i in 0..15 {
            std::fs::write(root.join(format!("f{i}.bin")), vec![0u8; (i as usize + 1) * 100]).unwrap();
        }
        let scan_result = scan(&[root.display().to_string()], 2, 10);
        assert_eq!(scan_result.top_files.len(), 10);
        assert_eq!(scan_result.top_files[0].bytes, 1500);
    }
}
