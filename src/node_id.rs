//! Arena-based widget identity.
//!
//! `NodeId` is a generational key from `slotmap` used by the arena-based
//! `WidgetTree`. It detects use-after-remove and round-trips through `u64`
//! for hit-test metadata compatibility (`MetaValue::Int`).

/// Canonical identity for every node in the widget tree.
///
/// Generational — detects use-after-remove. Used by the arena-based
/// `WidgetTree` for all widget identity needs.
///
/// Round-trips to `u64` via [`node_id_to_ffi`] / [`node_id_from_ffi`] for
/// hit-test metadata compatibility.
pub type NodeId = slotmap::DefaultKey;

/// Encode a `NodeId` as a `u64` for FFI / hit-test metadata.
///
/// The returned value is an opaque encoding of the key's version and index.
/// Use [`node_id_from_ffi`] to recover the original `NodeId`.
#[inline]
#[must_use]
pub fn node_id_to_ffi(id: NodeId) -> u64 {
    use slotmap::Key;
    id.data().as_ffi()
}

/// Decode a `u64` back into a `NodeId`.
///
/// # Safety contract (logical, not `unsafe`)
///
/// The caller must pass a value previously obtained from [`node_id_to_ffi`].
/// Passing arbitrary integers produces a syntactically valid but semantically
/// bogus key — any subsequent `SlotMap` lookup will simply return `None`.
#[inline]
#[must_use]
pub fn node_id_from_ffi(ffi: u64) -> NodeId {
    slotmap::KeyData::from_ffi(ffi).into()
}

/// Encode a `NodeId` as the `i64` that segment metadata (`MetaValue::Int`)
/// stores.
///
/// Reuses the bits of [`node_id_to_ffi`] unchanged, so an id whose top bit
/// is set becomes a negative `i64` and still decodes exactly with
/// [`node_id_from_meta`].
#[inline]
pub(crate) fn node_id_to_meta(id: NodeId) -> i64 {
    i64::from_ne_bytes(node_id_to_ffi(id).to_ne_bytes())
}

/// Decode a value written by [`node_id_to_meta`].
#[inline]
pub(crate) fn node_id_from_meta(meta: i64) -> NodeId {
    node_id_from_ffi(u64::from_ne_bytes(meta.to_ne_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use slotmap::SlotMap;

    #[test]
    fn round_trip_through_ffi() {
        let mut sm = SlotMap::new();
        let id: NodeId = sm.insert("hello");

        let encoded = node_id_to_ffi(id);
        let decoded = node_id_from_ffi(encoded);
        assert_eq!(id, decoded);
        assert_eq!(sm[decoded], "hello");
    }

    #[test]
    fn round_trip_through_meta_keeps_top_bit() {
        // Version 0x8000_0001 (odd, as slotmap requires), index 5.
        let id = node_id_from_ffi(0x8000_0001_0000_0005);
        let meta = node_id_to_meta(id);
        assert!(
            meta < 0,
            "an id with the top bit set encodes as a negative i64"
        );
        assert_eq!(node_id_from_meta(meta), id);

        let small = node_id_from_ffi(0x0000_0001_0000_0002);
        assert_eq!(node_id_to_meta(small), 0x0000_0001_0000_0002);
        assert_eq!(node_id_from_meta(node_id_to_meta(small)), small);
    }

    #[test]
    fn bogus_ffi_value_does_not_crash() {
        let sm: SlotMap<NodeId, ()> = SlotMap::new();
        let bogus = node_id_from_ffi(0xDEAD_BEEF);
        // Lookup returns None, no panic.
        assert!(sm.get(bogus).is_none());
    }
}
