use super::*;
use std::sync::atomic::AtomicBool;

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("wla-knowledge-test-{}", unique()));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn workspace(&self) -> Workspace {
        Workspace::new(self.0.join("data"))
    }
    fn docs(&self) -> PathBuf {
        let path = self.0.join("sources");
        fs::create_dir_all(&path).unwrap();
        path
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Offline;
impl Fetcher for Offline {
    fn get(&self, _: &str) -> Result<Resource> {
        Err("offline fixture".into())
    }
    fn render(&self, _: &str, _: &AtomicBool) -> Result<String> {
        Err("offline fixture".into())
    }
}
fn source(path: &Path) -> Source {
    Source {
        id: String::new(),
        name: "测试资料".into(),
        kind: "directory".into(),
        paths: vec![path.to_string_lossy().into()],
        url: String::new(),
        path_prefix: String::new(),
        depth: 0,
        max_pages: 100,
        dynamic: false,
    }
}
fn prepared(temp: &Temp) -> (Workspace, Library, PathBuf) {
    let workspace = temp.workspace();
    let library = workspace.create("测试库", "blank").unwrap();
    let docs = temp.docs();
    fs::write(
        docs.join("notes.md"),
        "# 控制流\n\n取消令牌会停止采集，保留成功快照。\n\n```rust\ncancel.store(true);\n```\n",
    )
    .unwrap();
    workspace.save_source(&library.id, source(&docs)).unwrap();
    workspace.build(&library.id, &Offline, false).unwrap();
    (workspace, library, docs)
}

#[test]
fn construction_and_mcp_reads_do_not_create_storage() {
    let temp = Temp::new();
    let workspace = temp.workspace();
    assert!(workspace.scope(&[], true).unwrap().is_empty());
    assert!(!workspace.root().exists());
}

#[test]
fn libraries_are_independent_and_new_libraries_are_not_exposed() {
    let temp = Temp::new();
    let (workspace, library, docs) = prepared(&temp);
    let second = workspace.create("另一库", "blank").unwrap();
    assert_ne!(library.id, second.id);
    assert!(workspace.scope(&[], true).unwrap().is_empty());
    workspace
        .update(&library.id, None, Some(true), None)
        .unwrap();
    assert_eq!(workspace.scope(&[], true).unwrap().len(), 1);
    assert!(
        workspace
            .scope(std::slice::from_ref(&second.id), true)
            .is_err()
    );
    workspace.delete(&second.id, None, true).unwrap();
    assert!(docs.join("notes.md").exists());
    assert_eq!(
        workspace
            .snapshot(&workspace.library(&library.id, false).unwrap())
            .unwrap()
            .documents
            .len(),
        1
    );
}

#[test]
fn successful_update_invalidates_cached_search_and_keeps_current_snapshot_only() {
    let temp = Temp::new();
    let (workspace, library, docs) = prepared(&temp);
    assert!(
        !workspace
            .search(std::slice::from_ref(&library.id), "取消令牌", 10, false)
            .unwrap()
            .hits
            .is_empty()
    );
    fs::write(
        docs.join("notes.md"),
        "# 新文档\n\n量子编译器支持增量构建。",
    )
    .unwrap();
    workspace.build(&library.id, &Offline, false).unwrap();
    let result = workspace
        .search(std::slice::from_ref(&library.id), "量子编译器", 10, false)
        .unwrap();
    assert!(!result.hits.is_empty());
    assert!(result.hits[0].hit.text.contains("量子编译器"));
    assert!(
        workspace
            .search(std::slice::from_ref(&library.id), "取消令牌", 10, false)
            .unwrap()
            .hits
            .is_empty()
    );
    let snapshots = fs::read_dir(workspace.directory(&library.id).unwrap())
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("snapshot-"))
        .count();
    assert_eq!(snapshots, 1);
}

#[test]
fn complete_scan_removes_missing_documents_and_managed_assets() {
    let temp = Temp::new();
    let (workspace, library, docs) = prepared(&temp);
    let snapshot = workspace
        .snapshot(&workspace.library(&library.id, false).unwrap())
        .unwrap();
    let key = snapshot.documents[0].raw_asset.clone();
    fs::remove_file(docs.join("notes.md")).unwrap();
    workspace.build(&library.id, &Offline, false).unwrap();
    assert!(
        workspace
            .snapshot(&workspace.library(&library.id, false).unwrap())
            .unwrap()
            .documents
            .is_empty()
    );
    assert!(
        !workspace
            .directory(&library.id)
            .unwrap()
            .join("assets")
            .join(key)
            .exists()
    );
}

