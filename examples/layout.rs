//! Prints the layout table; compare.sh diffs it between stock and shared.
use shared_sync_probe::layout::{NAMES, PROBE_LAYOUT, TRAITS};

fn main() {
    println!("{:43} size align  auto traits", "type");
    for (name, e) in NAMES.iter().zip(&PROBE_LAYOUT) {
        let traits: Vec<_> = TRAITS
            .iter()
            .zip(e.traits)
            .filter(|t| t.1)
            .map(|t| *t.0)
            .collect();
        println!("{name:43} {:4} {:5}  {}", e.size, e.align, traits.join(" "));
    }
}
