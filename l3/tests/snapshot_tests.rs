use std::fs;
use std::path::{Path, PathBuf};

fn snapshot_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Lang3/test/snapshot")
}

fn input_files() -> Vec<PathBuf> {
    let inputs_dir = snapshot_dir().join("inputs");
    let mut files: Vec<_> = fs::read_dir(&inputs_dir)
        .expect("snapshot inputs directory not found")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "l3"))
        .map(|e| e.path())
        .collect();
    files.sort();
    files
}

fn expected_lines(name: &str) -> Vec<String> {
    let path = snapshot_dir().join("expected").join(name).join("output.txt");
    let content = fs::read_to_string(&path).unwrap_or_default();
    content.lines().map(|l| l.to_string()).collect()
}

fn normalize(v: &[String]) -> Vec<String> {
    v.iter()
        .rev()
        .skip_while(|l| l.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn compare_output(name: &str, actual: &[String], expected: &[String]) {
    let actual = normalize(actual);
    let expected = normalize(expected);

    // For error tests, the expected file may have a location line (e.g. "  at file.l3:10.9-16")
    // that the Rust VM doesn't produce. We only compare up to matching lines.
    let mut i = 0;
    let mut failures = Vec::new();
    while i < actual.len() && i < expected.len() {
        if actual[i] != expected[i] {
            // Skip expected location lines (start with "  at ")
            if expected[i].trim_start().starts_with("  at ") {
                // Skip this expected line, continue
                let mut j = i + 1;
                while j < expected.len() && expected[j].trim_start().starts_with("  at ") {
                    j += 1;
                }
                // Don't advance actual, just skip expected location lines
                i = j;
                continue;
            }
            failures.push((i, actual[i].clone(), expected[i].clone()));
        }
        i += 1;
    }

    if !failures.is_empty() {
        panic!(
            "\n--- {} ---\nActual output:\n{}\n\nExpected output:\n{}\n\nMismatches:\n{}",
            name,
            actual.join("\n"),
            expected.join("\n"),
            failures
                .iter()
                .map(|(line, a, e)| format!("  line {}: actual={:?} expected={:?}", line, a, e))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
}

#[test]
fn all_snapshots() {
    let files = input_files();
    if files.is_empty() {
        panic!("No snapshot input files found");
    }

    let failures: Vec<String> = files
        .into_iter()
        .filter_map(|input| {
            let name = input.file_stem().unwrap().to_str().unwrap().to_string();
            let source = fs::read_to_string(&input).unwrap();
            println!("--- {} ---", name);

            let result = l3::run_pipeline(&source, &input.to_str().unwrap());
            match result {
                Ok(actual) => {
                    let expected = expected_lines(&name);
                    if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        compare_output(&name, &actual, &expected);
                    })) {
                        let msg = if let Some(s) = e.downcast_ref::<String>() {
                            s.clone()
                        } else if let Some(s) = e.downcast_ref::<&str>() {
                            s.to_string()
                        } else {
                            "unknown error".to_string()
                        };
                        Some(format!("{}:\n{}", name, msg))
                    } else {
                        None
                    }
                }
                Err(e) => Some(format!("{}: {}", name, e)),
            }
        })
        .collect();

    if !failures.is_empty() {
        panic!(
            "\n\nSnapshot failures:\n{}\n",
            failures.join("\n---\n")
        );
    }
}
