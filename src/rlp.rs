/// Minimal RLP encoder for Ethereum legacy transactions.
///
/// Implements just enough RLP to encode EIP-155 legacy transactions without
/// pulling in a full RLP library.

/// Encode a byte slice as an RLP string item.
pub fn encode_bytes(data: &[u8]) -> Vec<u8> {
    if data.len() == 1 && data[0] < 0x80 {
        vec![data[0]]
    } else if data.is_empty() {
        vec![0x80]
    } else if data.len() <= 55 {
        let mut out = Vec::with_capacity(1 + data.len());
        out.push(0x80 + data.len() as u8);
        out.extend_from_slice(data);
        out
    } else {
        let len_bytes = uint_to_min_bytes(data.len() as u64);
        let mut out = Vec::with_capacity(1 + len_bytes.len() + data.len());
        out.push(0xb7 + len_bytes.len() as u8);
        out.extend_from_slice(&len_bytes);
        out.extend_from_slice(data);
        out
    }
}

/// Encode a `u64` as an RLP integer item (big-endian, no leading zeros).
pub fn encode_u64(value: u64) -> Vec<u8> {
    if value == 0 {
        vec![0x80] // RLP encoding of empty byte string (== integer 0)
    } else {
        encode_bytes(&uint_to_min_bytes(value))
    }
}

/// Encode a list of already-RLP-encoded items.
pub fn encode_list(items: &[&[u8]]) -> Vec<u8> {
    let total_len: usize = items.iter().map(|i| i.len()).sum();
    if total_len <= 55 {
        let mut out = Vec::with_capacity(1 + total_len);
        out.push(0xc0 + total_len as u8);
        for item in items {
            out.extend_from_slice(item);
        }
        out
    } else {
        let len_bytes = uint_to_min_bytes(total_len as u64);
        let mut out = Vec::with_capacity(1 + len_bytes.len() + total_len);
        out.push(0xf7 + len_bytes.len() as u8);
        out.extend_from_slice(&len_bytes);
        for item in items {
            out.extend_from_slice(item);
        }
        out
    }
}

/// Convert a `u64` to its minimal big-endian byte representation.
/// Returns an empty vec for 0.
fn uint_to_min_bytes(value: u64) -> Vec<u8> {
    if value == 0 {
        return vec![];
    }
    let bytes = value.to_be_bytes();
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(7);
    bytes[start..].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_empty_bytes() {
        assert_eq!(encode_bytes(&[]), vec![0x80]);
    }

    #[test]
    fn test_encode_single_byte_below_0x80() {
        assert_eq!(encode_bytes(&[0x00]), vec![0x00]);
        assert_eq!(encode_bytes(&[0x01]), vec![0x01]);
        assert_eq!(encode_bytes(&[0x7f]), vec![0x7f]);
    }

    #[test]
    fn test_encode_single_byte_at_or_above_0x80() {
        assert_eq!(encode_bytes(&[0x80]), vec![0x81, 0x80]);
        assert_eq!(encode_bytes(&[0xff]), vec![0x81, 0xff]);
    }

    #[test]
    fn test_encode_short_string() {
        let data = b"hello";
        let mut expected = vec![0x80 + 5];
        expected.extend_from_slice(data);
        assert_eq!(encode_bytes(data), expected);
    }

    #[test]
    fn test_encode_u64_zero() {
        assert_eq!(encode_u64(0), vec![0x80]);
    }

    #[test]
    fn test_encode_u64_small() {
        assert_eq!(encode_u64(1), vec![0x01]);
        assert_eq!(encode_u64(127), vec![0x7f]);
    }

    #[test]
    fn test_encode_u64_medium() {
        assert_eq!(encode_u64(128), vec![0x81, 0x80]);
        assert_eq!(encode_u64(256), vec![0x82, 0x01, 0x00]);
    }

    #[test]
    fn test_encode_u64_gas_limit() {
        // 21000 = 0x5208
        assert_eq!(encode_u64(21000), vec![0x82, 0x52, 0x08]);
    }

    #[test]
    fn test_encode_u64_chain_id() {
        // 324705682 = 0x135A9D92
        assert_eq!(
            encode_u64(324_705_682),
            vec![0x84, 0x13, 0x5A, 0x9D, 0x92]
        );
    }

    #[test]
    fn test_encode_list_short() {
        let a = encode_u64(1);
        let b = encode_u64(2);
        let list = encode_list(&[&a, &b]);
        // payload = [0x01, 0x02] → len 2
        // list prefix = 0xc0 + 2 = 0xc2
        assert_eq!(list, vec![0xc2, 0x01, 0x02]);
    }

    #[test]
    fn test_encode_20_byte_address() {
        let addr = [0xab_u8; 20];
        let encoded = encode_bytes(&addr);
        assert_eq!(encoded.len(), 21); // 1 prefix + 20 bytes
        assert_eq!(encoded[0], 0x94); // 0x80 + 20
    }

    #[test]
    fn test_uint_to_min_bytes() {
        assert_eq!(uint_to_min_bytes(0), Vec::<u8>::new());
        assert_eq!(uint_to_min_bytes(1), vec![1]);
        assert_eq!(uint_to_min_bytes(255), vec![0xff]);
        assert_eq!(uint_to_min_bytes(256), vec![0x01, 0x00]);
    }
}
