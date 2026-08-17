use std::{fs, io};

use criterion::{
    BenchmarkGroup, Criterion, criterion_group, criterion_main, measurement::WallTime,
};
use l3::run_pipeline;

fn bench_l3(c: &mut BenchmarkGroup<'_, WallTime>, name: &str, file: &str) {
    let path = [env!("CARGO_MANIFEST_DIR"), "benches", "assets", file].join("/");
    let source = fs::read_to_string(&path).expect("failed to read benchmark file");

    c.bench_function(name, |b| {
        b.iter(|| {
            let mut stdout = io::sink();
            let mut stdin = io::empty();
            run_pipeline(&source, file, &mut stdout, &mut stdin).expect("l3 pipeline failed");
        });
    });
}

fn benchmarks(c: &mut Criterion) {
    let mut group = c.benchmark_group("l3_pipeline");
    for (name, file) in [
        ("arithmetic", "arithmetic.l3"),
        ("closures", "closures.l3"),
        ("fibonacci", "fib.l3"),
        ("strings", "strings.l3"),
        ("vectors", "vector.l3"),
    ] {
        bench_l3(&mut group, name, file);
    }
    group.finish();
}

criterion_group!(benches, benchmarks);
criterion_main!(benches);
