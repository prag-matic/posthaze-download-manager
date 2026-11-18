use bytes::Bytes;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use posthaze_engine::writer::PartFileWriter;
use tempfile::TempDir;

const TOTAL_BYTES: u64 = 16 * 1024 * 1024;

fn bench_part_file_writer(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("part_file_writer");
    group.throughput(Throughput::Bytes(TOTAL_BYTES));

    for chunk_size in [64 * 1024usize, 128 * 1024, 256 * 1024, 512 * 1024] {
        let payload = Bytes::from(vec![0xABu8; chunk_size]);
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}KiB", chunk_size / 1024)),
            &chunk_size,
            |b, &size| {
                b.to_async(&rt).iter(|| async {
                    let dir = TempDir::new().unwrap();
                    let part_path = dir.path().join("bench.bin.part");
                    let final_path = dir.path().join("bench.bin");
                    let writer = PartFileWriter::spawn(part_path, Some(TOTAL_BYTES)).unwrap();

                    let mut offset = 0u64;
                    while offset < TOTAL_BYTES {
                        writer.write_at(offset, payload.clone()).await.unwrap();
                        offset += size as u64;
                    }
                    writer.finish(&final_path).await.unwrap();
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_part_file_writer);
criterion_main!(benches);
