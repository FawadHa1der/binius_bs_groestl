// Copyright 2024 Ulvetanna Inc.
use cfg_if::cfg_if;
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use groestl_crypto::{Digest, Groestl256 as GenericGroestl256};
use rand::{thread_rng, RngCore};

cfg_if! {
	if #[cfg(all(target_arch = "x86_64",target_feature = "avx512bw",target_feature = "avx512vbmi",target_feature = "avx512f",target_feature = "gfni",))] {
		use binius_hash::arch::Groestl256;

		fn bench_groestl_avx512_inner(c: &mut Criterion) {
			let mut group = c.benchmark_group("groestl");

			let mut rng = thread_rng();

			const N: usize = 8192;
			let mut data = [0u8; N];
			rng.fill_bytes(&mut data);

			group.throughput(Throughput::Bytes((N) as u64));
			group.bench_function("Groestl256-AVX512", |bench| {
				bench.iter(|| Groestl256::digest(data))
			});

			group.finish()
		}

	} else {
		fn bench_groestl_avx512_inner(_c: &mut Criterion) {}
	}
}

fn bench_groestl(c: &mut Criterion) {
	let mut group = c.benchmark_group("groestl");

	let mut rng = thread_rng();

	const N: usize = 8192;
	let mut data = [0u8; N];
	rng.fill_bytes(&mut data);

	group.throughput(Throughput::Bytes(N as u64));
	group.bench_function("Groestl256-RustCrypto", |bench| {
		bench.iter(|| GenericGroestl256::digest(data));
	});

	group.finish()
}


fn bench_groestl_bitsliced(c: &mut Criterion) {
	let mut group = c.benchmark_group("groestl");

	let mut rng = thread_rng();

	const N: usize = 8192;
	let mut data = [0u8; N];
	rng.fill_bytes(&mut data);

	group.throughput(Throughput::Bytes(N as u64));
	group.bench_function("Groestl256-RustCrypto", |bench| {
		bench.iter(|| GenericGroestl256::digest(data));
	});

	group.finish()
}

fn bench_groestl_avx512(c: &mut Criterion) {
	bench_groestl_avx512_inner(c);
}

criterion_group!(hash, bench_groestl, bench_groestl_avx512);
criterion_main!(hash);
