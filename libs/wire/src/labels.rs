//! A label set as a typed message's `bytes` field carries it (the steward's `login` reply,
//! `blame`, `pending`, `submit`): eight bytes per id, little-endian, in the set's order.

/// The bytes of `ids`.
pub fn encode(ids: &[u64]) -> impl Iterator<Item = u8> + '_ { ids.iter().flat_map(|id| id.to_le_bytes()) }

/// The ids in `bytes`; a trailing piece shorter than eight bytes is no id.
pub fn decode(bytes: &[u8]) -> impl Iterator<Item = u64> + '_ {
    bytes.chunks_exact(8).map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    #[test]
    fn a_label_set_round_trips_and_a_short_tail_is_no_id() {
        let ids = [7, u64::MAX, 0x0102_0304_0506_0708];
        let mut bytes: Vec<u8> = super::encode(&ids).collect();
        assert_eq!(&bytes[16..], &[8, 7, 6, 5, 4, 3, 2, 1]);
        bytes.extend([1, 2, 3]);
        assert_eq!(super::decode(&bytes).collect::<Vec<_>>(), ids);
    }
}
