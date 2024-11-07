use alloc::vec::Vec;

pub struct LinkMap<K, V> {
    data: Vec<(K, V)>,
}

impl<K: PartialEq, V> LinkMap<K, V> {
    pub fn new() -> Self {
        LinkMap { data: Vec::new() }
    }

    pub fn insert(&mut self, key: K, value: V) {
        if let Some(i) = self.data.iter().position(|&(ref k, _)| *k == key) {
            self.data[i] = (key, value);
        } else {
            self.data.push((key, value));
        }
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        for (k, v) in &self.data {
            if k == key {
                return Some(v);
            }
        }
        None
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let mut index = 0;
        for (i, (k, _)) in self.data.iter().enumerate() {
            if k == key {
                index = i;
                break;
            }
        }
        if index < self.data.len() {
            Some(self.data.remove(index).1)
        } else {
            None
        }
    }
}