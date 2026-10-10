use std::collections::HashMap;

/// A map that iterates in insertion order: setting an existing key keeps its place, removing one
/// and setting it again moves it to the end. Selection's request order, eviction order and
/// balancing all depend on this, so they come out the same from run to run.
#[derive(Clone, Debug)]
pub struct OrderedMap<V> {
    slots: Vec<Option<(u64, V)>>,
    index: HashMap<u64, usize>,
}

impl<V> Default for OrderedMap<V> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            index: HashMap::new(),
        }
    }
}

impl<V> OrderedMap<V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub fn contains_key(&self, key: u64) -> bool {
        self.index.contains_key(&key)
    }

    pub fn get(&self, key: u64) -> Option<&V> {
        self.index
            .get(&key)
            .map(|&i| &self.slots[i].as_ref().expect("an indexed slot is live").1)
    }

    pub fn get_mut(&mut self, key: u64) -> Option<&mut V> {
        let i = *self.index.get(&key)?;
        Some(&mut self.slots[i].as_mut().expect("an indexed slot is live").1)
    }

    pub fn insert(&mut self, key: u64, value: V) {
        match self.index.get(&key) {
            Some(&i) => self.slots[i] = Some((key, value)),
            None => {
                self.index.insert(key, self.slots.len());
                self.slots.push(Some((key, value)));
            }
        }
    }

    pub fn remove(&mut self, key: u64) -> Option<V> {
        let i = self.index.remove(&key)?;
        let value = self.slots[i].take().map(|(_, v)| v);
        // Compact once tombstones outnumber live entries; order is kept.
        if self.slots.len() > 64 && self.index.len() * 2 < self.slots.len() {
            self.slots.retain(Option::is_some);
            for (i, slot) in self.slots.iter().enumerate() {
                self.index.insert(slot.as_ref().expect("retained").0, i);
            }
        }
        value
    }

    pub fn keys(&self) -> impl Iterator<Item = u64> + '_ {
        self.slots.iter().flatten().map(|(k, _)| *k)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.slots.iter().flatten().map(|(_, v)| v)
    }

    /// Keys in order, for loops that remove entries while they go.
    pub fn key_snapshot(&self) -> Vec<u64> {
        self.keys().collect()
    }
}
