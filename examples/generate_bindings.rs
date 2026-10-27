use ts_rs::{Config, TS};
use wonderland_knowledge::{ListKind, PageCell, PageElement, PageRun};
fn main() {
    let cfg = Config::default();
    let mut output =
        String::from("// Generated from Rust by examples/generate_bindings.rs. Do not edit.\n");
    macro_rules! emit { ($($t:ty),*) => { $(output.push_str("export ");output.push_str(&<$t>::decl(&cfg));output.push('\n');)* }; }
    emit!(PageElement, PageRun, PageCell, ListKind);
    output = output
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/src/types.generated.ts");
    if std::env::args().any(|value| value == "--check") {
        assert_eq!(
            std::fs::read_to_string(path).expect("generated types"),
            output,
            "Bindings drift"
        );
    } else {
        std::fs::write(path, output).unwrap();
    }
}
