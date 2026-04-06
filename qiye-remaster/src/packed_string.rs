const ALPHABET: &[u8; 31] = b"abcdefghijklmnopqrstuvwxyz_1234";

/// Decode a 5-bit packed string from 4×u32 (128 bits → up to 24 chars).
///
/// Each u32 encodes 6 characters at 5 bits each. Index 0x1f = terminator.
pub fn decode(words: &[u32; 4]) -> String {
    let mut result = String::with_capacity(24);
    for &word in words {
        for i in 0..6 {
            let idx = (word >> (i * 5)) & 0x1f;
            if idx == 0x1f {
                return result;
            }
            result.push(ALPHABET[idx as usize] as char);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminator_immediate() {
        assert_eq!(decode(&[0x1f, 0, 0, 0]), "");
    }

    #[test]
    fn test_single_char() {
        // 'a' = index 0, then terminator at index 1 (bits 5..9 = 0x1f)
        assert_eq!(decode(&[0x1f << 5, 0, 0, 0]), "a");
    }

    #[test]
    fn test_alphabet() {
        // First 6 chars: a=0, b=1, c=2, d=3, e=4, f=5
        let w0 = 0 | (1 << 5) | (2 << 10) | (3 << 15) | (4 << 20) | (5 << 25);
        // 7th char g=6, then terminator
        let w1 = 6 | (0x1f << 5);
        assert_eq!(decode(&[w0, w1, 0, 0]), "abcdefg");
    }
}
