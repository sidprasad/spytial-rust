//! Conformance tests: what the decorators *entail*.
//!
//! `tests/export.rs` checks the shape of the datum — which atoms and relations
//! come out of `export_json_instance`. This file checks the other half of the
//! integration's job: that the spec a `SpytialDecorators` derive emits actually
//! entails the spatial facts its author meant.
//!
//! A Spytial spec does not describe a picture, it describes a set of spatial
//! relationships, and infinitely many drawings satisfy any one of them. So
//! asserting on coordinates, or diffing screenshots, tests the force simulation
//! rather than this crate — such a test fails when nothing is wrong and passes
//! when something is. Instead each case here hands spytial-core's conformance
//! harness a datum, the spec, and the facts that should follow; the harness
//! solves the constraint graph and answers. Node positions do not exist at that
//! stage, so a case is deterministic and needs no browser.
//!
//! `must.rightOf(a)` means *in every layout the spec permits*, not "where it
//! landed this time". That is what makes these stable across machines.
//!
//! The harness is `templates/vendor/spytial-check.js`, pinned by `VERSION.txt`
//! alongside the browser assets. Tests skip rather than fail when Node or the
//! harness is unavailable, so `cargo test` stays green for contributors without
//! Node and from the published crate, which excludes the harness. Set
//! `SPYTIAL_NODE` to point at a specific Node binary.
//!
//! The `cyclic()`, `sized()` and `hidden()` queries need spytial-core 4.4.2 or
//! newer. Everything else here works from 4.4.1, which is where the CLI first
//! shipped. Against an older bundle those three fail as unrecognized queries
//! rather than silently passing, so a downgrade is loud.
//!
//! Docs: <https://sidprasad.github.io/spytial-core/#/testing-integrations>

use serde::Serialize;
use serde_json::{json, Value};
use spytial::export_json_instance;
use spytial::jsondata::JsonDataInstance;
use spytial::spytial_annotations::{to_yaml, HasSpytialDecorators};
use spytial::SpytialDecorators;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

// ──────────────────────────────────────────────
// Harness plumbing
// ──────────────────────────────────────────────

/// The contract version this file was written against. The harness stamps every
/// result with it, and the docs are explicit that a host should refuse a result
/// whose version it does not recognize rather than guess at the shape — so a
/// spytial-core bump that changes the case/result JSON surfaces here as one
/// clear failure instead of a scatter of confusing assertion errors.
const EXPECTED_FORMAT_VERSION: u64 = 1;

/// Node to run the harness with: `SPYTIAL_NODE` if set, else `node` on `PATH`.
///
/// Unlike the Python integration this crate has no existing Node bridge to
/// reuse, so resolution lives here. Returning `None` means skip, never fail:
/// Node is not a build requirement of this crate and contributors without it
/// should still get a green `cargo test`.
fn node_binary() -> Option<String> {
    if let Some(explicit) = std::env::var_os("SPYTIAL_NODE") {
        let explicit = explicit.to_string_lossy().into_owned();
        if Command::new(&explicit)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Some(explicit);
        }
        // An explicit override that does not work is a mistake worth hearing
        // about — falling back to `node` would silently test something other
        // than what was asked for.
        panic!("SPYTIAL_NODE is set to {explicit:?}, which is not a working Node binary");
    }

    Command::new("node")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()
        .filter(|s| s.success())
        .map(|_| "node".to_string())
}

/// The vendored harness bundle. Absent from the published crate on purpose; see
/// the `exclude` note in `Cargo.toml`.
fn harness_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates/vendor/spytial-check.js")
}

/// Everything a run needs, or the reason there is nothing to run.
fn harness() -> Option<(String, PathBuf)> {
    let node = match node_binary() {
        Some(node) => node,
        None => {
            eprintln!("SKIP: no Node on PATH and SPYTIAL_NODE unset");
            return None;
        }
    };
    let script = harness_path();
    if !script.is_file() {
        eprintln!("SKIP: {} is missing", script.display());
        return None;
    }
    Some((node, script))
}

