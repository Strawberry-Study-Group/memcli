/// Sorted name index for O(log N) prefix search
#[derive(Debug, Default)]
pub struct NameIndex {
    names: Vec<String>,
}

impl NameIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from an unsorted iterator of names
    pub fn from_iter(iter: impl IntoIterator<Item = String>) -> Self {
        let mut names: Vec<String> = iter.into_iter().collect();
        names.sort();
        Self { names }
    }

    /// Insert a name, maintaining sorted order
    pub fn insert(&mut self, name: String) {
        let pos = self.names.partition_point(|n| n.as_str() < name.as_str());
        if pos < self.names.len() && self.names[pos] == name {
            return; // already exists
        }
        self.names.insert(pos, name);
    }

    /// Remove a name
    pub fn remove(&mut self, name: &str) {
        let pos = self.names.partition_point(|n| n.as_str() < name);
        if pos < self.names.len() && self.names[pos] == name {
            self.names.remove(pos);
        }
    }

    /// Prefix search using binary search
    pub fn prefix_search(&self, prefix: &str) -> Vec<&str> {
        let start = self.names.partition_point(|n| n.as_str() < prefix);
        self.names[start..]
            .iter()
            .take_while(|n| n.starts_with(prefix))
            .map(|n| n.as_str())
            .collect()
    }

    /// Check if a name exists
    pub fn contains(&self, name: &str) -> bool {
        let pos = self.names.partition_point(|n| n.as_str() < name);
        pos < self.names.len() && self.names[pos] == name
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Get all names (for iteration)
    pub fn all(&self) -> &[String] {
        &self.names
    }
}
