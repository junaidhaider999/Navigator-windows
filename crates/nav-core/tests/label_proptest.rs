//! Property tests for `generate_labels` (`08-hint-generation.md`).

use nav_core::generate_labels;
use proptest::prelude::*;
use std::collections::HashSet;

fn hap_alphabet() -> Vec<char> {
    "sadfjklewcmpgh".chars().collect()
}

fn prefix_free(labels: &[Box<str>]) -> bool {
    for (i, a) in labels.iter().enumerate() {
        for (j, b) in labels.iter().enumerate() {
            if i == j {
                continue;
            }
            let (shorter, longer) = if a.len() <= b.len() {
                (&**a, &**b)
            } else {
                (&**b, &**a)
            };
            if shorter.len() < longer.len() && longer.starts_with(shorter) {
                return false;
            }
        }
    }
    true
}

fn alphabet_only(labels: &[Box<str>], alphabet: &[char]) -> bool {
    let set: HashSet<char> = alphabet.iter().copied().collect();
    labels.iter().all(|s| s.chars().all(|c| set.contains(&c)))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn labels_len_and_prefix_free(n in 0usize..5000) {
        let alphabet = hap_alphabet();
        let labels = generate_labels(n, &alphabet);
        prop_assert_eq!(labels.len(), n);
        prop_assert!(prefix_free(&labels));
        prop_assert!(alphabet_only(&labels, &alphabet));
        prop_assert_eq!(labels.iter().collect::<HashSet<_>>().len(), n);
    }

    #[test]
    fn short_labels_when_small_n(n in 1usize..=14) {
        let alphabet = hap_alphabet();
        let labels = generate_labels(n, &alphabet);
        prop_assert!(labels.iter().all(|s| s.len() == 1));
    }

    #[test]
    fn length_one_or_two_when_between_a_and_a2(n in 15usize..=195) {
        let alphabet = hap_alphabet();
        let a = alphabet.len();
        prop_assume!(n > a && n <= a * a);
        let labels = generate_labels(n, &alphabet);
        prop_assert!(labels.iter().all(|s| s.len() == 1 || s.len() == 2));
    }
}
