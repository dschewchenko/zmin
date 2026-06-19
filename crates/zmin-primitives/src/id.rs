const ZMIN_ALPHABET: &[char; 58] = &[
    '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'J', 'K',
    'L', 'M', 'N', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'a', 'b', 'c', 'd', 'e',
    'f', 'g', 'h', 'i', 'j', 'k', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y',
    'z',
];
const ZMIN_LENGTH: usize = 22;

/// Generate a new ZminId (Base58 string) using a cryptographically secure RNG.
pub fn generate() -> String {
    nanoid::nanoid!(ZMIN_LENGTH, ZMIN_ALPHABET)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_has_expected_length() {
        let id = generate();
        assert_eq!(id.len(), ZMIN_LENGTH);
    }

    #[test]
    fn generate_uses_alphabet() {
        let id = generate();
        assert!(id.chars().all(|ch| ZMIN_ALPHABET.contains(&ch)));
    }
}
