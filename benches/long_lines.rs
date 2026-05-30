//! Benchmark based on https://github.com/Aaron1011/slow-eventsource-stream

use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use eventsource_stream2::EventStream;
use futures::{stream, TryStreamExt};
use std::hint::black_box;

const DATA_SIZE: usize = 8 * 1024 * 1024; // 8 MB

fn build_chunks(num_chunks: usize) -> Vec<Result<String, ()>> {
    let payload = "x".repeat(DATA_SIZE);
    let full_message = format!("data: {}\n\n", payload);
    #[allow(clippy::incompatible_msrv)] // I don't bench on msrv
    let chunk_size = full_message.len().div_ceil(num_chunks);

    full_message
        .as_bytes()
        .chunks(chunk_size)
        .map(|c| Ok(String::from_utf8(c.to_vec()).unwrap()))
        .collect()
}

fn bench_long_lines(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("failed to build tokio runtime");

    let mut group = c.benchmark_group("long_lines");
    group.throughput(Throughput::Bytes(DATA_SIZE as u64));

    for num_chunks in [1, 16, 256, 4096, 65536] {
        let chunks = build_chunks(num_chunks);

        group.bench_with_input(
            BenchmarkId::from_parameter(num_chunks),
            &chunks,
            |b, chunks| {
                b.to_async(&runtime).iter_batched(
                    || chunks.clone(),
                    |chunks| async move {
                        let events = EventStream::new(stream::iter(chunks))
                            .try_collect::<Vec<_>>()
                            .await
                            .unwrap();
                        debug_assert_eq!(events.len(), 1);
                        debug_assert_eq!(events[0].data.len(), DATA_SIZE);
                        #[allow(clippy::incompatible_msrv)] // I don't bench on msrv
                        black_box(events)
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_long_lines);
criterion_main!(benches);
