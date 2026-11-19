//! LRU with a byte budget. Used for the shared view cache.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

pub struct Lru<K, V> {
    budget: u64,
    used: u64,
    tick: u64,
    map: HashMap<K, (V, u64, u64)>, // value, cost, stamp
    order: BTreeMap<u64, K>,        // stamp → key, oldest first
}

impl<K: Hash + Eq + Clone, V: Clone> Lru<K, V> {
    pub fn new(budget: u64) -> Self {
        Lru {
            budget,
            used: 0,
            tick: 0,
            map: HashMap::new(),
            order: BTreeMap::new(),
        }
    }

    pub fn used(&self) -> u64 {
        self.used
    }

    pub fn budget(&self) -> u64 {
        self.budget
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn contains(&self, k: &K) -> bool {
        self.map.contains_key(k)
    }

    pub fn get(&mut self, k: &K) -> Option<V> {
        let (v, _, stamp) = self.map.get_mut(k)?;
        self.order.remove(stamp);
        self.tick += 1;
        *stamp = self.tick;
        self.order.insert(self.tick, k.clone());
        Some(v.clone())
    }

    /// Insert, evicting least recently used entries. Entries larger than the budget are not stored.
    pub fn insert(&mut self, k: K, v: V, cost: u64) {
        self.remove(&k);
        if cost > self.budget {
            return;
        }
        while self.used + cost > self.budget {
            let Some((_, old)) = self.order.pop_first() else { break };
            if let Some((_, c, _)) = self.map.remove(&old) {
                self.used -= c;
            }
        }
        self.tick += 1;
        self.order.insert(self.tick, k.clone());
        self.map.insert(k, (v, cost, self.tick));
        self.used += cost;
    }

    pub fn remove(&mut self, k: &K) {
        if let Some((_, c, stamp)) = self.map.remove(k) {
            self.order.remove(&stamp);
            self.used -= c;
        }
    }

    /// Drop every entry matching `f` (e.g. all keys of a path after a save).
    pub fn retain(&mut self, mut f: impl FnMut(&K) -> bool) {
        let gone: Vec<K> = self.map.keys().filter(|k| !f(k)).cloned().collect();
        for k in gone {
            self.remove(&k);
        }
    }

    pub fn set_budget(&mut self, budget: u64) {
        self.budget = budget;
        while self.used > self.budget {
            let Some((_, old)) = self.order.pop_first() else { break };
            if let Some((_, c, _)) = self.map.remove(&old) {
                self.used -= c;
            }
        }
    }
}

/// Physical RAM in bytes (`GlobalMemoryStatusEx`), `None` if the call fails.
pub fn physical_memory() -> Option<u64> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: dwLength is set as required.
    unsafe { GlobalMemoryStatusEx(&mut m).ok()? };
    Some(m.ullTotalPhys)
}

/// Budget for a preloading mode, never above 25 % of physical RAM.
pub fn view_budget(mode_budget: u64, ram: Option<u64>) -> u64 {
    ram.map_or(mode_budget, |r| mode_budget.min(r / 4))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_by_bytes_in_lru_order() {
        let mut c = Lru::new(100);
        c.insert("a", 1, 40);
        c.insert("b", 2, 40);
        assert_eq!(c.get(&"a"), Some(1)); // a is now the most recent
        c.insert("c", 3, 40); // evicts b
        assert!(c.contains(&"a") && !c.contains(&"b") && c.contains(&"c"));
        assert_eq!(c.used(), 80);
        c.insert("huge", 4, 101); // larger than the budget: not stored, nothing evicted
        assert_eq!(c.len(), 2);
        c.insert("a", 5, 10); // replace updates the cost
        assert_eq!((c.get(&"a"), c.used()), (Some(5), 50));
        c.set_budget(20);
        assert_eq!(c.used(), 10);
        c.retain(|k| *k != "a");
        assert!(c.is_empty() && c.used() == 0);
    }

    #[test]
    fn budget_capped_at_quarter_of_ram() {
        assert_eq!(view_budget(2 << 30, Some(4 << 30)), 1 << 30);
        assert_eq!(view_budget(256 << 20, Some(32 << 30)), 256 << 20);
        assert_eq!(view_budget(256 << 20, None), 256 << 20);
        assert!(physical_memory().unwrap() > 0);
    }
}