/// Feed a case document to `spytial-check` and return the parsed `RunResult`.
///
/// The exit code split that matters is 0/1 versus 2/3, not zero versus
/// non-zero. On 0 (all passed) and 1 (some failed) the harness reached a
/// verdict and stdout holds a `RunResult`; on 2 (bad usage or unreadable
/// input) and 3 (timed out) it never got there and stdout is empty. Treating
/// any non-zero code as "cases failed" would report a mistyped path, or a
/// selector that does not terminate, as a spec that does not hold.
fn run_cases(document: &Value) -> Value {
    let (node, script) = match harness() {
        Some(found) => found,
        None => unreachable!("callers check `harness()` before building a document"),
    };

    let mut child = Command::new(&node)
        .arg(&script)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("could not spawn {node} {}: {err}", script.display()));

    let payload = serde_json::to_vec(document).expect("case document must serialize");
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(&payload)
        .expect("harness closed stdin early");

    let out = child.wait_with_output().expect("harness did not run");
    let stderr = String::from_utf8_lossy(&out.stderr);

    let result: Value = match out.status.code() {
        // A verdict was reached; stdout is the RunResult either way.
        Some(0) | Some(1) => serde_json::from_slice(&out.stdout).unwrap_or_else(|err| {
            panic!(
                "spytial-check exited {} but stdout was not JSON: {err}\nstdout: {}\nstderr: {stderr}",
                out.status.code().unwrap(),
                String::from_utf8_lossy(&out.stdout),
            )
        }),
        // 2 = bad usage or unreadable input, 3 = timed out. Neither is a
        // statement about the spec.
        Some(code) => panic!("spytial-check could not run (exit {code}): {stderr}"),
        None => panic!("spytial-check was killed by a signal: {stderr}"),
    };

    let format_version = result["formatVersion"].as_u64();
    assert_eq!(
        format_version,
        Some(EXPECTED_FORMAT_VERSION),
        "conformance format version changed (vendored spytial-core {}); \
         re-read the case/result contract before trusting these tests",
        result["spytialCoreVersion"].as_str().unwrap_or("unknown"),
    );

    result
}

/// Build one case out of a value and the spec its type's derive emits.
///
/// `JsonDataInstance` derives `Serialize`, so the datum drops in with no
/// conversion — the harness sees exactly the bytes spytial-core would.
fn case<T>(name: &str, value: &T, assertions: Value) -> Value
where
    T: Serialize + HasSpytialDecorators,
{
    json!({
        "name": name,
        "datum": export_json_instance(value),
        "spec": to_yaml(&T::decorators()).expect("decorators must serialize to YAML"),
        "assertions": assertions,
    })
}

/// Every case runs spytial-core's datum well-formedness check as well as its
/// assertions; that check is most of the value of the harness. The harness
/// can be told to skip it (`"skipDatumCheck": true` on the case), which was
/// needed while #88 left every `idx` tuple pointing at an undeclared index
/// atom. Nothing needs it now, and a case that does should say which issue.

/// Run one case and assert it passed, rendering any failure so a red test
/// explains itself without a rerun.
fn assert_conforms(case: Value) {
    let result = run_cases(&json!({ "cases": [case] }));
    let case_result = &result["cases"][0];
    if case_result["ok"] == json!(true) {
        return;
    }
    panic!("{}", describe_failure(case_result));
}

/// Turn a failed `CaseResult` into something readable.
fn describe_failure(case_result: &Value) -> String {
    let mut report = format!(
        "conformance case {} failed\n",
        case_result["name"].as_str().unwrap_or("<unnamed>")
    );

    for diagnostic in case_result["errors"].as_array().into_iter().flatten() {
        report.push_str(&format!(
            "  error [{}] {}",
            diagnostic["code"].as_str().unwrap_or("?"),
            diagnostic["message"].as_str().unwrap_or("?"),
        ));
        if let Some(where_) = diagnostic["where"].as_str() {
            report.push_str(&format!("\n        at {where_}"));
        }
        report.push('\n');
    }

    for assertion in case_result["assertions"].as_array().into_iter().flatten() {
        if assertion["ok"] == json!(true) {
            continue;
        }
        report.push_str(&format!(
            "  failed {}\n         {}\n",
            assertion["query"].as_str().unwrap_or("?"),
            assertion["message"].as_str().unwrap_or("(no detail)"),
        ));
        if let Some(because) = assertion["because"].as_str() {
            report.push_str(&format!("         expected because {because}\n"));
        }
    }

    report
}

