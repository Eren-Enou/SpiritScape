//! Carried inventories and building storage. Both only move whole units and
//! never create or destroy resources; callers move amounts between them.

use crate::defs::ResourceId;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inventory {
    items: BTreeMap<ResourceId, u32>,
    pub capacity: u32,
}

impl Inventory {
    pub fn new(capacity: u32) -> Self {
        Self { items: BTreeMap::new(), capacity }
    }

    pub fn items(&self) -> &BTreeMap<ResourceId, u32> {
        &self.items
    }

    pub fn get(&self, r: ResourceId) -> u32 {
        self.items.get(&r).copied().unwrap_or(0)
    }

    pub fn total(&self) -> u32 {
        self.items.values().sum()
    }

    pub fn free(&self) -> u32 {
        self.capacity.saturating_sub(self.total())
    }

    pub fn is_full(&self) -> bool {
        self.free() == 0
    }

    /// Adds up to `n` units, limited by free capacity. Returns the amount added.
    pub fn add(&mut self, r: ResourceId, n: u32) -> u32 {
        let added = n.min(self.free());
        if added > 0 {
            *self.items.entry(r).or_default() += added;
        }
        added
    }

    /// Removes up to `n` units. Returns the amount removed.
    pub fn remove(&mut self, r: ResourceId, n: u32) -> u32 {
        let have = self.get(r);
        let removed = n.min(have);
        if removed == have {
            self.items.remove(&r);
        } else {
            *self.items.get_mut(&r).unwrap() -= removed;
        }
        removed
    }
}

/// Building storage with per-resource reservations. Reserved units stay in
/// `contents` (they physically exist) but are not available to normal users.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Storage {
    contents: BTreeMap<ResourceId, u32>,
    reserved: BTreeMap<ResourceId, u32>,
    pub capacity: u32,
}

impl Storage {
    pub fn new(capacity: u32) -> Self {
        Self { capacity, ..Default::default() }
    }

    pub fn contents(&self) -> &BTreeMap<ResourceId, u32> {
        &self.contents
    }

    pub fn get(&self, r: ResourceId) -> u32 {
        self.contents.get(&r).copied().unwrap_or(0)
    }

    pub fn reserved(&self, r: ResourceId) -> u32 {
        self.reserved.get(&r).copied().unwrap_or(0)
    }

    pub fn available(&self, r: ResourceId) -> u32 {
        self.get(r).saturating_sub(self.reserved(r))
    }

    pub fn total(&self) -> u32 {
        self.contents.values().sum()
    }

    pub fn free(&self) -> u32 {
        self.capacity.saturating_sub(self.total())
    }

    /// Stores up to `n` units, limited by free capacity. Returns the amount stored.
    pub fn deposit(&mut self, r: ResourceId, n: u32) -> u32 {
        let stored = n.min(self.free());
        if stored > 0 {
            *self.contents.entry(r).or_default() += stored;
        }
        stored
    }

    /// Takes up to `n` unreserved units.
    pub fn withdraw_available(&mut self, r: ResourceId, n: u32) -> u32 {
        let taken = n.min(self.available(r));
        self.take(r, taken);
        taken
    }

    /// Takes up to `n` previously reserved units, releasing the reservation.
    pub fn withdraw_reserved(&mut self, r: ResourceId, n: u32) -> u32 {
        let taken = n.min(self.reserved(r)).min(self.get(r));
        self.take(r, taken);
        self.set_reserved(r, self.reserved(r) - taken);
        taken
    }

    /// Reserves `n` units if that many are available.
    pub fn try_reserve(&mut self, r: ResourceId, n: u32) -> bool {
        if self.available(r) < n {
            return false;
        }
        self.set_reserved(r, self.reserved(r) + n);
        true
    }

    pub fn release_reserved(&mut self, r: ResourceId, n: u32) {
        self.set_reserved(r, self.reserved(r).saturating_sub(n));
    }

    fn take(&mut self, r: ResourceId, n: u32) {
        if n == 0 {
            return;
        }
        let left = self.get(r) - n;
        if left == 0 {
            self.contents.remove(&r);
        } else {
            self.contents.insert(r, left);
        }
    }

    fn set_reserved(&mut self, r: ResourceId, n: u32) {
        if n == 0 {
            self.reserved.remove(&r);
        } else {
            self.reserved.insert(r, n);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOGS: ResourceId = ResourceId(0);

    #[test]
    fn inventory_respects_capacity() {
        let mut inv = Inventory::new(10);
        assert_eq!(inv.add(LOGS, 7), 7);
        assert_eq!(inv.add(LOGS, 7), 3);
        assert!(inv.is_full());
        assert_eq!(inv.remove(LOGS, 20), 10);
        assert_eq!(inv.total(), 0);
    }

    #[test]
    fn reserved_units_are_not_available() {
        let mut s = Storage::new(100);
        s.deposit(LOGS, 20);
        assert!(s.try_reserve(LOGS, 15));
        assert!(!s.try_reserve(LOGS, 6));
        assert_eq!(s.available(LOGS), 5);
        assert_eq!(s.withdraw_available(LOGS, 10), 5);
        assert_eq!(s.withdraw_reserved(LOGS, 10), 10);
        assert_eq!(s.reserved(LOGS), 5);
        assert_eq!(s.get(LOGS), 5);
    }
}
