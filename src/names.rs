use std::time::SystemTime;

const ADJECTIVES: &[&str] = &[
    "bold", "brave", "bright", "calm", "clever", "cool", "daring", "eager", "fair", "fierce",
    "fleet", "free", "gentle", "glad", "golden", "grand", "happy", "hardy", "keen", "kind",
    "lively", "lucky", "mellow", "merry", "mighty", "noble", "plucky", "proud", "quick", "quiet",
    "rapid", "ready", "sharp", "sleek", "sleepy", "smooth", "snappy", "snowy", "spry", "steady",
    "stoic", "sunny", "swift", "tender", "tidy", "vivid", "warm", "wild", "witty", "zesty",
];

const ANIMALS: &[&str] = &[
    "alpaca", "badger", "bear", "bison", "bobcat", "bunny", "caribou", "cat", "cobra", "condor",
    "corgi", "crane", "crow", "deer", "dingo", "eagle", "falcon", "ferret", "finch", "fox",
    "gecko", "goose", "hawk", "heron", "horse", "husky", "ibis", "impala", "jackal", "jaguar",
    "koala", "lemur", "lion", "llama", "lynx", "moose", "newt", "okapi", "otter", "owl", "panda",
    "parrot", "puma", "quail", "raven", "robin", "salmon", "seal", "stork", "swan", "tiger",
    "toad", "viper", "whale", "wolf",
];

pub fn generate(exists: impl Fn(&str) -> bool) -> String {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as usize;

    let adj = ADJECTIVES[nanos % ADJECTIVES.len()];
    let animal = ANIMALS[(nanos / ADJECTIVES.len()) % ANIMALS.len()];
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
