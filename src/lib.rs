//! Spytial is a drop-in replacement for [`std::dbg!`] that opens an
//! interactive diagram of Rust values in the browser.
//!
//! The crate-level entry points are [`dbg!`] (a strict superset of
//! [`std::dbg!`]) and [`diagram`] (no stderr, doesn't move). Both work on any
//! type that derives [`std::fmt::Debug`], [`serde::Serialize`], and
//! [`SpytialDecorators`].
//!
//! Start with the guide at <https://sidprasad.github.io/spytial-rust/> for the
//! tutorial, decorator reference, and architecture notes. The README on
//! GitHub has the elevator pitch.

#![deny(missing_docs)]

/// Serde-driven export of Rust values into the relational [`jsondata`] shape.
pub mod export;
/// Serializable atom/relation data model consumed by spytial-core.
pub mod jsondata;
/// Reconstruct Rust values from the relational [`jsondata`] shape (inverse of [`export`]).
pub mod reify;
/// SpyTial decorator types, derive-macro runtime, and YAML serialization.
pub mod spytial_annotations;

pub use export::export_json_instance;
pub use reify::{from_datum, from_datum_root, replit, replit_root, ReifyError};
// Re-export the derive macro for spatial annotations
use serde::Serialize;
pub use spytial_export_macros::SpytialDecorators;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{self, Command};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

static DIAGRAM_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Pick the path where the rendered HTML diagram should be written.
///
/// If `SPYTIAL_OUTPUT_PATH` is set, that path is used verbatim (used by the Docker
/// setup, which needs a stable filename to serve). Otherwise a unique path under
/// the OS temp dir is generated using pid + an atomic counter + nanoseconds, so
/// concurrent `dbg!` calls do not trample each other's files.
fn diagram_output_path() -> PathBuf {
    if let Ok(explicit) = env::var("SPYTIAL_OUTPUT_PATH") {
        return PathBuf::from(explicit);
    }

    let pid = process::id();
    let counter = DIAGRAM_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    env::temp_dir().join(format!("spytial-{pid}-{counter}-{nanos}.html"))
}

/// Creates a diagram of the given data structure and opens it in the browser.
///
/// This function uses **compile-time decorator collection** to automatically include
/// decorators from all nested types without requiring manual registration.
///
/// ## How it works:
/// 1. **Compile-time analysis**: The `#[derive(SpytialDecorators)]` macro analyzes the type tree
/// 2. **Automatic inclusion**: Decorators from nested types are automatically included
/// 3. **Single call**: Just call `diagram(&your_struct)` - no registration needed
///
/// ## Example:
/// ```no_run
/// use serde::Serialize;
/// use spytial::{diagram, SpytialDecorators};
///
/// #[derive(Serialize, SpytialDecorators)]
/// #[attribute(field = "name")]
/// struct Company {
///     name: String,
///     employees: Vec<Person>,  // Person's decorators automatically included
/// }
///
/// #[derive(Serialize, SpytialDecorators)]
/// #[attribute(field = "age")]
/// struct Person {
///     name: String,
///     age: u32,
/// }
///
/// let company = Company {
///     name: "Acme Corp".to_string(),
///     employees: vec![Person {
///         name: "Alice".to_string(),
///         age: 30,
///     }],
/// };
/// diagram(&company);  // Shows decorators from both Company AND Person
/// ```
pub fn diagram<T: spytial_annotations::HasSpytialDecorators + Serialize>(value: &T) {
    let spytial_spec = collect_spytial_spec_for_diagram(value);
    diagram_impl(value, &spytial_spec);
}

/// Collect SpyTial specification using compile-time decorator collection.
///
/// With the new compile-time system, calling `T::decorators()` returns decorators
/// from the type itself AND all nested types that have decorators. This eliminates
/// the need for complex runtime type discovery and registration.
fn collect_spytial_spec_for_diagram<T: spytial_annotations::HasSpytialDecorators + Serialize>(
    _value: &T,
) -> String {
    // The magic happens here: T::decorators() includes ALL decorators
    // from this type AND all nested decorated types (analyzed at compile time)
    let all_decorators = T::decorators();

    // Serialize to YAML
    spytial_annotations::to_yaml(&all_decorators).unwrap_or_default()
}

/// Creates a diagram with a custom SpyTial specification (legacy function).
///
/// This allows you to provide a custom SpyTial specification instead of using
/// the automatic compile-time decorator collection.
pub fn diagram_with_spec<T: Serialize>(value: &T, spec: &str) {
    diagram_impl(value, spec);
}

