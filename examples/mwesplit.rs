use std::io::{self, Read};

fn main() {
    // Read input from stdin
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .expect("Failed to read input");

    // Create MweSplit instance
    let mwesplit = cg3::MweSplit::new();

    // Process the input
    match mwesplit.run(&input) {
        Some(output) => print!("{}", output),
        None => eprintln!("Error: MweSplit returned no output"),
    }
}
