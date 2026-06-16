use serde::Serialize;
use spytial::{dbg, SpytialDecorators};

#[derive(Debug, Serialize, SpytialDecorators)]
struct RBTree {
    root: Option<Box<RBNode>>,
}

/// RBNode in the red-black tree with decorators that will be automatically
/// included when processing any type that contains RBNode fields.
#[derive(Debug, Serialize, SpytialDecorators)]
#[attribute(field = "key")]
#[attribute(field = "color")]
#[orientation(selector="{x, y : RBNode | x->y in left}", directions=["left", "below"])]
#[orientation(selector="{x, y : RBNode | x->y in right}", directions=["right", "below"])]
#[hide_atom(selector = "Color + u32 + None")]
#[atom_color(selector = "{x : RBNode | @:(x.color) = Red}", value = "red")]
#[atom_color(selector = "{x : RBNode | @:(x.color) = Black}", value = "black")]
struct RBNode {
    key: u32,
    color: Color,
    left: Option<Box<RBNode>>,
    right: Option<Box<RBNode>>,
}

/// Color of a node in the red-black tree
/// Deriving SpytialDecorators on enums is supported - they just have empty decorators
#[derive(Serialize, SpytialDecorators, Debug, Clone, Copy, PartialEq, Eq)]
enum Color {
    Red,
    Black,
}

impl Color {
    fn flipped(self) -> Self {
        match self {
            Color::Red => Color::Black,
            Color::Black => Color::Red,
        }
    }
}

impl RBNode {
    fn new(key: u32, color: Color) -> Self {
        RBNode {
            key,
            color,
            left: None,
            right: None,
        }
    }

    fn is_red(node: &Option<Box<RBNode>>) -> bool {
        matches!(node.as_ref().map(|n| n.color), Some(Color::Red))
    }

    fn rotate_left(mut node: Box<RBNode>) -> Box<RBNode> {
        let mut new_root = node
            .right
            .take()
            .expect("rotate_left requires an existing right child");
        node.right = new_root.left.take();
        let original_color = node.color;
        node.color = Color::Red;
        new_root.color = original_color;
        new_root.left = Some(node);
        new_root
    }

    fn rotate_right(mut node: Box<RBNode>) -> Box<RBNode> {
        let mut new_root = node
            .left
            .take()
            .expect("rotate_right requires an existing left child");
        node.left = new_root.right.take();
        let original_color = node.color;
        node.color = Color::Red;
        new_root.color = original_color;
        new_root.right = Some(node);
        new_root
    }

    fn flip_colors(node: &mut Box<RBNode>) {
        node.color = node.color.flipped();
        if let Some(left) = node.left.as_mut() {
            left.color = left.color.flipped();
        }
        if let Some(right) = node.right.as_mut() {
            right.color = right.color.flipped();
        }
    }

    fn left_left_is_red(node: &RBNode) -> bool {
        match node.left.as_ref() {
            Some(left) => RBNode::is_red(&left.left),
            None => false,
        }
    }

    fn insert_node(node: Option<Box<RBNode>>, key: u32) -> Box<RBNode> {
        match node {
            None => Box::new(RBNode::new(key, Color::Red)),
            Some(mut current) => {
                if key < current.key {
                    current.left = Some(RBNode::insert_node(current.left.take(), key));
                } else if key > current.key {
                    current.right = Some(RBNode::insert_node(current.right.take(), key));
                } else {
                    current.key = key;
                }

                if RBNode::is_red(&current.right) && !RBNode::is_red(&current.left) {
                    current = RBNode::rotate_left(current);
                }

                if RBNode::is_red(&current.left) && RBNode::left_left_is_red(&current) {
                    current = RBNode::rotate_right(current);
                }

                if RBNode::is_red(&current.left) && RBNode::is_red(&current.right) {
                    RBNode::flip_colors(&mut current);
                }

                current
            }
        }
    }

    fn validate_bst(node: &Option<Box<RBNode>>, min: Option<u32>, max: Option<u32>) -> bool {
        match node {
            None => true,
            Some(current) => {
                if let Some(lower) = min {
                    if current.key <= lower {
                        return false;
                    }
                }
                if let Some(upper) = max {
                    if current.key >= upper {
                        return false;
                    }
                }

                RBNode::validate_bst(&current.left, min, Some(current.key))
                    && RBNode::validate_bst(&current.right, Some(current.key), max)
            }
        }
    }

    fn black_height(node: &Option<Box<RBNode>>) -> Option<usize> {
        match node {
            None => Some(1),
            Some(current) => {
                if current.color == Color::Red
                    && (RBNode::is_red(&current.left) || RBNode::is_red(&current.right))
                {
                    return None;
                }

                let left_height = RBNode::black_height(&current.left)?;
                let right_height = RBNode::black_height(&current.right)?;

                if left_height != right_height {
                    return None;
                }

                let black_increment = if current.color == Color::Black { 1 } else { 0 };
                Some(left_height + black_increment)
            }
        }
    }
}

impl RBTree {
    fn new() -> Self {
        RBTree { root: None }
    }

    fn insert(&mut self, key: u32) {
        self.root = Some(RBNode::insert_node(self.root.take(), key));
        if let Some(root) = self.root.as_mut() {
            root.color = Color::Black;
        }
    }

    fn is_valid(&self) -> bool {
        match self.root.as_ref() {
            None => true,
            Some(root) => {
                root.color == Color::Black
                    && RBNode::validate_bst(&self.root, None, None)
                    && RBNode::black_height(&self.root).is_some()
            }
        }
    }
}

fn main() {
    let mut tree = RBTree::new();
    for key in [41, 38, 31, 12, 19, 8, 50, 60, 55, 54, 53] {
        tree.insert(key);
    }

    assert!(tree.is_valid(), "red-black invariants should hold");

    dbg!(tree);
}
