use moka::sync::Cache;
use std::hash::Hash;
use std::time::Duration;

pub struct ExpirableCache<K, V> {
    cache: Cache<K, V>,
}

impl<K, V> ExpirableCache<K, V>
where
    K: 'static + Eq + Hash + Send + Sync,
    V: Clone + Send + Sync + 'static,
{
    pub fn new(max_capacity: u64, ttl: Duration) -> Self {
        let cache = Cache::builder()
            .max_capacity(max_capacity)
            .time_to_live(ttl)
            .build();
        ExpirableCache { cache }
    }

    pub fn insert(&self, key: K, value: V) {
        self.cache.insert(key, value);
    }

    pub fn get(&self, key: &K) -> Option<V> {
        self.cache.get(key)
    }

    pub fn remove(&self, key: &K) {
        self.cache.invalidate(key);
    }
}
