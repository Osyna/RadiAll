//! Glyph icon library for action icons: an index over user-provided SVG
//! collections (the v1 launcher shipped ~24k monochrome icons from
//! material-symbols / tabler / lucide — they're indexed in place).
//!
//! Sources, in order:
//!   ~/.config/radiall/icons/**/*.svg      (canonical location)
//!   ~/.config/quickshell/icons/**/*.svg   (v1 install, reused untouched)
//!
//! Selecting an icon stores its absolute path in the config, so rendering
//! goes through the ordinary abs-path pipeline (resvg + colorize).

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct IconEntry {
    /// File stem, lowercase (search key + display name).
    pub name: String,
    /// Icon set = parent directory name ("lucide", "tabler", …).
    pub set: String,
    pub path: PathBuf,
}

#[derive(Debug, Default)]
pub struct IconLib {
    /// Sorted by name for stable, alphabetical search results.
    entries: Vec<IconEntry>,
}

fn source_dirs() -> Vec<PathBuf> {
    let cfg = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    vec![cfg.join("radiall/icons"), cfg.join("quickshell/icons")]
}

impl IconLib {
    pub fn scan() -> Self {
        let mut entries = Vec::new();
        for root in source_dirs() {
            let Ok(sets) = std::fs::read_dir(&root) else { continue };
            for set_dir in sets.flatten() {
                let set_path = set_dir.path();
                if set_path.is_file() {
                    // loose SVGs directly in the root belong to set ""
                    push_svg(&mut entries, String::new(), set_path);
                    continue;
                }
                let set = set_dir.file_name().to_string_lossy().to_string();
                let Ok(files) = std::fs::read_dir(&set_path) else { continue };
                for f in files.flatten() {
                    push_svg(&mut entries, set.clone(), f.path());
                }
            }
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.set.cmp(&b.set)));
        log::info!("icon library: {} glyphs indexed", entries.len());
        Self { entries }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Substring search over icon names, prefix matches ranked first,
    /// capped at `cap` results. Empty query -> first `cap` entries.
    pub fn search(&self, query: &str, cap: usize) -> Vec<&IconEntry> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.entries.iter().take(cap).collect();
        }
        let mut prefix = Vec::new();
        let mut contains = Vec::new();
        for e in &self.entries {
            if e.name.starts_with(&q) {
                prefix.push(e);
            } else if e.name.contains(&q) {
                contains.push(e);
            }
            if prefix.len() >= cap {
                break;
            }
        }
        prefix.extend(contains);
        prefix.truncate(cap);
        prefix
    }
}

fn push_svg(entries: &mut Vec<IconEntry>, set: String, path: PathBuf) {
    if path.extension().is_none_or(|e| e != "svg") {
        return;
    }
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return;
    };
    entries.push(IconEntry {
        name: stem.to_lowercase(),
        set,
        path,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib(names: &[&str]) -> IconLib {
        let mut entries: Vec<IconEntry> = names
            .iter()
            .map(|n| IconEntry {
                name: n.to_string(),
                set: "test".into(),
                path: PathBuf::from(format!("/tmp/{n}.svg")),
            })
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        IconLib { entries }
    }

    #[test]
    fn search_ranks_prefix_before_contains() {
        let l = lib(&["arrow-up", "bar-chart", "chart-arrow", "chart-bar"]);
        let hits: Vec<&str> = l.search("chart", 10).iter().map(|e| e.name.as_str()).collect();
        assert_eq!(hits, ["chart-arrow", "chart-bar", "bar-chart"]);
    }

    #[test]
    fn search_caps_and_handles_empty() {
        let l = lib(&["a", "b", "c", "d"]);
        assert_eq!(l.search("", 2).len(), 2);
        assert_eq!(l.search("zzz", 10).len(), 0);
        assert_eq!(l.search("  B ", 10).len(), 1); // trimmed, lowercased
    }
}
