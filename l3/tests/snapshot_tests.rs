#![allow(clippy::panic_in_result_fn, reason = "test assertions panic by design")]

use std::{
    env,
    error::Error,
    fs::{self, DirEntry},
    io::{self, BufWriter, Cursor, Write as _},
    path::PathBuf,
};

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
    env::var("L3_UPDATE_SNAPSHOTS").as_deref() == Ok("1")
}

fn write_expected(name: &str, kind: &str, content: &str) -> io::Result<()> {
    let path = expected_path(name, kind);
    let Some(parent) = path.parent() else {
        return Err(io::Error::other("snapshot path has no parent"));
    };
    fs::create_dir_all(parent)?;
    fs::write(&path, content)
}

fn run_update_or_check_output(name: &str) -> Result<(), Box<dyn Error>> {
    let input = input_path(name);
    let source = fs::read_to_string(&input)?;
    let filename = input.to_str().ok_or("input path is not UTF-8")?;
    let output = run_capture(false, &source, filename)?;
    if should_update() {
        write_expected(name, "output", &output)?;
    } else {
        let expected = expected_text(name, "output");
        let actual: Vec<&str> = output.lines().collect();
        let expected_lines: Vec<&str> = expected.lines().collect();
        compare_output(name, &actual, &expected_lines);
    }
    Ok(())
}

fn run_capture(optimized: bool, source: &str, filename: &str) -> Result<String, Box<dyn Error>> {
    let mut bytes = Vec::new();
    {
        let mut writer = BufWriter::new(&mut bytes);
        let mut reader = io::empty();
        let result = if optimized {
            l3::run_pipeline_optimized(source, filename, &mut writer, &mut reader)
        } else {
            l3::run_pipeline(source, filename, &mut writer, &mut reader)
        };
        result?;
        writer.flush()?;
    }
    Ok(String::from_utf8(bytes)?)
}

fn run_update_or_check_ast(name: &str) -> Result<(), Box<dyn Error>> {
    let input = input_path(name);
    let source = fs::read_to_string(&input)?;
    let filename = input.to_str().ok_or("input path is not UTF-8")?;
    let text = l3::format_ast(&source, filename)?;
    if should_update() {
        write_expected(name, "ast", &text)?;
    } else {
        let expected = expected_text(name, "ast");
        compare_text(name, "ast", &text, &expected);
    }
    Ok(())
}

fn run_update_or_check_bytecode(name: &str) -> Result<(), Box<dyn Error>> {
    let input = input_path(name);
    let source = fs::read_to_string(&input)?;
    let filename = input.to_str().ok_or("input path is not UTF-8")?;
    let text = l3::format_bytecode(&source, filename)?;
    if should_update() {
        write_expected(name, "bytecode", &text)?;
    } else {
        let expected = expected_text(name, "bytecode");
        compare_text(name, "bytecode", &text, &expected);
    }
    Ok(())
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
    while let (Some(a), Some(e)) = (actual.get(i), expected.get(i)) {
        if a != e {
            if e.trim_start().starts_with("  at ") {
                i = (i + 1..expected.len())
                    .find(|&j| {
                        !expected
                            .get(j)
                            .is_some_and(|line| line.trim_start().starts_with("  at "))
                    })
                    .unwrap_or(expected.len());
                continue;
            }
            failures.push((i, a.clone(), e.clone()));
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
            #[test] fn [<snapshot_output_ $name>]() -> Result<(), Box<dyn Error>> {
                run_update_or_check_output(stringify!($name))?;
                Ok(())
            }
            #[test] fn [<snapshot_ast_ $name>]() -> Result<(), Box<dyn Error>> {
                run_update_or_check_ast(stringify!($name))?;
                Ok(())
            }
            #[test] fn [<snapshot_bytecode_ $name>]() -> Result<(), Box<dyn Error>> {
                run_update_or_check_bytecode(stringify!($name))?;
                Ok(())
            }
        })*
    };
}

snapshot_tests! {
    arithmetic_mixed,
    builtin_strictness,
    chained_comparison_sideeffect,
    closure_mutation,
    closures_recursive_factory,
    closures_stateful,
    comparisons_and_logic,
    control_flow,
    currying_partial_application,
    destructuring,
    equality_values,
    expressions,
    functions_closures,
    garbage_collection,
    indexing,
    indexing_assignments,
    indexing_bounds,
    indexing_invalid_types,
    indexing_negative,
    indexing_nested_error,
    loop_control_for,
    mutable_references,
    range_for,
    recursion_direct,
    recursion_indirect,
    strings_unicode,
}

#[test]
fn snapshot_optimized_output_matches_expected() -> Result<(), Box<dyn Error>> {
    let mut inputs: Vec<_> = fs::read_dir(snapshot_dir().join("inputs"))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("l3"))
        .collect();
    inputs.sort_by_key(DirEntry::file_name);
    for entry in inputs {
        let path = entry.path();
        let name = path
            .file_stem()
            .ok_or("input file has no stem")?
            .to_str()
            .ok_or("input file name is not UTF-8")?
            .to_string();
        let filename = path.to_str().ok_or("input path is not UTF-8")?;
        let source = fs::read_to_string(&path)?;
        let actual = run_capture(true, &source, filename)?;
        let actual_lines: Vec<&str> = actual.lines().collect();
        let expected = expected_text(&name, "output");
        let expected_lines: Vec<&str> = expected.lines().collect();
        compare_output(&name, &actual_lines, &expected_lines);
    }
    Ok(())
}

#[test]
fn input_builtin_reads_lines_from_reader() -> Result<(), Box<dyn Error>> {
    let source = "println(input())\nprintln(input())\nprintln(input())\n";
    let mut output = Vec::new();
    let mut input = Cursor::new("line1\nline2\nline3\n");
    l3::run_pipeline(source, "<test>", &mut output, &mut input)?;
    assert_eq!(String::from_utf8(output)?, "line1\nline2\nline3\n");
    Ok(())
}

#[test]
fn input_builtin_strips_crlf() -> Result<(), Box<dyn Error>> {
    let source = "println(input())\n";
    let mut output = Vec::new();
    let mut input = Cursor::new("line1\r\n");
    l3::run_pipeline(source, "<test>", &mut output, &mut input)?;
    assert_eq!(String::from_utf8(output)?, "line1\n");
    Ok(())
}
