mod runtime;
mod tools;
fn main() {
    if let Err(error) = runtime::run() {
        eprintln!("wla_knowledge_mcp: {error}");
        std::process::exit(1);
    }
}