/// Strict superset of [`std::dbg!`]: prints the `Debug` representation to
/// stderr *and* opens an interactive diagram of the value in your browser.
///
/// The calling convention matches `std::dbg!` exactly, so swapping
/// `std::dbg!` for `spytial::dbg!` (or `use spytial::dbg;`) is purely
/// additive — you keep the stderr trail you already rely on and get the
/// diagram on top.
///
/// - `dbg!()` — prints the source location, opens nothing.
/// - `dbg!(expr)` — evaluates `expr`, prints `[file:line:col] expr = …` to
///   stderr (using `{:#?}`), opens a diagram in the browser, and returns
///   the value through.
/// - `dbg!(a, b, …)` — returns a tuple `(a, b, …)`. Each argument is
///   diagrammed (opens one tab per argument).
///
/// The expression's type must derive [`std::fmt::Debug`],
/// [`serde::Serialize`], and [`SpytialDecorators`]. Both owned
/// (`dbg!(x)`) and borrowed (`dbg!(&x)`) forms work.
///
/// # Examples
///
/// ```no_run
/// use spytial::{dbg, SpytialDecorators};
/// use serde::Serialize;
///
/// #[derive(Debug, Serialize, SpytialDecorators)]
/// #[attribute(field = "key")]
/// struct Node {
///     key: u32,
///     left: Option<Box<Node>>,
///     right: Option<Box<Node>>,
/// }
///
/// let tree = Node {
///     key: 5,
///     left: Some(Box::new(Node { key: 3, left: None, right: None })),
///     right: Some(Box::new(Node { key: 7, left: None, right: None })),
/// };
///
/// // Drop in for `std::dbg!`: prints Debug + opens a diagram,
/// // returns `tree` through for further use.
/// let tree = dbg!(tree);
/// ```
///
/// To suppress browser launch (CI, tests, headless runs), set
/// `SPYTIAL_NO_OPEN=1`. Stderr output is unaffected, so `cargo test`
/// captures still behave exactly like they would for `std::dbg!`.
#[macro_export]
macro_rules! dbg {
    () => {
        ::std::eprintln!(
            "[{}:{}:{}]",
            ::std::file!(),
            ::std::line!(),
            ::std::column!(),
        )
    };
    ($val:expr $(,)?) => {
        match $val {
            tmp => {
                ::std::eprintln!(
                    "[{}:{}:{}] {} = {:#?}",
                    ::std::file!(),
                    ::std::line!(),
                    ::std::column!(),
                    ::std::stringify!($val),
                    &tmp,
                );
                $crate::diagram(&tmp);
                tmp
            }
        }
    };
    ($($val:expr),+ $(,)?) => {
        ($($crate::dbg!($val)),+,)
    };
}

/// Internal implementation shared by diagram functions.
///
/// Best-effort: any failure (serialization, JSON encoding, tempfile write, browser
/// launch) is reported via `eprintln!` and execution continues. `dbg!(x)` always
/// returns `x` regardless of whether the diagram step succeeded.
fn diagram_impl<T: Serialize>(value: &T, spec: &str) {
    let json_instance = export_json_instance(value);
    let json_data = match serde_json::to_string_pretty(&json_instance) {
        Ok(json) => json,
        Err(err) => {
            eprintln!("spytial: could not encode diagram JSON, skipping: {err}");
            return;
        }
    };

    let template = include_str!("../templates/template.html");
    let rendered_html = template
        .replace(
            "/*__SPYTIAL_CORE_CSS__*/",
            include_str!("../templates/vendor/spytial-core.css"),
        )
        .replace(
            "/*__REACT_COMPONENTS_CSS__*/",
            include_str!("../templates/vendor/react-component-integration.css"),
        )
        .replace(
            "/*__SPYTIAL_CORE_JS__*/",
            include_str!("../templates/vendor/spytial-core.global.js"),
        )
        .replace(
            "/*__REACT_COMPONENTS_JS__*/",
            include_str!("../templates/vendor/react-component-integration.global.js"),
        )
        .replace("{{ json_data }}", &json_data)
        .replace("{{ spytial_spec }}", spec);

    let temp_file_path = diagram_output_path();
    if let Err(err) = fs::write(&temp_file_path, rendered_html) {
        eprintln!(
            "spytial: could not write diagram to {}: {err}",
            temp_file_path.display()
        );
        return;
    }

    let skip_browser_open = env::var("SPYTIAL_NO_OPEN")
        .map(|raw| matches!(raw.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);

    if skip_browser_open {
        eprintln!("spytial: diagram written to {}", temp_file_path.display());
        return;
    }

    #[cfg(target_os = "macos")]
    let open_cmd: Option<&str> = Some("open");
    #[cfg(target_os = "windows")]
    let open_cmd: Option<&str> = Some("start");
    #[cfg(any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    let open_cmd: Option<&str> = Some("xdg-open");
    #[cfg(not(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
    )))]
    let open_cmd: Option<&str> = None;

    let Some(open_cmd) = open_cmd else {
        eprintln!(
            "spytial: no known browser-open command for this platform. Open this file manually: {}",
            temp_file_path.display()
        );
        return;
    };

    if let Err(err) = Command::new(open_cmd).arg(&temp_file_path).spawn() {
        eprintln!(
            "spytial: failed to open browser ({err}). Open this file manually: {}",
            temp_file_path.display()
        );
    }
}