#[test]
fn inaccessible_directory_does_not_mean_source_removal() {
    let temp = Temp::new();
    let (workspace, library, docs) = prepared(&temp);
    fs::rename(&docs, temp.0.join("temporarily-moved")).unwrap();
    workspace.build(&library.id, &Offline, false).unwrap();
    assert_eq!(
        workspace
            .snapshot(&workspace.library(&library.id, false).unwrap())
            .unwrap()
            .documents
            .len(),
        1
    );
    assert!(!workspace.job().errors.is_empty());
}

#[test]
fn source_deletion_requires_confirmation_and_preserves_originals() {
    let temp = Temp::new();
    let (workspace, library, docs) = prepared(&temp);
    let current = workspace.library(&library.id, false).unwrap();
    let id = &current.sources[0].id;
    assert!(workspace.delete(&library.id, Some(id), false).is_err());
    workspace.delete(&library.id, Some(id), true).unwrap();
    assert!(
        workspace
            .snapshot(&workspace.library(&library.id, false).unwrap())
            .unwrap()
            .documents
            .is_empty()
    );
    assert!(docs.join("notes.md").exists());
    let assets = workspace.directory(&library.id).unwrap().join("assets");
    assert_eq!(fs::read_dir(assets).unwrap().count(), 0);
}

#[test]
fn new_library_backup_roundtrip_rebinds_paths_and_disables_exposure() {
    let temp = Temp::new();
    let (workspace, library, docs) = prepared(&temp);
    workspace
        .update(&library.id, None, Some(true), None)
        .unwrap();
    let backup = workspace.export_library(&library.id).unwrap();
    let imported = workspace.import_library(&backup).unwrap();
    assert_ne!(library.id, imported.id);
    assert!(!imported.exposed);
    assert!(imported.sources[0].paths.is_empty());
    assert!(imported.model.is_none());
    assert!(
        !workspace
            .search(std::slice::from_ref(&imported.id), "取消令牌", 10, false)
            .unwrap()
            .hits
            .is_empty()
    );
    assert!(docs.join("notes.md").exists());
    workspace.build(&imported.id, &Offline, false).unwrap();
    assert_eq!(
        workspace
            .snapshot(&workspace.library(&imported.id, false).unwrap())
            .unwrap()
            .documents
            .len(),
        1
    );
}

#[test]
fn imported_presets_reject_executable_fields() {
    let malicious =
        serde_json::json!({"id":"bad","name":"bad","sources":[],"script":"python run.py"});
    assert!(serde_json::from_value::<Preset>(malicious).is_err());
    let temp = Temp::new();
    let (workspace, library, _) = prepared(&temp);
    let current = workspace.library(&library.id, false).unwrap();
    let preset = workspace
        .save_preset(Preset {
            id: "".into(),
            name: "资料配方".into(),
            sources: current.sources,
        })
        .unwrap();
    assert!(preset.sources[0].paths.is_empty());
    let created = workspace.create("独立实例", &preset.id).unwrap();
    assert_ne!(
        created.sources[0].id,
        workspace.library(&library.id, false).unwrap().sources[0].id
    );
}

#[test]
fn markdown_html_code_tables_and_safe_links_use_the_real_parser() {
    let text = "# 示例\n\n**加粗** [来源](https://example.test/docs)\n\n```lua\nprint(42)\n```\n\n| 名称 | 类型 |\n| --- | --- |\n| count | int |";
    let (title, elements, _) =
        crate::parsing::parse("md", text, "file:///D:/notes/a.md", "a.md").unwrap();
    assert_eq!(title, "示例");
    assert!(elements.iter().any(|element|matches!(element,PageElement::Code{language,text} if language=="lua"&&text.contains("print(42)"))));
    assert!(
        elements
            .iter()
            .any(|element| matches!(element,PageElement::Table{rows,..} if rows.len()==2))
    );
    let (_, elements, _) = crate::parsing::parse(
        "html",
        "<article><h1>HTML</h1><p>A <a href='javascript:alert(1)'>link</a></p></article>",
        "https://example.test/docs/",
        "fallback",
    )
    .unwrap();
    assert!(elements.iter().all(|element| match element {
        PageElement::Paragraph { runs } => runs.iter().all(|run| run.link.is_none()),
        _ => true,
    }));
}

