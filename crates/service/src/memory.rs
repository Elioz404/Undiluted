use std::collections::BTreeMap;

use crate::ports::{Store, StoreError};

/// A store for tests and the local demo. Nothing survives the process.
#[derive(Debug, Clone, Default)]
pub struct MemoryStore(BTreeMap<String, Vec<u8>>);

impl Store for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self.0.get(key).cloned())
    }

    fn put(&mut self, key: &str, value: &[u8]) -> Result<(), StoreError> {
        self.0.insert(key.to_owned(), value.to_vec());
        Ok(())
    }

    fn put_new(&mut self, key: &str, value: &[u8]) -> Result<bool, StoreError> {
        if self.0.contains_key(key) {
            return Ok(false);
        }
        self.0.insert(key.to_owned(), value.to_vec());
        Ok(true)
    }

    fn delete(&mut self, key: &str) -> Result<(), StoreError> {
        self.0.remove(key);
        Ok(())
    }
}
