use serde::Serialize;
use spytial::export_json_instance;

#[derive(Serialize)]
struct Node {
    value: u32,
    left: Option<u32>,
    right: Option<u32>,
}

fn main() {
    // Multiple None values
    let node1 = Node {
        value: 1,
        left: None,
        right: Some(2),
    };
    let node2 = Node {
        value: 3,
        left: None,
        right: None,
    };
    let nodes = vec![node1, node2];

    let instance = export_json_instance(&nodes);

    println!("Total atoms: {}", instance.atoms.len());
    println!("\nAtoms:");
    for atom in &instance.atoms {
        println!(
            "  - id: {}, type: {}, label: {}",
            atom.id, atom.r#type, atom.label
        );
    }

    // Count None atoms
    let none_count = instance.atoms.iter().filter(|a| a.r#type == "None").count();
    println!("\nNumber of 'None' atoms: {} (should be 1!)", none_count);

    // Test booleans too
    let bools = vec![true, false, true, true, false];
    let bool_instance = export_json_instance(&bools);

    let true_count = bool_instance
        .atoms
        .iter()
        .filter(|a| a.label == "true")
        .count();
    let false_count = bool_instance
        .atoms
        .iter()
        .filter(|a| a.label == "false")
        .count();

    println!("\nBoolean test:");
    println!("  'true' atoms: {} (should be 1!)", true_count);
    println!("  'false' atoms: {} (should be 1!)", false_count);
}
