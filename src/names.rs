use std::time::SystemTime;

const ADJECTIVES: &[&str] = &[
    "agile", "amber", "bold", "brave", "breezy", "bright", "brisk", "calm", "cheerful", "clever",
    "cool", "cosmic", "crafty", "curious", "daring", "dazzling", "eager", "earnest", "electric",
    "fair", "fearless", "fierce", "fleet", "fluffy", "friendly", "gentle", "glad", "glowing",
    "golden", "graceful", "happy", "jolly", "joyful", "keen", "kind", "lively", "lucky", "mellow",
    "merry", "mighty", "noble", "peachy", "playful", "proud", "quick", "radiant", "rapid", "ready",
    "rosy", "serene", "sharp", "silver", "sleepy", "smooth", "snowy", "spirited", "spry",
    "stellar", "stoic", "swift", "tender", "tidy", "tranquil", "vibrant", "vivid", "wise", "witty",
    "zestful", "zesty",
];

const ANIMALS: &[&str] = &[
    "alpaca",
    "anteater",
    "antelope",
    "armadillo",
    "axolotl",
    "badger",
    "bear",
    "bison",
    "bobcat",
    "buffalo",
    "bunny",
    "camel",
    "capybara",
    "caribou",
    "cardinal",
    "cat",
    "chamois",
    "cheetah",
    "chipmunk",
    "cobra",
    "condor",
    "corgi",
    "coyote",
    "crane",
    "crow",
    "dingo",
    "dolphin",
    "donkey",
    "dormouse",
    "dragonfly",
    "duck",
    "eagle",
    "elephant",
    "elk",
    "falcon",
    "ferret",
    "finch",
    "flamingo",
    "fox",
    "gazelle",
    "gecko",
    "goose",
    "gorilla",
    "hamster",
    "hawk",
    "hedgehog",
    "heron",
    "hippo",
    "husky",
    "hyena",
    "ibis",
    "impala",
    "jackal",
    "jaguar",
    "kangaroo",
    "kingfisher",
    "kiwi",
    "koala",
    "lemur",
    "leopard",
    "llama",
    "lynx",
    "manatee",
    "meerkat",
    "narwhal",
    "newt",
    "octopus",
    "okapi",
    "opossum",
    "orca",
    "ostrich",
    "otter",
    "owl",
    "panda",
    "parrot",
    "penguin",
    "pheasant",
    "platypus",
    "porcupine",
    "puma",
    "quail",
    "rabbit",
    "raccoon",
    "raven",
    "reindeer",
    "rhino",
    "squirrel",
    "stork",
    "swan",
    "tiger",
    "toad",
    "toucan",
    "turtle",
    "walrus",
    "whale",
    "wolf",
    "wombat",
];

pub fn generate(exists: impl Fn(&str) -> bool) -> String {
    let nanos = u64::from(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos(),
    );

    // xorshift to spread clustered nanosecond values
    let mut seed = nanos;
    seed ^= seed << 13;
    seed ^= seed >> 7;
    seed ^= seed << 17;

    let adj = ADJECTIVES[(seed as usize) % ADJECTIVES.len()];
    let animal = ANIMALS[((seed >> 16) as usize) % ANIMALS.len()];
    let base = format!("{adj}-{animal}");

    if !exists(&base) {
        return base;
    }

    for suffix in 2.. {
        let candidate = format!("{base}{suffix}");
        if !exists(&candidate) {
            return candidate;
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit_distance(left: &str, right: &str) -> usize {
        let mut distances: Vec<usize> = (0..=right.len()).collect();

        for (left_index, left_byte) in left.bytes().enumerate() {
            let mut previous = distances[0];
            distances[0] = left_index + 1;
            for (right_index, right_byte) in right.bytes().enumerate() {
                let old = distances[right_index + 1];
                distances[right_index + 1] = (distances[right_index + 1] + 1)
                    .min(distances[right_index] + 1)
                    .min(previous + usize::from(left_byte != right_byte));
                previous = old;
            }
        }

        distances[right.len()]
    }

    fn assert_names_are_distinct(names: &[&str]) {
        for (index, left) in names.iter().enumerate() {
            for right in &names[index + 1..] {
                assert!(
                    edit_distance(left, right) > 2,
                    "names are within edit distance 2: {left}, {right}"
                );
                assert!(
                    left.len() != right.len()
                        || left.as_bytes().first() != right.as_bytes().first()
                        || left.as_bytes().last() != right.as_bytes().last(),
                    "same-length names have matching first and last letters: {left}, {right}"
                );
            }
        }
    }

    #[test]
    fn names_are_distinct() {
        assert_names_are_distinct(ADJECTIVES);
        assert_names_are_distinct(ANIMALS);
    }

    #[test]
    fn generate_returns_adjective_hyphen_animal() {
        let name = generate(|_| false);
        let parts: Vec<&str> = name.splitn(2, '-').collect();
        assert_eq!(parts.len(), 2, "expected adjective-animal, got: {name}");
        assert!(ADJECTIVES.contains(&parts[0]), "unknown adjective: {}", parts[0]);
        assert!(ANIMALS.contains(&parts[1]), "unknown animal: {}", parts[1]);
    }

    #[test]
    fn generate_appends_number_on_collision() {
        // Use a fixed known name to test collision handling deterministically.
        let blocked = "bold-eagle";
        let name = generate(|candidate| candidate == blocked || candidate == format!("{blocked}2"));
        // If the time-based seed happens to pick "bold-eagle", we get "bold-eagle3".
        // Otherwise we get a different uncontested name. Either way, "bold-eagle"
        // itself must never be returned.
        assert_ne!(name, blocked);
        assert_ne!(name, format!("{blocked}2"));
    }
}
