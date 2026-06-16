//! Minimal end-to-end example of `spytial::dbg!`.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example dbg_basic
//! ```
//!
//! Or headless (no browser):
//!
//! ```sh
//! SPYTIAL_NO_OPEN=1 cargo run --example dbg_basic
//! ```

use serde::Serialize;
use spytial::{dbg, SpytialDecorators};

#[derive(Debug, Serialize, SpytialDecorators)]
#[attribute(field = "key")]
struct Node {
    key: u32,
    left: Option<Box<Node>>,
    right: Option<Box<Node>>,
}

fn main() {
    let tree = Node {
        key: 5,
        left: Some(Box::new(Node {
            key: 3,
            left: None,
            right: None,
        })),
        right: Some(Box::new(Node {
            key: 7,
            left: None,
            right: None,
        })),
    };

    // Drop in for `std::dbg!`: opens a browser tab with the diagram,
    // returns the value through for further use.
    let _tree = dbg!(tree);
}
