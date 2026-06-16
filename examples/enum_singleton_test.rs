use spytial::export_json_instance;
use serde::Serialize;

#[derive(Serialize)]
enum Color {
    Red,
    Black,
}

#[derive(Serialize)]
struct Node {
    id: u32,
    color: Color,
}

fn main() {
    // Create 5 nodes: 3 Red, 2 Black
    let nodes = vec![
        Node {
            id: 1,
            color: Color::Red,
        },
        Node {
            id: 2,
            color: Color::Red,
        },
        Node {
            id: 3,
            color: Color::Black,
        },
        Node {
            id: 4,
            color: Color::Red,
        },
        Node {
            id: 5,
            color: Color::Black,
        },
    ];

    let instance = export_json_instance(&nodes);

    println!("Total atoms: {}", instance.atoms.len());
    println!("\nColor atoms:");
    for atom in &instance.atoms {
        if atom.r#type == "Color" {
            println!(
                "  - id: {}, type: {}, label: {}",
                atom.id, atom.r#type, atom.label
            );
        }
    }

    let red_count = instance
        .atoms
        .iter()
        .filter(|a| a.r#type == "Color" && a.label == "Red")
        .count();
    let black_count = instance
        .atoms
        .iter()
        .filter(|a| a.r#type == "Color" && a.label == "Black")
        .count();

    println!("\nColor atom counts:");
    println!("  Red atoms: {} (should be 1 for 3 Red values)", red_count);
    println!(
        "  Black atoms: {} (should be 1 for 2 Black values)",
        black_count
    );

    println!("\nAll atoms:");
    for atom in &instance.atoms {
        println!("  - {}: {} = {}", atom.id, atom.r#type, atom.label);
    }
}
