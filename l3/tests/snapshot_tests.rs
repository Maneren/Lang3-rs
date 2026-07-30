use std::fs;
use std::path::PathBuf;

fn snapshot_dir() -> PathBuf {
    println!("cwd: {}", std::env::current_dir().unwrap().display());
    PathBuf::from("tests").join("snapshot")
}

fn input_files() -> Vec<PathBuf> {
    let inputs_dir = snapshot_dir().join("inputs");
    let mut files: Vec<_> = fs::read_dir(&inputs_dir)
        .expect("snapshot inputs directory not found")
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "l3"))
        .map(|e| e.path())
        .collect();
    files.sort();
    files
}

fn expected_text(name: &str, kind: &str) -> String {
    let path = snapshot_dir()
        .join("expected")
        .join(name)
        .join(format!("{kind}.txt"));
    fs::read_to_string(&path).unwrap_or_default()
}

fn should_update() -> bool {
    std::env::var("L3_UPDATE_SNAPSHOTS").as_deref() == Ok("1")
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

    let mut i = 0;
    let mut failures = Vec::new();
    while i < actual.len() && i < expected.len() {
        if actual[i] != expected[i] {
            if expected[i].trim_start().starts_with("  at ") {
                let mut j = i + 1;
                while j < expected.len() && expected[j].trim_start().starts_with("  at ") {
                    j += 1;
                }
                i = j;
                continue;
            }
            failures.push((i, actual[i].clone(), expected[i].clone()));
        }
        i += 1;
    }

    assert!(
        failures.is_empty(),
        "\n--- {name} ---\nActual output:\n{}\n\nExpected output:\n{}\n\nMismatches:\n{}",
        actual.join("\n"),
        expected.join("\n"),
        failures
            .iter()
            .map(|(line, a, e)| format!("  line {line}: actual={a:?} expected={e:?}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

fn compare_text(name: &str, kind: &str, actual: &str, expected: &str) {
    let actual_lines: Vec<&str> = actual.lines().collect();
    let expected_lines: Vec<&str> = expected.lines().collect();

    let mut failures = Vec::new();
    for i in 0..actual_lines.len().max(expected_lines.len()) {
        let a = actual_lines.get(i).copied().unwrap_or("");
        let e = expected_lines.get(i).copied().unwrap_or("");
        if a != e {
            failures.push((i, a, e));
        }
    }

    assert!(
        failures.is_empty(),
        "\n--- {name} [{kind}] ---\nActual:\n{actual}\n\nExpected:\n{expected}\n\nMismatches:\n{}",
        failures
            .iter()
            .map(|(line, a, e)| format!("  line {line}: actual={a:?} expected={e:?}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

#[test]
fn all_snapshots() {
    let files = input_files();
    assert!(!files.is_empty(), "No snapshot input files found");
    let update = should_update();

    let failures: Vec<String> = files
        .into_iter()
        .filter_map(|input| {
            let name = input.file_stem().unwrap().to_str().unwrap().to_string();
            let source = fs::read_to_string(&input).unwrap();
            println!("--- {name} ---");

            let result = l3::run_pipeline(&source, input.to_str().unwrap());
            match result {
                Ok(actual) => {
                    let expected = expected_text(&name, "output");
                    if update {
                        let path = snapshot_dir()
                            .join("expected")
                            .join(&name)
                            .join("output.txt");
                        fs::create_dir_all(path.parent().unwrap()).unwrap();
                        fs::write(&path, actual.join("\n")).unwrap();
                        return None;
                    }
                    let expected_lines: Vec<String> =
                        expected.lines().map(ToString::to_string).collect();
                    if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        compare_output(&name, &actual, &expected_lines);
                    })) {
                        let msg = if let Some(s) = e.downcast_ref::<String>() {
                            s.clone()
                        } else if let Some(s) = e.downcast_ref::<&str>() {
                            s.to_string()
                        } else {
                            "unknown error".to_string()
                        };
                        return Some(format!("{name}:\n{msg}"));
                    }
                    None
                }
                Err(e) => Some(format!("{name}: {e}")),
            }
        })
        .collect();

    assert!(
        failures.is_empty(),
        "\n\nSnapshot output failures:\n{}\n",
        failures.join("\n---\n")
    );
}

#[test]
fn all_ast_snapshots() {
    let files = input_files();
    assert!(!files.is_empty(), "No snapshot input files found");
    let update = should_update();

    let failures: Vec<String> = files
        .into_iter()
        .filter_map(|input| {
            let name = input.file_stem().unwrap().to_str().unwrap().to_string();
            let source = fs::read_to_string(&input).unwrap();
            println!("--- {name} ast ---");

            match l3::format_ast(&source, input.to_str().unwrap()) {
                Ok(actual) => {
                    if update {
                        let path = snapshot_dir()
                            .join("expected")
                            .join(&name)
                            .join("ast.txt");
                        fs::create_dir_all(path.parent().unwrap()).unwrap();
                        fs::write(&path, &actual).unwrap();
                        return None;
                    }
                    let expected = expected_text(&name, "ast");
                    if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        compare_text(&name, "ast", &actual, &expected);
                    })) {
                        let msg = if let Some(s) = e.downcast_ref::<String>() {
                            s.clone()
                        } else if let Some(s) = e.downcast_ref::<&str>() {
                            s.to_string()
                        } else {
                            "unknown error".to_string()
                        };
                        return Some(format!("{name}:\n{msg}"));
                    }
                    None
                }
                Err(e) => Some(format!("{name}: {e}")),
            }
        })
        .collect();

    assert!(
        failures.is_empty(),
        "\n\nSnapshot ast failures:\n{}\n",
        failures.join("\n---\n")
    );
}

#[test]
fn all_bytecode_snapshots() {
    let files = input_files();
    assert!(!files.is_empty(), "No snapshot input files found");
    let update = should_update();

    let failures: Vec<String> = files
        .into_iter()
        .filter_map(|input| {
            let name = input.file_stem().unwrap().to_str().unwrap().to_string();
            let source = fs::read_to_string(&input).unwrap();
            println!("--- {name} bytecode ---");

            match l3::format_bytecode(&source, input.to_str().unwrap()) {
                Ok(actual) => {
                    if update {
                        let path = snapshot_dir()
                            .join("expected")
                            .join(&name)
                            .join("bytecode.txt");
                        fs::create_dir_all(path.parent().unwrap()).unwrap();
                        fs::write(&path, &actual).unwrap();
                        return None;
                    }
                    let expected = expected_text(&name, "bytecode");
                    if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        compare_text(&name, "bytecode", &actual, &expected);
                    })) {
                        let msg = if let Some(s) = e.downcast_ref::<String>() {
                            s.clone()
                        } else if let Some(s) = e.downcast_ref::<&str>() {
                            s.to_string()
                        } else {
                            "unknown error".to_string()
                        };
                        return Some(format!("{name}:\n{msg}"));
                    }
                    None
                }
                Err(e) => Some(format!("{name}: {e}")),
            }
        })
        .collect();

    assert!(
        failures.is_empty(),
        "\n\nSnapshot bytecode failures:\n{}\n",
        failures.join("\n---\n")
    );
}