/// The id of the `n`th atom of a given type, in export order.
///
/// Atom ids are a bare counter, so `atom4` says nothing about which value it
/// is and shifts the moment a field is added. Naming the atom by what it is
/// keeps a case readable and keeps an unrelated edit from silently changing
/// what it asserts.
fn nth_of_type(datum: &JsonDataInstance, ty: &str, n: usize) -> String {
    let mut matching = datum.atoms.iter().filter(|a| a.r#type == ty);
    matching.nth(n).map(|a| a.id.clone()).unwrap_or_else(|| {
        let seen: Vec<&str> = datum.atoms.iter().map(|a| a.r#type.as_str()).collect();
        panic!("no {n}th atom of type {ty:?}; types present: {seen:?}")
    })
}

// ──────────────────────────────────────────────
// 1. Orientation is transitive along a linked list
// ──────────────────────────────────────────────

#[derive(Serialize, SpytialDecorators)]
#[orientation(selector = "{x : Node, y : Node | x.next = y}", directions = ["right"])]
struct Node {
    val: u32,
    next: Option<Box<Node>>,
}

fn three_node_list() -> Node {
    Node {
        val: 1,
        next: Some(Box::new(Node {
            val: 2,
            next: Some(Box::new(Node { val: 3, next: None })),
        })),
    }
}

#[test]
fn list_orientation_carries_down_the_whole_tail() {
    if harness().is_none() {
        return;
    }

    let list = three_node_list();
    let datum = export_json_instance(&list);
    let (head, mid, tail) = (
        nth_of_type(&datum, "Node", 0),
        nth_of_type(&datum, "Node", 1),
        nth_of_type(&datum, "Node", 2),
    );

    assert_conforms(case(
        "linked list",
        &list,
        json!([
            { "query": format!("must.rightOf({head})"), "contains": [&mid, &tail],
              "because": "orientation is transitive, so the whole tail is right of the head" },
            { "query": format!("must.leftOf({tail})"), "contains": [&head, &mid],
              "because": "the mirror of the same constraint" },
            { "query": format!("must.above({head})"), "empty": true,
              "because": "the spec orders horizontally only — nothing is entailed vertically" },
        ]),
    ));
}

// ──────────────────────────────────────────────
// 2. What a per-child spec does *not* say
// ──────────────────────────────────────────────

#[derive(Serialize, SpytialDecorators)]
#[orientation(selector = "{x : Tree, y : Tree | x.left = y}", directions = ["left", "below"])]
#[orientation(selector = "{x : Tree, y : Tree | x.right = y}", directions = ["right", "below"])]
struct Tree {
    val: u32,
    left: Option<Box<Tree>>,
    right: Option<Box<Tree>>,
}

/// A root whose left child has a right child — the shape where the intuition
/// "the left subtree is on the left" comes apart.
fn lopsided_tree() -> Tree {
    Tree {
        val: 0,
        left: Some(Box::new(Tree {
            val: 1,
            left: None,
            right: Some(Box::new(Tree {
                val: 2,
                left: None,
                right: None,
            })),
        })),
        right: Some(Box::new(Tree {
            val: 3,
            left: None,
            right: None,
        })),
    }
}

#[test]
fn tree_spec_does_not_constrain_a_grandchild_against_the_root() {
    if harness().is_none() {
        return;
    }

    let tree = lopsided_tree();
    let datum = export_json_instance(&tree);
    // Export order is root, left child, that child's right child, right child.
    let root = nth_of_type(&datum, "Tree", 0);
    let left_child = nth_of_type(&datum, "Tree", 1);
    let left_grandchild = nth_of_type(&datum, "Tree", 2);
    let right_child = nth_of_type(&datum, "Tree", 3);

    assert_conforms(case(
        "binary tree",
        &tree,
        json!([
            { "query": format!("must.leftOf({root})"), "contains": [&left_child],
              "because": "the left child is constrained directly against the root" },
            { "query": format!("must.rightOf({root})"), "contains": [&right_child],
              "because": "and the right child likewise" },

            // The point of the case. A left-child's right-child is constrained
            // against its own parent and nothing else, so the spec permits it
            // landing right of the root. A rendered drawing usually hides that;
            // `must.leftOf` does not. Pinned so a change in decorator semantics
            // shows up here rather than as a surprising diagram.
            { "query": format!("must.leftOf({root})"), "excludes": [&left_grandchild],
              "because": "nothing relates it to the root, so the whole left subtree is not entailed left" },

            { "query": format!("must.below({root})"), "contains": [&left_child, &right_child],
              "because": "both directions carry `below`, which the root does constrain" },
        ]),
    ));
}

// ──────────────────────────────────────────────
// 3. Identity across sharing
// ──────────────────────────────────────────────

#[derive(Serialize, SpytialDecorators)]
struct Leaf {
    tag: String,
}

#[derive(Serialize, SpytialDecorators)]
struct Shared<'a> {
    a: &'a Leaf,
    b: &'a Leaf,
}

/// Sharing is *not* collapsed, and that is deliberate.
///
/// There is no pointer identity, so one `Leaf` reachable through two fields is
/// walked twice and comes out as two atoms. The equal string value exposed by
/// Serde is one atom, however: four atoms total, not three or five.
///
/// Nothing about the datum's *shape* can catch a change here — both the shared
/// and the duplicated version are well-formed graphs — so a count assertion is
/// the only thing that pins it. Worth pinning because the alternative is a
/// silent switch between drawing a DAG and drawing a tree.
///
/// (`Rc`/`Arc` cannot stand in for `&`: they only implement `Serialize` under
/// serde's `rc` feature, which this crate does not enable, and serde documents
/// that even then the pointee is serialized once per reference.)
#[test]
fn sharing_a_composite_through_two_fields_yields_two_atoms() {
    if harness().is_none() {
        return;
    }

    let leaf = Leaf {
        tag: "shared".into(),
    };
    let shared = Shared { a: &leaf, b: &leaf };

    assert_conforms(case(
        "shared leaf",
        &shared,
        json!([
            { "query": "nodes()", "count": 4,
              "because": "one Shared, two serialized Leaf occurrences, and one equal tag string value" },
        ]),
    ));
}

// ──────────────────────────────────────────────
// 4. Option
// ──────────────────────────────────────────────

/// Every empty slot in the tree exposes the same `None` value, so each refers
/// to the same atom.
#[test]
fn equal_none_values_share_an_atom() {
    if harness().is_none() {
        return;
    }

    let tree = lopsided_tree();
    let datum = export_json_instance(&tree);
    assert_eq!(
        datum.atoms.iter().filter(|a| a.r#type == "None").count(),
        1,
        "the five empty child slots should share one None atom",
    );

    assert_conforms(case(
        "option",
        &tree,
        json!([
            { "query": format!("must.leftOf({})", nth_of_type(&datum, "None", 0)), "empty": true,
              "because": "None is a leaf of the selectors, never a Tree, so nothing is entailed about it" },
        ]),
    ));
}

// ──────────────────────────────────────────────
// 5. Vec
// ──────────────────────────────────────────────
//
// A `Vec` field does not relationalize to its elements directly. It goes
// through an intermediate `sequence` atom and a *ternary* relation,
// `idx(sequence, index, element)`. That indirection is where a selector is
// most likely to miss, so both halves are pinned: that the graph is
// connected, and that a selector can join through the index.

#[derive(Serialize, SpytialDecorators)]
struct Row {
    items: Vec<Item>,
}

#[derive(Serialize, SpytialDecorators)]
#[orientation(selector = "{x : Bag, y : Item | y in x.items.idx[u64]}", directions = ["below"])]
struct Bag {
    items: Vec<Item>,
}

#[derive(Serialize, SpytialDecorators)]
struct Item {
    n: u32,
}

/// The graph a `Vec` produces is correctly connected: the sequence atom joins
/// the container on one side and every element on the other, so the diagram a
/// user sees is right.
///
/// This is the counterweight to the test below, and the two are independent.
/// While #88 left the index positions undeclared, the elements were unreachable
/// *to a selector*, which was easy to misread as "`Vec` rendering is broken";
/// it never was, and this pins the difference — the sequence atom is a real
/// node with real edges whatever a selector can reach.
#[test]
fn vec_connects_its_container_to_its_elements() {
    if harness().is_none() {
        return;
    }

    let row = Row {
        items: vec![Item { n: 1 }, Item { n: 2 }],
    };
    let datum = export_json_instance(&row);
    let root = nth_of_type(&datum, "Row", 0);
    let sequence = nth_of_type(&datum, "sequence", 0);
    let (first, second) = (
        nth_of_type(&datum, "Item", 0),
        nth_of_type(&datum, "Item", 1),
    );

    assert_conforms(case(
        "vec graph",
        &row,
        json!([
            { "query": format!("edges({sequence})"), "contains": [&root, &first, &second],
              "because": "the sequence atom is joined to the Row by `items` and to both Items by `idx`" },
        ]),
    ));
}

/// A Vec's position is an ordinary usize value. Serde exposes usize through
/// serialize_u64, so each idx tuple names a declared u64 atom and selectors can
/// join through the relation normally.
#[test]
fn vec_elements_are_reachable_through_the_index() {
    if harness().is_none() {
        return;
    }

    let bag = Bag {
        items: vec![Item { n: 1 }, Item { n: 2 }],
    };
    let datum = export_json_instance(&bag);
    let root = nth_of_type(&datum, "Bag", 0);
    let (first, second) = (
        nth_of_type(&datum, "Item", 0),
        nth_of_type(&datum, "Item", 1),
    );

    assert_conforms(case(
        "vec",
        &bag,
        json!([
            { "query": format!("must.below({root})"), "contains": [&first, &second],
              "because": "both elements are joined to the bag through idx[u64], so both are entailed below it" },
        ]),
    ));
}

// ──────────────────────────────────────────────
// 6. Cyclic
// ──────────────────────────────────────────────

#[derive(Serialize, SpytialDecorators)]
#[cyclic(selector = "next")]
struct Ring {
    val: u32,
    next: Option<Box<Ring>>,
}

/// `cyclic(A)` reports which atoms share a cyclic fragment with `A`.
///
/// Membership is settled by what the constraint *selected*, not by what a
/// renderer drew, and it is symmetric: asking from any member returns the whole
/// fragment. Which rotation was drawn is not entailed, so the query says
/// nothing about it — that is exactly why this is testable without a browser.
///
/// The `None` terminator is in the fragment. `next` relates the last node to
/// the shared `None` atom just like any other target, so the selector picks
/// it up. Pinned deliberately: it is the kind of thing a reader would assume
/// otherwise, and a change to how `Option` relationalizes should show up here.
#[test]
fn cyclic_fragment_membership_is_symmetric() {
    if harness().is_none() {
        return;
    }

    let ring = Ring {
        val: 1,
        next: Some(Box::new(Ring {
            val: 2,
            next: Some(Box::new(Ring { val: 3, next: None })),
        })),
    };
    let datum = export_json_instance(&ring);
    let (head, mid, tail) = (
        nth_of_type(&datum, "Ring", 0),
        nth_of_type(&datum, "Ring", 1),
        nth_of_type(&datum, "Ring", 2),
    );
    let none = nth_of_type(&datum, "None", 0);

    assert_conforms(case(
        "cyclic",
        &ring,
        json!([
            { "query": format!("cyclic({head})"), "equals": [&head, &mid, &tail, &none],
              "because": "the fragment is every atom `next` reaches, including the None terminator" },
            { "query": format!("cyclic({mid})"), "equals": [&head, &mid, &tail, &none],
              "because": "membership is symmetric, so any member reports the same fragment" },
        ]),
    ));
}

// ──────────────────────────────────────────────
// 7. Size
// ──────────────────────────────────────────────

#[derive(Serialize, SpytialDecorators)]
#[size(selector = "Boxy", width = 120, height = 80)]
struct Boxy {
    val: u32,
}

/// A `size` constraint produces exactly the dimensions it asked for, so
/// `sized(w, h)` matching those numbers is an entailment check rather than an
/// observation about a particular render.
///
/// The `u32` field atom is unsized and must not match. That is the half worth
/// asserting: a selector that widened to every atom would still satisfy the
/// positive check on its own.
#[test]
fn size_applies_to_the_selected_atoms_only() {
    if harness().is_none() {
        return;
    }

    let boxy = Boxy { val: 7 };
    let datum = export_json_instance(&boxy);
    let root = nth_of_type(&datum, "Boxy", 0);
    let field = nth_of_type(&datum, "u32", 0);

    assert_conforms(case(
        "size",
        &boxy,
        json!([
            { "query": "sized(120, 80)", "equals": [&root],
              "because": "the constraint selects Boxy and asks for exactly 120x80" },
            { "query": "sized(120, 80)", "excludes": [&field],
              "because": "the u32 field atom is auto-sized, so the selector must not reach it" },
        ]),
    ));
}

// ──────────────────────────────────────────────
// 8. Hiding
// ──────────────────────────────────────────────

#[derive(Serialize, SpytialDecorators)]
#[hide_atom(selector = "u32")]
struct Hider {
    val: u32,
    tag: String,
}

/// Hiding is two facts, and only asserting both pins it.
///
/// `hidden()` reports what the directive selected, and `nodes()` no longer
/// contains it — a hidden atom is genuinely out of the drawn graph rather than
/// merely flagged. A regression that recorded the directive without applying
/// it would still satisfy the first assertion.
#[test]
fn hidden_atoms_are_reported_and_removed_from_the_graph() {
    if harness().is_none() {
        return;
    }

    let hider = Hider {
        val: 7,
        tag: "t".into(),
    };
    let datum = export_json_instance(&hider);
    let root = nth_of_type(&datum, "Hider", 0);
    let number = nth_of_type(&datum, "u32", 0);
    let text = nth_of_type(&datum, "string", 0);

    assert_conforms(case(
        "hide_atom",
        &hider,
        json!([
            { "query": "hidden()", "equals": [&number],
              "because": "the directive selects the u32 atom and nothing else" },
            { "query": "nodes()", "equals": [&root, &text],
              "because": "a hidden atom is out of the drawn graph, not just marked" },
        ]),
    ));
}

// ──────────────────────────────────────────────
// 9. Same-named records are one relation to a selector
// ──────────────────────────────────────────────
//
// `Person.name` and `Company.name` are separate records — keyed by source
// type, each with an exact header — and spytial-core 6.0 keeps records with
// distinct ids apart rather than merging them by name. A selector still sees
// one relation `name`: the union of every record carrying it. That union is
// the property the split rests on, and it is the engine's to keep, so it is
// pinned here against the engine rather than assumed from the datum's shape.
// An engine that merged by name (as 5.x did) would pass this too, and one that
// took only the first record of a name would fail the second query.

#[derive(Serialize, SpytialDecorators)]
struct Person {
    name: String,
}

#[derive(Serialize, SpytialDecorators)]
struct Company {
    name: String,
}

#[derive(Serialize, SpytialDecorators)]
#[orientation(selector = "name", directions = ["below"])]
struct Directory {
    p: Person,
    c: Company,
}

#[test]
fn a_selector_sees_the_union_of_same_named_records() {
    if harness().is_none() {
        return;
    }

    let dir = Directory {
        p: Person { name: "Ada".into() },
        c: Company {
            name: "Acme".into(),
        },
    };
    let datum = export_json_instance(&dir);
    assert_eq!(
        datum.relations.iter().filter(|r| r.name == "name").count(),
        2,
        "the datum carries two records named `name`"
    );
    let person = nth_of_type(&datum, "Person", 0);
    let company = nth_of_type(&datum, "Company", 0);
    let labelled = |label: &str| {
        datum
            .atoms
            .iter()
            .find(|a| a.label == label)
            .map(|a| a.id.clone())
            .unwrap_or_else(|| panic!("no atom labelled {label:?}"))
    };
    let (ada, acme) = (labelled("Ada"), labelled("Acme"));

    assert_conforms(case(
        "split records",
        &dir,
        json!([
            { "query": format!("must.below({person})"), "contains": [&ada],
              "because": "`name` selects Person.name's tuple" },
            { "query": format!("must.below({company})"), "contains": [&acme],
              "because": "`name` selects Company.name's tuple as well — the union of both records, not the first" },
        ]),
    ));
}
