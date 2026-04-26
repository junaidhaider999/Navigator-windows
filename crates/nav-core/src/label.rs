// Legacy parity: see legacy/src/HuntAndPeck/Services/HintLabelService.cs
//
//! Vimium-style, capacity-aware label strings (see `08-hint-generation.md`).
//!
//! # Example
//!
//! ```
//! use nav_core::generate_labels;
//! let alphabet: Vec<char> = "SADFJKLEWCMPGH".chars().collect();
//! let v = generate_labels(1, &alphabet);
//! assert_eq!(v.len(), 1);
//! assert_eq!(&*v[0], "S");
//! ```

/// Builds `count` unique, prefix-free labels over `alphabet` (order matters: front = preferred for short keys).
///
/// # Panics
///
/// Debug builds: `debug_assert!` if `count > 0` and `alphabet` is empty. Release builds treat an empty
/// alphabet as producing empty strings (invalid configuration — should not reach the hot path).
///
/// # Example
///
/// ```
/// use nav_core::generate_labels;
/// let alphabet: Vec<char> = (0..14).map(|i| (b'A' + i) as char).collect();
/// let labels = generate_labels(256, &alphabet);
/// assert_eq!(labels.len(), 256);
/// assert_eq!(labels.iter().collect::<std::collections::HashSet<_>>().len(), 256);
/// ```
#[must_use]
pub fn generate_labels(count: usize, alphabet: &[char]) -> Vec<Box<str>> {
    if count == 0 {
        return Vec::new();
    }
    debug_assert!(
        !alphabet.is_empty(),
        "alphabet must be non-empty when count > 0"
    );
    let base = alphabet.len().max(1);

    // Integer-stable ceil(log_base(count)) for count > 1 (avoids float surprises at boundaries).
    let digits_needed = if count <= 1 {
        0
    } else {
        let mut d = 1usize;
        let mut threshold = base;
        while threshold < count {
            d += 1;
            threshold = threshold.saturating_mul(base);
        }
        d
    };

    let whole_hint_count = base.pow(digits_needed as u32);
    let short_hint_count = whole_hint_count.saturating_sub(count) / base;
    let long_hint_count = count - short_hint_count;
    let long_hint_prefix_count = whole_hint_count / base - short_hint_count;

    let mut hint_strings: Vec<Box<str>> = Vec::with_capacity(count);

    let mut j = 0usize;
    for i in 0..long_hint_count {
        let s = number_to_hint_string_reversed(j, alphabet, digits_needed);
        hint_strings.push(s.into_boxed_str());
        if long_hint_prefix_count > 0 && (i + 1) % long_hint_prefix_count == 0 {
            j += short_hint_count;
        }
        j += 1;
    }

    if digits_needed > 1 {
        for i in 0..short_hint_count {
            let s = number_to_hint_string_reversed(
                i + long_hint_prefix_count,
                alphabet,
                digits_needed - 1,
            );
            hint_strings.push(s.into_boxed_str());
        }
    }

    debug_assert_eq!(hint_strings.len(), count);
    hint_strings
}

/// `(digits_needed, long_count, short_count)` for the vimium partition (matches legacy `GetHintStrings`).
#[must_use]
pub(crate) fn vimium_partition(count: usize, alphabet_len: usize) -> (usize, usize, usize) {
    if count == 0 {
        return (0, 0, 0);
    }
    let base = alphabet_len.max(1);
    let digits_needed = if count <= 1 {
        0
    } else {
        let mut d = 1usize;
        let mut threshold = base;
        while threshold < count {
            d += 1;
            threshold = threshold.saturating_mul(base);
        }
        d
    };
    let whole_hint_count = base.pow(digits_needed as u32);
    let short_hint_count = whole_hint_count.saturating_sub(count) / base;
    let long_hint_count = count - short_hint_count;
    (digits_needed, long_hint_count, short_hint_count)
}

fn number_to_hint_string_reversed(
    mut number: usize,
    character_set: &[char],
    num_hint_digits: usize,
) -> String {
    let divisor = character_set.len().max(1);
    let mut hint = String::new();
    loop {
        let remainder = number % divisor;
        hint.insert(0, character_set[remainder]);
        number = number.saturating_sub(remainder) / divisor;
        if number == 0 {
            break;
        }
    }
    let length = hint.len();
    let pad = num_hint_digits.saturating_sub(length);
    for _ in 0..pad {
        hint.insert(0, character_set[0]);
    }
    hint.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn hap_alphabet() -> Vec<char> {
        "SADFJKLEWCMPGH".chars().collect()
    }

    #[test]
    fn get_hint_strings_zero() {
        let a = hap_alphabet();
        let hints = generate_labels(0, &a);
        assert!(hints.is_empty());
    }

    #[test]
    fn get_hint_strings_one() {
        let a = hap_alphabet();
        let hints = generate_labels(1, &a);
        assert_eq!(hints.len(), 1);
        assert_eq!(&*hints[0], "S");
    }

    #[test]
    fn get_hint_strings_fourteen() {
        let a = hap_alphabet();
        let hints = generate_labels(14, &a);
        assert_eq!(hints.len(), 14);
        for (h, ch) in hints.iter().zip(a.iter()) {
            assert_eq!(&**h, ch.to_string());
        }
    }

    #[test]
    fn get_hint_strings_fifteen() {
        let a = hap_alphabet();
        let hints = generate_labels(15, &a);
        assert_eq!(hints.len(), 15);
        let lengths: Vec<usize> = hints.iter().map(|s| s.len()).collect();
        let singles = lengths.iter().filter(|&&l| l == 1).count();
        let doubles = lengths.iter().filter(|&&l| l == 2).count();
        assert_eq!(singles + doubles, 15);
        assert_eq!(singles, 12);
        assert_eq!(doubles, 3);
    }

    #[test]
    fn get_hint_strings_196() {
        let a = hap_alphabet();
        let hints = generate_labels(196, &a);
        assert_eq!(hints.len(), 196);
        assert!(hints.iter().all(|s| s.len() == 2));
        assert_eq!(hints.iter().collect::<HashSet<_>>().len(), 196);
    }

    #[test]
    fn get_hint_strings_197() {
        let a = hap_alphabet();
        let hints = generate_labels(197, &a);
        assert_eq!(hints.len(), 197);
        let n3 = hints.iter().filter(|s| s.len() == 3).count();
        let n2 = hints.iter().filter(|s| s.len() == 2).count();
        assert_eq!(n2 + n3, 197);
    }

    #[test]
    fn get_hint_strings_unique_256_legacy() {
        let a = hap_alphabet();
        let hints = generate_labels(256, &a);
        assert_eq!(hints.len(), 256);
        assert_eq!(hints.iter().collect::<HashSet<_>>().len(), 256);
    }

    #[test]
    fn alphabet_len_two() {
        let a = vec!['a', 'b'];
        let hints = generate_labels(5, &a);
        assert_eq!(hints.len(), 5);
        assert_eq!(hints.iter().collect::<HashSet<_>>().len(), 5);
    }

    #[test]
    fn n_1000_and_5000_counts() {
        let a = hap_alphabet();
        assert_eq!(generate_labels(1000, &a).len(), 1000);
        assert_eq!(generate_labels(5000, &a).len(), 5000);
    }

    #[test]
    fn n_1024_unique_mixed_lengths() {
        let a = hap_alphabet();
        let hints = generate_labels(1024, &a);
        assert_eq!(hints.len(), 1024);
        assert_eq!(hints.iter().collect::<HashSet<_>>().len(), 1024);
    }
}
