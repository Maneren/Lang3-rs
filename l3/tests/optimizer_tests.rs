#![allow(clippy::panic_in_result_fn, reason = "test assertions panic by design")]

use std::{error::Error, io};

fn run_optimized(source: &str) -> Result<String, Box<dyn Error>> {
    let mut output = Vec::new();
    let mut reader = io::empty();
    l3::run_pipeline_optimized(source, "<test>", &mut output, &mut reader)?;
    Ok(String::from_utf8(output)?.trim_end().to_string())
}

fn bytecode_optimized(source: &str) -> Result<String, Box<dyn Error>> {
    Ok(l3::format_bytecode_optimized(source, "<test>")?)
}

#[test]
fn propagates_local_constant() -> Result<(), Box<dyn Error>> {
    let source = "let a = 42;\nprintln(a)\n";
    assert_eq!(run_optimized(source)?, "42");
    let bytecode = bytecode_optimized(source)?;
    assert!(!bytecode.contains("GET_LOCAL 0"));
    assert!(bytecode.contains("CONSTANT"));
    Ok(())
}

#[test]
fn folds_variable_arithmetic() -> Result<(), Box<dyn Error>> {
    let source = "let x = 40;\nlet y = 2;\nprintln(x + y)\n";
    assert_eq!(run_optimized(source)?, "42");
    let bytecode = bytecode_optimized(source)?;
    assert!(!bytecode.contains("ADD"));
    assert!(bytecode.contains("(42)"));
    Ok(())
}

#[test]
fn folds_reassignment() -> Result<(), Box<dyn Error>> {
    let source = "let mut a = 42;\na = a + 1;\nprintln(a)\n";
    assert_eq!(run_optimized(source)?, "43");
    let bytecode = bytecode_optimized(source)?;
    assert!(!bytecode.contains("ADD"));
    assert!(bytecode.contains("(43)"));
    Ok(())
}

#[test]
fn folds_string_concat_through_local() -> Result<(), Box<dyn Error>> {
    let source = "let s = \"hel\";\nprintln(s + \"lo\")\n";
    assert_eq!(run_optimized(source)?, "hello");
    let bytecode = bytecode_optimized(source)?;
    assert!(!bytecode.contains("ADD"));
    assert!(bytecode.contains("(\"hello\")"));
    Ok(())
}

#[test]
fn preserves_closure_capture_semantics() -> Result<(), Box<dyn Error>> {
    let source = "let mut x = 5;\nlet f = fn() return x end;\nx = x + 1;\nprintln(f())\n";
    assert_eq!(run_optimized(source)?, "6");
    Ok(())
}

#[test]
fn preserves_loop_constant_invalidation() -> Result<(), Box<dyn Error>> {
    let source = "let mut i = 0;\nwhile i < 3 do\n  i = i + 1;\nend\nprintln(i)\n";
    assert_eq!(run_optimized(source)?, "3");
    Ok(())
}

#[test]
fn removes_unreachable_code_after_return() -> Result<(), Box<dyn Error>> {
    let source = "fn f(n)\n  while n > 0 do\n    n = n - 1;\n    return 5\n  end\n  return \
                  6\nend\nprintln(f(3))\n";
    assert_eq!(run_optimized(source)?, "5");
    Ok(())
}

#[test]
fn preserves_chained_comparison() -> Result<(), Box<dyn Error>> {
    let source = "let a = 1;\nlet b = 3;\nprintln(a < b and b < 5)\n";
    assert_eq!(run_optimized(source)?, "true");
    Ok(())
}

#[test]
fn preserves_parameter_alignment_in_function_chunks() -> Result<(), Box<dyn Error>> {
    let source = "fn f(x)\n  let mut acc = 0\n  for i in x - 1 ..= x + 1 do\n    acc = acc + i\n  \
                  end\n  return acc\nend\nprintln(f(3))\n";
    assert_eq!(run_optimized(source)?, "9");
    Ok(())
}