struct Site {
    pages: HashMap<String, (u16, String, String)>,
}
impl Fetcher for Site {
    fn get(&self, url: &str) -> Result<Resource> {
        let (status, text, location) = self.pages.get(url).ok_or("fixture page unavailable")?;
        Ok(Resource {
            status: *status,
            url: url.into(),
            headers: if location.is_empty() {
                vec![]
            } else {
                vec![("location".into(), location.clone())]
            },
            bytes: text.as_bytes().to_vec(),
        })
    }
    fn render(&self, url: &str, _: &AtomicBool) -> Result<String> {
        String::from_utf8(self.get(url)?.bytes).map_err(|error| error.to_string())
    }
}
#[test]
fn failed_and_out_of_scope_crawls_preserve_the_last_successful_document() {
    let temp = Temp::new();
    let workspace = temp.workspace();
    let library = workspace.create("网站", "blank").unwrap();
    let site_source = Source {
        id: "".into(),
        name: "网页".into(),
        kind: "website".into(),
        paths: vec![],
        url: "https://example.test/docs".into(),
        path_prefix: "/docs".into(),
        depth: 1,
        max_pages: 10,
        dynamic: false,
    };
    workspace.save_source(&library.id, site_source).unwrap();
    let site = Site {
        pages: HashMap::from([(
            "https://example.test/docs".into(),
            (
                200,
                "<main><h1>成功正文</h1><p>离线知识库</p></main>".into(),
                "".into(),
            ),
        )]),
    };
    workspace.build(&library.id, &site, false).unwrap();
    let redirect = Site {
        pages: HashMap::from([(
            "https://example.test/docs".into(),
            (302, "".into(), "/docs-other".into()),
        )]),
    };
    workspace.build(&library.id, &redirect, false).unwrap();
    assert_eq!(
        workspace
            .snapshot(&workspace.library(&library.id, false).unwrap())
            .unwrap()
            .documents[0]
            .title,
        "成功正文"
    );
    assert!(!workspace.job().errors.is_empty());
}

#[test]
fn cancellation_preserves_committed_snapshot_and_can_be_followed_by_a_new_build() {
    let temp = Temp::new();
    let (workspace, library, _) = prepared(&temp);
    workspace.cancel();
    workspace.build(&library.id, &Offline, false).unwrap();
    assert_eq!(workspace.job().state, "completed");
    let snapshot = workspace.library(&library.id, false).unwrap().snapshot;
    workspace.begin("update", Some(&library.id)).unwrap();
    workspace.cancel();
    assert!(workspace.check_cancel().is_err());
    workspace.finish(&Err("任务已取消".into()));
    assert_eq!(
        workspace.library(&library.id, false).unwrap().snapshot,
        snapshot
    );
    assert_eq!(workspace.job().state, "cancelled");
}

#[test]
fn revocation_is_visible_to_a_long_lived_read_only_workspace() {
    let temp = Temp::new();
    let (writer, library, _) = prepared(&temp);
    let reader = temp.workspace();
    writer.update(&library.id, None, Some(true), None).unwrap();
    assert_eq!(reader.scope(&[], true).unwrap().len(), 1);
    writer.update(&library.id, None, Some(false), None).unwrap();
    assert!(reader.library(&library.id, true).is_err());
    assert!(reader.scope(&[], true).unwrap().is_empty());
}

#[test]
fn large_document_reader_pages_are_bounded_and_preserve_every_character() {
    let temp = Temp::new();
    let workspace = temp.workspace();
    let library = workspace.create("Large", "blank").unwrap();
    let directory = temp.docs();
    let original = "长文档。".repeat(100000);
    fs::write(directory.join("large.txt"), &original).unwrap();
    workspace
        .save_source(&library.id, source(&directory))
        .unwrap();
    workspace.build(&library.id, &Offline, false).unwrap();
    let id = workspace
        .snapshot(&workspace.library(&library.id, false).unwrap())
        .unwrap()
        .documents[0]
        .id
        .clone();
    let mut offset = 0;
    let mut text = String::new();
    let mut pages = 0;
    loop {
        let page = workspace
            .document_page(&library.id, &id, offset, None)
            .unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() < 1024 * 1024);
        let elements: Vec<PageElement> = serde_json::from_value(page["elements"].clone()).unwrap();
        for element in elements {
            text.push_str(&crate::parsing::element_text(&element));
        }
        pages += 1;
        match page["next_offset"].as_u64() {
            Some(next) => offset = next as usize,
            None => break,
        }
    }
    assert!(pages > 1);
    assert_eq!(text, original);
}

