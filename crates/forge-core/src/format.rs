//! Deterministic hashing and formatting primitives for ContextUnity Forge.

/// Computes the deterministic FNV-1a 64-bit hash used for indexed identifiers.
#[inline]
pub fn stable_hash64(value: &str) -> i64 {
    value
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        }) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_hash64_uses_fnv1a_vectors() {
        assert_eq!(stable_hash64("") as u64, 0xcbf29ce484222325);
        assert_eq!(stable_hash64("a") as u64, 0xaf63dc4c8601ec8c);
        assert_eq!(stable_hash64("hello") as u64, 0xa430d84680aabd0b);
    }
}
