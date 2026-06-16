# Running headless & in Docker

Spytial's default — open a browser tab on every `dbg!(value)` or
`diagram(&value)` call — is right for an interactive shell and wrong for
CI, remote shells, containers, and test suites. Two environment variables
cover the headless cases.

## `SPYTIAL_NO_OPEN` — suppress the browser launch

Set to `1`, `true`, or `yes` (case-insensitive) and spytial skips the
platform browser-open command, writes the HTML to disk, and prints its
path to stderr:

```sh
SPYTIAL_NO_OPEN=1 cargo run --example rbt
```
```text
spytial: diagram written to /tmp/spytial-12345-0-987654321.html
```

`dbg!`'s pretty-printed output is unaffected, so `cargo test` capture
behaves exactly as it does for `std::dbg!`.

## `SPYTIAL_OUTPUT_PATH` — pin the output filename

By default each diagram gets a unique temp path. To get a stable one — for
serving with a static HTTP server, or round-tripping off a remote machine —
set `SPYTIAL_OUTPUT_PATH`:

```sh
SPYTIAL_OUTPUT_PATH=/var/www/diagram.html cargo run
```

The value is used verbatim. Each call overwrites it, the parent directory
must already exist, and concurrent calls race — so pair it with
`SPYTIAL_NO_OPEN=1` and render one diagram at a time.

## Docker

The repository ships a `Dockerfile` and entrypoint that give you a
one-command demo without a local Rust toolchain:

```sh
docker build -t spytial .
docker run --rm -p 8080:8080 spytial        # default: the rbt example
docker run --rm -p 8080:8080 spytial demo   # any example in examples/
```

The container builds and runs the example once, then serves the rendered
HTML over a small HTTP server. Open <http://localhost:8080/rust_viz_data.html>
on your host. The image sets both variables above — `SPYTIAL_NO_OPEN=1`
via the Dockerfile (no display in the container) and
`SPYTIAL_OUTPUT_PATH=/tmp/rust_viz_data.html` via the entrypoint (the path
the server serves). The server is single-threaded and for local viewing
only.
