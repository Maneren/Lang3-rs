use std::{fs, io::Write, path::PathBuf};

fn snapshot_dir() -> PathBuf {
    PathBuf::from("tests").join("snapshot")
}

fn input_path(name: &str) -> PathBuf {
    snapshot_dir()
        .join("inputs")
        .join(name)
        .with_extension("l3")
}

fn expected_path(name: &str, kind: &str) -> PathBuf {
    snapshot_dir()
        .join("expected")
        .join(name)
        .join(format!("{kind}.txt"))
}

fn expected_text(name: &str, kind: &str) -> String {
    fs::read_to_string(expected_path(name, kind)).unwrap_or_default()
}

fn should_update() -> bool {
    std::env::var("L3_UPDATE_SNAPSHOTS").as_deref() == Ok("1")
}

fn write_expected(name: &str, kind: &str, content: &str) {
    let path = expected_path(name, kind);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, content).unwrap();
}

fn run_update_or_check_output(name: &str) {
    let input = input_path(name);
    let source = fs::read_to_string(&input).unwrap();
    let output = run_capture(false, &source, input.to_str().unwrap());
    if should_update() {
        write_expected(name, "output", &output);
    } else {
        let expected = expected_text(name, "output");
        let actual: Vec<&str> = output.lines().collect();
        let expected_lines: Vec<&str> = expected.lines().collect();
        compare_output(name, &actual, &expected_lines);
    }
}

fn run_capture(optimized: bool, source: &str, filename: &str) -> String {
    let mut bytes = Vec::new();
    {
        let mut writer = std::io::BufWriter::new(&mut bytes);
        let mut reader = std::io::empty();
        let result = if optimized {
            l3::run_pipeline_optimized(source, filename, &mut writer, &mut reader)
        } else {
            l3::run_pipeline(source, filename, &mut writer, &mut reader)
        };
        result.unwrap();
        writer.flush().unwrap();
    }
    String::from_utf8(bytes).unwrap()
}

fn run_update_or_check_ast(name: &str) {
    let input = input_path(name);
    let source = fs::read_to_string(&input).unwrap();
    let text = l3::format_ast(&source, input.to_str().unwrap()).unwrap();
    if should_update() {
        write_expected(name, "ast", &text);
    } else {
        let expected = expected_text(name, "ast");
        compare_text(name, "ast", &text, &expected);
    }
}

fn run_update_or_check_bytecode(name: &str) {
    let input = input_path(name);
    let source = fs::read_to_string(&input).unwrap();
    let text = l3::format_bytecode(&source, input.to_str().unwrap()).unwrap();
    if should_update() {
        write_expected(name, "bytecode", &text);
    } else {
        let expected = expected_text(name, "bytecode");
        compare_text(name, "bytecode", &text, &expected);
    }
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

fn compare_output(name: &str, actual: &[&str], expected: &[&str]) {
    let actual: Vec<String> = actual.iter().map(ToString::to_string).collect();
    let expected: Vec<String> = expected.iter().map(ToString::to_string).collect();
    let actual = normalize(&actual);
    let expected = normalize(&expected);

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

macro_rules! snapshot_tests {
    ($($name:ident),* $(,)?) => {
        $(paste::item! {
            #[test] fn [<snapshot_output_ $name>]() { run_update_or_check_output(stringify!($name)); }
            #[test] fn [<snapshot_ast_ $name>]() { run_update_or_check_ast(stringify!($name)); }
            #[test] fn [<snapshot_bytecode_ $name>]() { run_update_or_check_bytecode(stringify!($name)); }
        })*
    };
}

snapshot_tests! {
    chained_comparison_sideeffect,
    closures_recursive_factory,
    closures_stateful,
    comparisons_and_logic,
    control_flow,
    currying_partial_application,
    expressions,
    functions_closures,
    indexing,
    indexing_assignments,
    indexing_bounds,
    indexing_invalid_types,
    indexing_negative,
    indexing_nested_error,
    mutable_references,
    range_for,
    recursion_direct,
    recursion_indirect,
}

#[test]
fn snapshot_optimized_output_matches_expected() {
    let mut inputs: Vec<_> = fs::read_dir(snapshot_dir().join("inputs"))
        .unwrap()
        .map(Result::unwrap)
        .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("l3"))
        .collect();
    inputs.sort_by_key(std::fs::DirEntry::file_name);
    for entry in inputs {
        let name = entry
            .path()
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let source = fs::read_to_string(entry.path()).unwrap();
        let actual = run_capture(true, &source, entry.path().to_str().unwrap());
        let actual_lines: Vec<&str> = actual.lines().collect();
        let expected = expected_text(&name, "output");
        let expected_lines: Vec<&str> = expected.lines().collect();
        compare_output(&name, &actual_lines, &expected_lines);
    }
}

#[test]
fn input_builtin_reads_lines_from_reader() {
    let source = "println(input())\nprintln(input())\nprintln(input())\n";
    let mut output = Vec::new();
    let mut input = std::io::Cursor::new("line1\nline2\nline3\n");
    l3::run_pipeline(source, "<test>", &mut output, &mut input).unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), "line1\nline2\nline3\n");
}

#[test]
fn input_builtin_strips_crlf() {
    let source = "println(input())\n";
    let mut output = Vec::new();
    let mut input = std::io::Cursor::new("line1\r\n");
    l3::run_pipeline(source, "<test>", &mut output, &mut input).unwrap();
    assert_eq!(String::from_utf8(output).unwrap(), "line1\n");
}
