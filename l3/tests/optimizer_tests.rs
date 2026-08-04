use std::io;

fn run_optimized(source: &str) -> String {
    let mut output = Vec::new();
    let mut reader = io::empty();
    l3::run_pipeline_optimized(source, "<test>", &mut output, &mut reader).unwrap();
    String::from_utf8(output).unwrap().trim_end().to_string()
}

fn bytecode_optimized(source: &str) -> String {
    l3::format_bytecode_optimized(source, "<test>").unwrap()
}

#[test]
fn propagates_local_constant() {
    let source = "let a = 42;\nprintln(a)\n";
    assert_eq!(run_optimized(source), "42");
    let bytecode = bytecode_optimized(source);
    assert!(!bytecode.contains("GET_LOCAL 0"));
    assert!(bytecode.contains("CONSTANT"));
}

#[test]
fn folds_variable_arithmetic() {
    let source = "let x = 40;\nlet y = 2;\nprintln(x + y)\n";
    assert_eq!(run_optimized(source), "42");
    let bytecode = bytecode_optimized(source);
    assert!(!bytecode.contains("ADD"));
    assert!(bytecode.contains("(42)"));
}

#[test]
fn folds_reassignment() {
    let source = "let a = 42;\na = a + 1;\nprintln(a)\n";
    assert_eq!(run_optimized(source), "43");
    let bytecode = bytecode_optimized(source);
    assert!(!bytecode.contains("ADD"));
    assert!(bytecode.contains("(43)"));
}

#[test]
fn folds_string_concat_through_local() {
    let source = "let s = \"hel\";\nprintln(s + \"lo\")\n";
    assert_eq!(run_optimized(source), "hello");
    let bytecode = bytecode_optimized(source);
    assert!(!bytecode.contains("ADD"));
    assert!(bytecode.contains("(\"hello\")"));
}

#[test]
fn preserves_closure_capture_semantics() {
    let source = "let x = 5;\nlet f = fn() return x end;\nx = x + 1;\nprintln(f())\n";
    assert_eq!(run_optimized(source), "6");
}

#[test]
fn preserves_loop_constant_invalidation() {
    let source = "let i = 0;\nwhile i < 3 do\n  i = i + 1;\nend\nprintln(i)\n";
    assert_eq!(run_optimized(source), "3");
}

#[test]
fn removes_unreachable_code_after_return() {
    let source = "fn f(n)\n  while n > 0 do\n    n = n - 1;\n    return 5\n  end\n  return \
                  6\nend\nprintln(f(3))\n";
    assert_eq!(run_optimized(source), "5");
}

#[test]
fn preserves_chained_comparison() {
    let source = "let a = 1;\nlet b = 3;\nprintln(a < b and b < 5)\n";
    assert_eq!(run_optimized(source), "true");
}

#[test]
fn preserves_parameter_alignment_in_function_chunks() {
    let source = "fn f(x)\n  let mut acc = 0\n  for i in x - 1 ..= x + 1 do\n    acc = acc + i\n  \
                  end\n  return acc\nend\nprintln(f(3))\n";
    assert_eq!(run_optimized(source), "9");
}
