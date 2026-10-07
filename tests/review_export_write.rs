use flashmob::{model::Project, skm::{self, WriteOptions}};
use std::path::PathBuf;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("flashmob-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

#[test]
fn directory_target_preflight_leaves_every_file_unchanged() {
    let dir = TempDir::new("directory-target");
    std::fs::create_dir(dir.0.join("buses.csv")).unwrap();
    std::fs::write(dir.0.join("project.json"), "ORIGINAL").unwrap();
    let error = skm::write_dir(&Project::sample(), &dir.0, WriteOptions { overwrite: true, protect: None }).unwrap_err();
    assert!(error.contains("buses.csv") && error.contains("not a regular file"), "{error}");
    assert_eq!(std::fs::read_to_string(dir.0.join("project.json")).unwrap(), "ORIGINAL");
    assert!(!dir.0.join("project.xml").exists());
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 2);
}

#[test]
fn hard_link_export_preserves_protected_input_content() {
    let dir = TempDir::new("hard-link");
    let input = dir.0.join("input.json");
    let output = dir.0.join("output");
    std::fs::create_dir(&output).unwrap();
    std::fs::write(&input, "ORIGINAL INPUT").unwrap();
    std::fs::hard_link(&input, output.join("project.json")).unwrap();
    skm::write_dir(&Project::sample(), &output, WriteOptions { overwrite: true, protect: Some(&input) }).unwrap();
    assert_eq!(std::fs::read_to_string(&input).unwrap(), "ORIGINAL INPUT");
    let exported: Project = serde_json::from_str(&std::fs::read_to_string(output.join("project.json")).unwrap()).unwrap();
    assert_eq!(exported.name, Project::sample().name);
    assert!(std::fs::read_dir(&output).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".flashmob-export-")));
}
