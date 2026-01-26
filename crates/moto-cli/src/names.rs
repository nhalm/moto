//! Auto-generated garage name generator.
//!
//! Generates memorable names like `bold-mongoose` or `quiet-falcon`.

use rand::prelude::IndexedRandom;

/// Adjectives for name generation.
const ADJECTIVES: &[&str] = &[
    "bold", "brave", "bright", "calm", "clever", "cool", "daring", "eager", "fancy", "fast",
    "fierce", "gentle", "golden", "grand", "happy", "keen", "kind", "lively", "lucky", "merry",
    "mighty", "noble", "proud", "quick", "quiet", "rapid", "sharp", "shiny", "silent", "sleek",
    "smart", "smooth", "snappy", "solid", "speedy", "steady", "strong", "super", "swift", "tough",
    "vivid", "warm", "wild", "wise", "witty", "zesty",
];

/// Animals for name generation.
const ANIMALS: &[&str] = &[
    "badger", "bear", "beaver", "buffalo", "camel", "cat", "cheetah", "cobra", "condor", "cougar",
    "coyote", "crane", "crow", "deer", "dingo", "dolphin", "eagle", "falcon", "ferret", "finch",
    "fox", "gecko", "goat", "goose", "gorilla", "gull", "hawk", "heron", "horse", "husky",
    "iguana", "jackal", "jaguar", "jay", "koala", "lemur", "leopard", "lion", "llama", "lynx",
    "mantis", "marten", "mongoose", "moose", "mouse", "newt", "otter", "owl", "panda", "panther",
    "parrot", "penguin", "pigeon", "puma", "python", "rabbit", "raven", "salmon", "seal", "shark",
    "snake", "spider", "squirrel", "stork", "swan", "tiger", "toucan", "turtle", "viper", "walrus",
    "weasel", "whale", "wolf", "wombat", "zebra",
];

/// Generates a random garage name in the format `adjective-animal`.
#[must_use]
pub fn generate() -> String {
    let mut rng = rand::rng();
    let adjective = ADJECTIVES.choose(&mut rng).expect("non-empty list");
    let animal = ANIMALS.choose(&mut rng).expect("non-empty list");
    format!("{adjective}-{animal}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_valid_format() {
        let name = generate();
        let parts: Vec<&str> = name.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert!(ADJECTIVES.contains(&parts[0]));
        assert!(ANIMALS.contains(&parts[1]));
    }

    #[test]
    fn generates_different_names() {
        // Generate 10 names and check they're not all the same
        let names: Vec<String> = (0..10).map(|_| generate()).collect();
        let unique: std::collections::HashSet<_> = names.iter().collect();
        // With 46 adjectives * 75 animals = 3450 combinations, 10 names should have variety
        assert!(unique.len() > 1, "expected multiple unique names");
    }
}
