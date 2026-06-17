use serde::Serialize;
use spytial::{diagram, SpytialDecorators};

#[derive(Serialize, SpytialDecorators, Debug)]
#[attribute(field = "name")]
#[flag(name = "hideDisconnected")]
struct Company {
    name: String,
    employees: Vec<Person>,
}

/// Person type with decorators that will be automatically
/// included when processing any type that contains Person fields.
#[derive(Serialize, SpytialDecorators, Debug)]
#[attribute(field = "age")]
struct Person {
    name: String,
    age: u32,
}

fn main() {
    let company = Company {
        name: "Pemberley".to_string(),
        employees: vec![
            Person {
                name: "Elizabeth".to_string(),
                age: 20,
            },
            Person {
                name: "Darcy".to_string(),
                age: 28,
            },
        ],
    };
    // So the debug trait works sort of like we want SpyTial to work in terms of
    // serialization.
    println!("{company:#?}");

    // This call to diagram() will automatically collect decorators from:
    // 1. Company type (name attribute, hideDisconnected flag)
    // 2. Person type (age attribute) - discovered automatically at compile time!
    diagram(&company);
}
