use super::{BmsError, BmsErrorKind, DuplicatePolicy, define};
use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::rc::Rc;

#[derive(Debug)]
struct Key {
    identity: u16,
    tag: &'static str,
    comparisons: Rc<Cell<usize>>,
}

impl Key {
    fn new(identity: u16, tag: &'static str, comparisons: &Rc<Cell<usize>>) -> Self {
        Self {
            identity,
            tag,
            comparisons: Rc::clone(comparisons),
        }
    }
}

impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
    }
}

impl Eq for Key {}

impl PartialOrd for Key {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Key {
    fn cmp(&self, other: &Self) -> Ordering {
        self.comparisons.set(self.comparisons.get() + 1);
        self.identity.cmp(&other.identity)
    }
}

// The previous helper is deliberately local: compare the work performed by
// the production helper without exposing a second implementation to callers.
fn previous_define<K: Ord, V>(
    map: &mut BTreeMap<K, V>,
    key: K,
    value: V,
    line: usize,
    label: &'static str,
    policy: DuplicatePolicy,
) -> Result<(), BmsError> {
    if policy == DuplicatePolicy::Reject && map.contains_key(&key) {
        return Err(BmsError::new(line, BmsErrorKind::Duplicate(label)));
    }
    map.insert(key, value);
    Ok(())
}

fn populated_map(comparisons: &Rc<Cell<usize>>) -> BTreeMap<Key, u16> {
    (0..256)
        .step_by(2)
        .map(|identity| (Key::new(identity, "existing", comparisons), identity))
        .collect()
}

fn snapshot(map: &BTreeMap<Key, u16>) -> Vec<(u16, &'static str, u16)> {
    map.iter()
        .map(|(key, value)| (key.identity, key.tag, *value))
        .collect()
}

#[test]
fn reject_vacant_definitions_use_fewer_comparisons_than_previous_helper() {
    let current_comparisons = Rc::new(Cell::new(0));
    let previous_comparisons = Rc::new(Cell::new(0));
    let mut current = populated_map(&current_comparisons);
    let mut previous = populated_map(&previous_comparisons);
    assert_eq!(snapshot(&current), snapshot(&previous));
    current_comparisons.set(0);
    previous_comparisons.set(0);

    for identity in (1..256).step_by(2) {
        define(
            &mut current,
            Key::new(identity, "inserted", &current_comparisons),
            identity,
            37,
            "WAV",
            DuplicatePolicy::Reject,
        )
        .unwrap();
        previous_define(
            &mut previous,
            Key::new(identity, "inserted", &previous_comparisons),
            identity,
            37,
            "WAV",
            DuplicatePolicy::Reject,
        )
        .unwrap();
    }

    assert_eq!(snapshot(&current), snapshot(&previous));
    let current_count = current_comparisons.get();
    let previous_count = previous_comparisons.get();
    assert!(current_count > 0, "comparison instrumentation must run");
    assert!(
        current_count < previous_count,
        "single lookup performed {current_count} comparisons; previous helper performed {previous_count}"
    );
}

#[test]
fn reject_equivalent_key_preserves_canonical_key_value_and_length() {
    let comparisons = Rc::new(Cell::new(0));
    let mut map = populated_map(&comparisons);
    let before = snapshot(&map);
    let error = define(
        &mut map,
        Key::new(64, "replacement", &comparisons),
        999,
        73,
        "STOP",
        DuplicatePolicy::Reject,
    )
    .unwrap_err();

    assert_eq!(error.line, 73);
    assert_eq!(error.kind, BmsErrorKind::Duplicate("STOP"));
    assert_eq!(map.len(), before.len());
    assert_eq!(snapshot(&map), before);
}

#[test]
fn last_wins_equivalent_key_replaces_value_and_retains_canonical_key() {
    let comparisons = Rc::new(Cell::new(0));
    let mut map = populated_map(&comparisons);
    let before = snapshot(&map);
    define(
        &mut map,
        Key::new(64, "replacement", &comparisons),
        999,
        73,
        "STOP",
        DuplicatePolicy::LastWins,
    )
    .unwrap();

    let expected: Vec<_> = before
        .into_iter()
        .map(|(identity, tag, value)| (identity, tag, if identity == 64 { 999 } else { value }))
        .collect();
    assert_eq!(map.len(), expected.len());
    assert_eq!(snapshot(&map), expected);
}

#[test]
fn both_policies_admit_a_vacant_key_with_its_original_representation() {
    for policy in [DuplicatePolicy::Reject, DuplicatePolicy::LastWins] {
        let comparisons = Rc::new(Cell::new(0));
        let mut map = populated_map(&comparisons);
        let mut expected = snapshot(&map);
        expected.push((257, "new", 999));
        define(
            &mut map,
            Key::new(257, "new", &comparisons),
            999,
            73,
            "STOP",
            policy,
        )
        .unwrap();

        assert_eq!(map.len(), expected.len());
        assert_eq!(snapshot(&map), expected);
    }
}
