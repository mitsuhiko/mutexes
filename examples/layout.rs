//! Prints the layout table; compare.sh diffs it between stock and shared.
use shared_sync_probe::layout::{LAYOUT, NAMES, TRAITS};

fn main() {
    println!("{:43} size align  auto traits", "type");
    for (name, e) in NAMES.iter().zip(&LAYOUT) {
        let traits: Vec<_> = TRAITS
            .iter()
            .zip(e.traits)
            .filter(|t| t.1)
            .map(|t| *t.0)
            .collect();
        println!("{name:43} {:4} {:5}  {}", e.size, e.align, traits.join(" "));
    }
}