struct Embeddings;
impl models::Transport for Embeddings {
    fn request(
        &self,
        _: &models::Service,
        _: &str,
        _: &str,
        body: Option<&serde_json::Value>,
        _: &AtomicBool,
    ) -> Result<serde_json::Value> {
        let body = body.unwrap();
        let count = body["input"].as_array().map(Vec::len).unwrap_or(1);
        Ok(
            serde_json::json!({"data":(0..count).map(|index|serde_json::json!({"index":index,"embedding":[1.0,0.5,0.25]})).collect::<Vec<_>>()}),
        )
    }
}
fn local_model(workspace: &Workspace) -> models::ModelProfile {
    let service = workspace
        .save_service(models::Service {
            id: String::new(),
            name: "Local".into(),
            kind: "openai".into(),
            url: "http://127.0.0.1:1234/v1".into(),
            secret_id: None,
            revision: String::new(),
        })
        .unwrap();
    let profile = models::ModelProfile {
        id: String::new(),
        name: "My model".into(),
        service_id: service.id,
        model: "any-new-model".into(),
        dimensions: 0,
        max_chars: 1000,
        query_prefix: String::new(),
        document_prefix: String::new(),
        revision: String::new(),
    };
    let tested = workspace
        .validate_model(
            profile,
            &["取消任务".into(), "Cancel the task".into()],
            &Embeddings,
        )
        .unwrap();
    workspace
        .save_model(serde_json::from_value(tested["profile"].clone()).unwrap())
        .unwrap()
}
#[test]
fn model_changes_only_invalidate_bound_libraries_and_keep_documents_and_keyword_search() {
    let temp = Temp::new();
    let (workspace, library, _) = prepared(&temp);
    let model = local_model(&workspace);
    let second = workspace.create("Unaffected", "blank").unwrap();
    let original = workspace.library(&second.id, false).unwrap();
    workspace
        .update(&library.id, None, Some(true), Some(Some(model.id.clone())))
        .unwrap();
    workspace
        .build_with(&library.id, &Offline, true, &Embeddings)
        .unwrap();
    let built = workspace.library(&library.id, false).unwrap();
    assert!(built.vector_revision.is_some());
    let result = workspace
        .search_with(&[], "取消", 5, true, &Embeddings)
        .unwrap();
    assert!(!result.hits.is_empty());
    let mut service = workspace.service(&model.service_id).unwrap();
    service.url = "http://127.0.0.1:1235/v1".into();
    workspace.save_service(service).unwrap();
    let changed = workspace.library(&library.id, false).unwrap();
    assert_eq!(changed.snapshot, built.snapshot);
    assert!(changed.vector_revision.is_none());
    assert_eq!(
        workspace.library(&second.id, false).unwrap().revision,
        original.revision
    );
    let result = workspace.search(&[], "取消", 5, true).unwrap();
    assert!(!result.hits.is_empty());
    assert!(!result.warnings.is_empty());
    workspace.remove_service(&model.service_id, true).unwrap();
    let changed = workspace.library(&library.id, false).unwrap();
    assert!(changed.model.is_none());
    assert_eq!(
        changed.model_requirement.as_ref().unwrap().model,
        "any-new-model"
    );
    assert_eq!(workspace.snapshot(&changed).unwrap().documents.len(), 1);
}
#[test]
fn backup_carries_model_requirements_without_service_connections_or_bindings() {
    let temp = Temp::new();
    let (workspace, library, _) = prepared(&temp);
    let model = local_model(&workspace);
    workspace
        .update(&library.id, None, None, Some(Some(model.id.clone())))
        .unwrap();
    let backup = workspace.export_library(&library.id).unwrap();
    let mut archive = zip::ZipArchive::new(fs::File::open(&backup).unwrap()).unwrap();
    let mut raw = String::new();
    use std::io::Read;
    archive
        .by_name("library.json")
        .unwrap()
        .read_to_string(&mut raw)
        .unwrap();
    assert!(!raw.contains("127.0.0.1"));
    assert!(!raw.contains("secret_id"));
    let data: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert!(data["library"]["model"].is_null());
    assert_eq!(
        data["library"]["model_requirement"]["model"],
        "any-new-model"
    );
    assert_eq!(
        data["library"]["sources"][0]["paths"],
        serde_json::json!([])
    );
    let imported = workspace.import_library(&backup).unwrap();
    assert!(imported.model.is_none());
    assert!(imported.model_requirement.is_some());
    assert!(!imported.exposed);
    assert_eq!(workspace.snapshot(&imported).unwrap().documents.len(), 1);
}
#[test]
fn old_registry_is_read_without_changes_and_first_mutation_preserves_it() {
    let temp = Temp::new();
    let (workspace, library, _) = prepared(&temp);
    let path = workspace.root().join("registry.json");
    let mut old: serde_json::Value = read_json(&path).unwrap();
    old.as_object_mut().unwrap().remove("services");
    old.as_object_mut().unwrap().remove("models");
    old["libraries"][0]["model"] = serde_json::json!("qwen3-embedding-0.6b");
    atomic(&path, &old).unwrap();
    let before = fs::read(&path).unwrap();
    let reader = Workspace::new(workspace.root().to_path_buf());
    assert_eq!(reader.registry().unwrap().libraries.len(), 1);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(
        !workspace
            .root()
            .join("registry.before-services.json")
            .exists()
    );
    reader
        .update(&library.id, Some("Renamed"), None, None)
        .unwrap();
    assert_eq!(
        fs::read(workspace.root().join("registry.before-services.json")).unwrap(),
        before
    );
    assert_eq!(
        reader
            .snapshot(&reader.library(&library.id, false).unwrap())
            .unwrap()
            .documents
            .len(),
        1
    );
}
#[test]
fn task_logs_are_complete_incremental_and_only_retained_for_the_latest_runtime_task() {
    let temp = Temp::new();
    let workspace = temp.workspace();
    workspace.begin("test", None).unwrap();
    let id = workspace.job().id;
    for i in 0..1001 {
        workspace.warning(format!("warning-{i}"));
    }
    workspace.finish(&Ok(()));
    assert_eq!(workspace.job().errors.len(), 100);
    let mut cursor = 0;
    let mut count = 0;
    loop {
        let batch = workspace.task_logs(&id, cursor).unwrap();
        let entries = batch["entries"].as_array().unwrap();
        count += entries.len();
        assert!(entries.len() <= 400);
        cursor = batch["next_cursor"].as_u64().unwrap() as usize;
        if !batch["has_more"].as_bool().unwrap() {
            break;
        }
    }
    assert_eq!(count, 1003);
    assert!(!workspace.root().exists());
    workspace.begin("next", None).unwrap();
    let reset = workspace.task_logs(&id, cursor).unwrap();
    assert_eq!(reset["reset"], true);
    assert_eq!(reset["entries"].as_array().unwrap().len(), 1);
    let restarted = temp.workspace();
    assert!(restarted.job().id.is_empty());
    assert!(
        restarted.task_logs("old", 0).unwrap()["entries"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn paginated_document_always_exposes_the_full_outline() {
    let temp = Temp::new();
    let (workspace, library, docs) = prepared(&temp);
    fs::write(
        docs.join("long.md"),
        (0..250)
            .map(|index| format!("## Heading {index}\n\nParagraph {index}\n\n"))
            .collect::<String>(),
    )
    .unwrap();
    workspace.build(&library.id, &Offline, false).unwrap();
    let document = workspace
        .snapshot(&workspace.library(&library.id, false).unwrap())
        .unwrap()
        .documents
        .into_iter()
        .find(|doc| doc.url.ends_with("long.md"))
        .unwrap();
    let page = workspace
        .document_page(&library.id, &document.id, 0, None)
        .unwrap();
    assert!(page["next_offset"].is_number());
    let outline = page["outline"].as_array().unwrap();
    assert_eq!(outline.len(), 250);
    let last = outline.last().unwrap()["anchor"].as_str().unwrap();
    let end = workspace
        .document_page(&library.id, &document.id, 0, Some(last))
        .unwrap();
    assert!(end["element_offset"].as_u64().unwrap() > 200);
    assert_eq!(end["outline"].as_array().unwrap().len(), 250);
}
