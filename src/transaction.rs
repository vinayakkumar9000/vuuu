/// Ethereum legacy (EIP-155) transaction builder and signer.
use k256::ecdsa::SigningKey;
use sha3::{Digest, Keccak256};

use crate::config::CHAIN_ID;
use crate::rlp;

/// A legacy EIP-155 Ethereum transaction (type 0).
pub struct LegacyTx {
    pub nonce: u64,
    pub gas_price: u64,
    pub gas_limit: u64,
    pub to: [u8; 20],
    pub value: u64,
}

impl LegacyTx {
    /// Sign this transaction using the given key and return the raw RLP-encoded
    /// signed transaction bytes ready for `eth_sendRawTransaction`.
    pub fn sign(&self, key: &SigningKey) -> Result<Vec<u8>, k256::ecdsa::Error> {
        // Pre-encode fields that appear in both signing and final payloads.
        let nonce_rlp = rlp::encode_u64(self.nonce);
        let gas_price_rlp = rlp::encode_u64(self.gas_price);
        let gas_limit_rlp = rlp::encode_u64(self.gas_limit);
        let to_rlp = rlp::encode_bytes(&self.to);
        let value_rlp = rlp::encode_u64(self.value);
        let data_rlp = rlp::encode_bytes(&[]); // empty calldata
        let chain_id_rlp = rlp::encode_u64(CHAIN_ID);
        let zero_rlp = rlp::encode_u64(0);

        // EIP-155 signing payload:
        //   RLP([nonce, gasPrice, gasLimit, to, value, data, chainId, 0, 0])
        let sign_payload = rlp::encode_list(&[
            &nonce_rlp,
            &gas_price_rlp,
            &gas_limit_rlp,
            &to_rlp,
            &value_rlp,
            &data_rlp,
            &chain_id_rlp,
            &zero_rlp,
            &zero_rlp,
        ]);

        let hash = Keccak256::digest(&sign_payload);

        // Sign the hash (returns recoverable signature).
        let (signature, recovery_id) = key.sign_prehash_recoverable(&hash)?;

        // EIP-155 v value: chain_id * 2 + 35 + recovery_id
        let v = CHAIN_ID * 2 + 35 + recovery_id.to_byte() as u64;

        // Extract r and s as big-endian byte arrays.
        let sig_bytes = signature.to_bytes();
        let r_bytes = strip_leading_zeros(&sig_bytes[..32]);
        let s_bytes = strip_leading_zeros(&sig_bytes[32..]);

        let v_rlp = rlp::encode_u64(v);
        let r_rlp = rlp::encode_bytes(r_bytes);
        let s_rlp = rlp::encode_bytes(s_bytes);

        // Signed transaction:
        //   RLP([nonce, gasPrice, gasLimit, to, value, data, v, r, s])
        let raw_tx = rlp::encode_list(&[
            &nonce_rlp,
            &gas_price_rlp,
            &gas_limit_rlp,
            &to_rlp,
            &value_rlp,
            &data_rlp,
            &v_rlp,
            &r_rlp,
            &s_rlp,
        ]);

        Ok(raw_tx)
    }
}

/// Strip leading zero bytes from a big-endian integer representation.
fn strip_leading_zeros(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    &bytes[start..]
}

/// Derive the Ethereum address (last 20 bytes of keccak256 of the uncompressed
/// public key) from a signing key.
pub fn address_from_key(key: &SigningKey) -> [u8; 20] {
    let verifying_key = key.verifying_key();
    let point = verifying_key.to_encoded_point(false);
    // Skip the 0x04 uncompressed prefix byte.
    let pubkey_bytes = &point.as_bytes()[1..];
    let hash = Keccak256::digest(pubkey_bytes);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash[12..]);
    addr
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Well-known test vector: derive address from a known private key.
    #[test]
    fn test_address_derivation() {
        let key_hex = "4c0883a69102937d6231471b5dbb6204fe512961708279f2ee5b32a1b3d8e3e3";
        let key_bytes = hex::decode(key_hex).unwrap();
        let key = SigningKey::from_slice(&key_bytes).unwrap();
        let addr = address_from_key(&key);
        let addr_hex = hex::encode(addr);
        assert_eq!(
            addr_hex.to_lowercase(),
            "c84f75910179d8bd681fe741e701bb67b2097659"
        );
    }

    #[test]
    fn test_sign_produces_valid_output() {
        let key_hex = "4c0883a69102937d6231471b5dbb6204fe512961708279f2ee5b32a1b3d8e3e3";
        let key_bytes = hex::decode(key_hex).unwrap();
        let key = SigningKey::from_slice(&key_bytes).unwrap();

        let tx = LegacyTx {
            nonce: 0,
            gas_price: 0,
            gas_limit: 21000,
            to: [0xab; 20],
            value: 1,
        };

        let raw = tx.sign(&key).unwrap();
        // The raw transaction should be non-empty and start with an RLP list prefix.
        assert!(!raw.is_empty());
        // Long list: first byte should be >= 0xf8 (payload > 55 bytes)
        // or >= 0xc0 for short list.
        assert!(raw[0] >= 0xc0);
    }

    #[test]
    fn test_strip_leading_zeros() {
        assert_eq!(strip_leading_zeros(&[0, 0, 1, 2]), &[1, 2]);
        assert_eq!(strip_leading_zeros(&[1, 2, 3]), &[1, 2, 3]);
        assert_eq!(strip_leading_zeros(&[0, 0, 0]), &[] as &[u8]);
        assert_eq!(strip_leading_zeros(&[]), &[] as &[u8]);
    }
}
