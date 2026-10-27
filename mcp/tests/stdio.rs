use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
};
use wonderland_knowledge::workspace::{Fetcher, Resource, Source, Workspace, unique};
struct Offline;
impl Fetcher for Offline {
    fn get(&self, _: &str) -> Result<Resource, String> {
        Err("offline".into())
    }
    fn render(&self, _: &str, _: &AtomicBool) -> Result<String, String> {
        Err("offline".into())
    }
}
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn live_stdio_observes_updates_and_revocation_without_writing_or_restarting() {
    let root = Temp(std::env::temp_dir().join(format!("wla-mcp-test-{}", unique())));
    fs::create_dir_all(&root.0).unwrap();
    let file = root.0.join("notes.txt");
    fs::write(&file, "Knowledge source initial content.").unwrap();
    let workspace = Workspace::new(root.0.join("data"));
    let library = workspace.create("Local Notes", "blank").unwrap();
    workspace
        .save_source(
            &library.id,
            Source {
                id: String::new(),
                name: "Local".into(),
                kind: "files".into(),
                paths: vec![file.to_string_lossy().into()],
                url: String::new(),
                path_prefix: String::new(),
                depth: 0,
                max_pages: 1,
                dynamic: false,
            },
        )
        .unwrap();
    workspace.build(&library.id, &Offline, false).unwrap();
    let document = workspace
        .snapshot(&workspace.library(&library.id, false).unwrap())
        .unwrap()
        .documents[0]
        .id
        .clone();
    let mut child = Command::new(env!("CARGO_BIN_EXE_wla_knowledge_mcp"))
        .args(["--cache-dir", workspace.root().to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut id = 0;
    let mut call = |method: &str, params: Value| {
        id += 1;
        writeln!(
            input,
            "{}",
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
        )
        .unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], id);
        response["result"].clone()
    };
    let initialized = call("initialize", json!({}));
    assert_eq!(initialized["serverInfo"]["name"], "wla_knowledge_library");
    let tools = call("tools/list", json!({}));
    assert_eq!(tools["tools"].as_array().unwrap().len(), 7);
    assert!(
        tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|tool| tool["annotations"]["readOnlyHint"] == true)
    );
    let hidden = call(
        "tools/call",
        json!({"name":"list_libraries","arguments":{}}),
    );
    assert_eq!(hidden["structuredContent"]["results"], json!([]));
    workspace
        .update(&library.id, None, Some(true), None)
        .unwrap();
    let visible = call(
        "tools/call",
        json!({"name":"read_doc","arguments":{"key":document}}),
    );
    assert_eq!(visible["isError"], false);
    assert!(visible.to_string().contains("initial content"));
    fs::write(&file, "Replacement content after a successful update.").unwrap();
    workspace.build(&library.id, &Offline, false).unwrap();
    let updated = call(
        "tools/call",
        json!({"name":"read_doc","arguments":{"key":document}}),
    );
    assert!(updated.to_string().contains("Replacement content"));
    assert!(!updated.to_string().contains("initial content"));
    workspace
        .update(&library.id, None, Some(false), None)
        .unwrap();
    let registry_before = fs::read(workspace.root().join("registry.json")).unwrap();
    let denied = call(
        "tools/call",
        json!({"name":"read_doc","arguments":{"library_ids":[library.id],"key":document}}),
    );
    assert_eq!(denied["isError"], true);
    let listed = call(
        "tools/call",
        json!({"name":"list_libraries","arguments":{}}),
    );
    assert_eq!(listed["structuredContent"]["results"], json!([]));
    assert_eq!(
        fs::read(workspace.root().join("registry.json")).unwrap(),
        registry_before
    );
    drop(input);
    assert!(child.wait().unwrap().success());
}
