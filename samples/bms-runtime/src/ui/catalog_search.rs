//! Cached title/artist substring search, preserving original catalog identities.
use super::selection::SelectionItem;
use std::sync::Arc;

pub const MAX_QUERY_BYTES: usize = 256;

pub struct CatalogSearch {
    text: Vec<String>,
    query: String,
    indices: Arc<[usize]>,
    cursor: Option<usize>,
}
impl CatalogSearch {
    pub fn new(items: &[SelectionItem]) -> Result<Self, String> {
        items
            .len()
            .checked_add(100)
            .and_then(|count| u64::try_from(count).ok())
            .ok_or("catalog control identity overflow")?;
        Ok(Self {
            text: items
                .iter()
                .map(|item| {
                    format!(
                        "{} {}",
                        item.title.to_lowercase(),
                        item.artist.to_lowercase()
                    )
                })
                .collect(),
            query: String::new(),
            indices: (0..items.len()).collect::<Vec<_>>().into(),
            cursor: (!items.is_empty()).then_some(0),
        })
    }
    /// Unicode lowercase, without normalization or full case folding. Tokens
    /// must all occur in the combined title and artist text.
    pub fn set_query(&mut self, query: &str) -> Result<(), String> {
        if query.len() > MAX_QUERY_BYTES
            || query
                .chars()
                .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
        {
            return Err("search query exceeds 256 bytes or contains control characters".into());
        }
        if self.query == query {
            return Ok(());
        }
        let lowered = query.to_lowercase();
        let tokens = lowered.split_whitespace().collect::<Vec<_>>();
        let indices: Arc<[usize]> = self
            .text
            .iter()
            .enumerate()
            .filter_map(|(index, text)| {
                tokens
                    .iter()
                    .all(|token| text.contains(*token))
                    .then_some(index)
            })
            .collect::<Vec<_>>()
            .into();
        let cursor = self
            .selected()
            .and_then(|index| indices.binary_search(&index).ok())
            .or_else(|| (!indices.is_empty()).then_some(0));
        self.query = query.into();
        self.indices = indices;
        self.cursor = cursor;
        Ok(())
    }
    pub fn selected(&self) -> Option<usize> {
        self.cursor.map(|cursor| self.indices[cursor])
    }
    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }
    pub fn indices(&self) -> Arc<[usize]> {
        Arc::clone(&self.indices)
    }
    /// Clamps at either end, matching ordinary catalog arrow navigation.
    pub fn step(&mut self, forward: bool) {
        self.step_by(forward, 1);
    }
    /// O(1) movement within the cached projection, including very large jumps.
    pub fn step_by(&mut self, forward: bool, count: usize) {
        if let Some(cursor) = self.cursor {
            self.cursor = Some(if forward {
                cursor.saturating_add(count).min(self.indices.len() - 1)
            } else {
                cursor.saturating_sub(count)
            });
        }
    }
    pub fn edge(&mut self, last: bool) {
        self.cursor =
            (!self.indices.is_empty()).then(|| if last { self.indices.len() - 1 } else { 0 });
    }
    pub fn select(&mut self, original_index: usize) -> Result<(), String> {
        let cursor = self
            .indices
            .binary_search(&original_index)
            .map_err(|_| "chart is outside the current search results")?;
        self.cursor = Some(cursor);
        Ok(())
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    fn search() -> CatalogSearch {
        CatalogSearch::new(&[
            SelectionItem {
                title: "ÉTOILE Blue".into(),
                artist: "Alice".into(),
            },
            SelectionItem {
                title: "Blue Moon".into(),
                artist: "BOB".into(),
            },
            SelectionItem {
                title: "Red étoile".into(),
                artist: "Alice".into(),
            },
        ])
        .unwrap()
    }
    #[test]
    fn pages_and_edges_keep_filtered_identity_and_cached_projection() {
        let items = (0..100)
            .map(|index| SelectionItem {
                title: format!("CHART {index}"),
                artist: if index % 2 == 0 { "even" } else { "odd" }.into(),
            })
            .collect::<Vec<_>>();
        let mut search = CatalogSearch::new(&items).unwrap();
        search.set_query("even").unwrap();
        let projection = search.indices();
        search.step_by(true, 15);
        assert_eq!(search.cursor(), Some(15));
        assert_eq!(search.selected(), Some(30));
        search.step_by(true, usize::MAX);
        assert_eq!(search.selected(), Some(98));
        search.step_by(false, 15);
        assert_eq!(search.selected(), Some(68));
        search.step_by(false, usize::MAX);
        assert_eq!(search.selected(), Some(0));
        search.edge(true);
        assert_eq!(search.selected(), Some(98));
        search.edge(false);
        search.step_by(true, 0);
        assert_eq!(search.selected(), Some(0));
        assert!(Arc::ptr_eq(&projection, &search.indices()));
        search.set_query("missing").unwrap();
        let empty = search.indices();
        search.edge(true);
        search.edge(false);
        search.step_by(true, usize::MAX);
        assert_eq!(search.selected(), None);
        assert!(Arc::ptr_eq(&empty, &search.indices()));
    }
    #[test]
    fn unicode_and_cross_field_tokens_preserve_order_and_selected_identity() {
        let mut search = search();
        search.select(2).unwrap();
        search.set_query("ÉTOILE alice").unwrap();
        assert_eq!(&*search.indices(), &[0, 2]);
        assert_eq!(search.selected(), Some(2));
        assert_eq!(search.cursor(), Some(1));
        search.step(true);
        assert_eq!(search.selected(), Some(2));
        search.step(false);
        assert_eq!(search.selected(), Some(0));
        search.set_query("  blue  BOB ").unwrap();
        assert_eq!(&*search.indices(), &[1]);
        assert_eq!(search.selected(), Some(1));
        search.set_query("").unwrap();
        assert_eq!(&*search.indices(), &[0, 1, 2]);
        assert_eq!(search.selected(), Some(1));
    }
    #[test]
    fn no_results_and_invalid_query_or_selection_preserve_atomic_state() {
        let mut search = search();
        let original = search.indices();
        search.set_query("").unwrap();
        assert!(Arc::ptr_eq(&original, &search.indices()));
        for invalid in ["x".repeat(257), "blue\n".into(), "a\u{2028}b".into()] {
            assert!(search.set_query(&invalid).is_err());
            assert!(Arc::ptr_eq(&original, &search.indices()));
            assert_eq!(search.selected(), Some(0));
        }
        assert!(search.select(4).is_err());
        assert_eq!(search.selected(), Some(0));
        search.set_query("missing").unwrap();
        assert!(search.indices().is_empty());
        assert_eq!(search.cursor(), None);
        search.step(true);
        search.step(false);
        assert_eq!(search.selected(), None);
        search.set_query("blue").unwrap();
        assert_eq!(search.selected(), Some(0));
        assert!(search.select(2).is_err());
        assert_eq!(search.selected(), Some(0));
        assert_eq!(CatalogSearch::new(&[]).unwrap().selected(), None);
    }
}
