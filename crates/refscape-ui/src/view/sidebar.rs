//! Sidebar projections invalidate only for catalog or query changes.
use std::{path::PathBuf, sync::Arc};
#[derive(Default)]
pub(super) struct SidebarCache {
    normalized: Vec<(String, PathBuf)>,
    query: Option<String>,
    filtered: Arc<Vec<PathBuf>>,
}
impl SidebarCache {
    pub(super) fn catalog(&mut self, files: &[PathBuf]) {
        self.normalized = files
            .iter()
            .map(|path| (path.to_string_lossy().to_lowercase(), path.clone()))
            .collect();
        self.query = None;
    }
    pub(super) fn filter(&mut self, query: &str) -> Arc<Vec<PathBuf>> {
        if self.query.as_deref() != Some(query) {
            let lower = query.to_lowercase();
            self.filtered = Arc::new(
                self.normalized
                    .iter()
                    .filter(|(key, _)| lower.is_empty() || key.contains(&lower))
                    .map(|(_, path)| path.clone())
                    .collect(),
            );
            self.query = Some(query.to_owned());
        }
        self.filtered.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::SidebarCache;
    use std::sync::Arc;
    #[test]
    fn camera_frames_reuse_catalog_query_projection_and_new_catalog_invalidates_it() {
        let mut cache = SidebarCache::default();
        cache.catalog(&["日本/sample.rs".into(), "Elsewhere.ts".into()]);
        let first = cache.filter("SAMPLE");
        assert_eq!(first.len(), 1);
        assert!(Arc::ptr_eq(&first, &cache.filter("SAMPLE")));
        cache.catalog(&["new/sample.rs".into()]);
        let next = cache.filter("SAMPLE");
        assert!(!Arc::ptr_eq(&first, &next));
        assert_eq!(next[0], std::path::PathBuf::from("new/sample.rs"));
    }
}
