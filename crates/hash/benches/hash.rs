// Copyright 2024 Ulvetanna Inc.
use binius_field::{
	BinaryField32b, PackedAESBinaryField32x8b, PackedBinaryField32x8b, PackedField,
};
use binius_hash::{FixedLenHasherDigest, Groestl256, HashDigest, HasherDigest, Vision32b};
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use groestl_crypto::{Digest, Groestl256 as GenericGroestl256};
use rand::{thread_rng, RngCore};
use std::{any::type_name, array};

fn bench_groestl(c: &mut Criterion) {
	let mut group = c.benchmark_group("groestl");

	let mut rng = thread_rng();

	const N: usize = 1 << 12;
	let data_aes: [PackedAESBinaryField32x8b; N] =
		array::from_fn(|_| PackedAESBinaryField32x8b::random(&mut rng));
	let data_bin: [PackedBinaryField32x8b; N] =
		array::from_fn(|_| PackedBinaryField32x8b::random(&mut rng));

	group.throughput(Throughput::Bytes((N * PackedAESBinaryField32x8b::WIDTH) as u64));
	group.bench_function("Groestl256-Binary", |bench| {
		bench.iter(|| HasherDigest::<_, Groestl256<_, _>>::hash(data_bin));
	});

	group.bench_function("Groestl256-AES", |bench| {
		bench.iter(|| HasherDigest::<_, Groestl256<_, _>>::hash(data_aes));
	});

	group.finish()
}

fn bench_groestl_rustcrypto(c: &mut Criterion) {
	let mut group = c.benchmark_group("groestl");

	let mut rng = thread_rng();

	const N: usize = 1 << 16;
	let mut data = [0u8; N];
	rng.fill_bytes(&mut data);

	group.throughput(Throughput::Bytes(N as u64));
	group.bench_function("Groestl256-RustCrypto", |bench| {
		bench.iter(|| GenericGroestl256::digest(data));
	});

	group.finish()
}

fn bench_vision32(c: &mut Criterion) {
	let mut group = c.benchmark_group("vision");

	let mut rng = thread_rng();

	const N: usize = 1 << 14;
	let data = (0..N)
		.map(|_| BinaryField32b::random(&mut rng))
		.collect::<Vec<_>>();

	group.throughput(Throughput::Bytes((N * 4) as u64));
	group.bench_function(type_name::<Vision32b<BinaryField32b>>(), |bench| {
		bench.iter(|| FixedLenHasherDigest::<_, Vision32b<_>>::hash(data.as_slice()))
	});

	group.finish()
}

criterion_group!(hash, bench_groestl, bench_groestl_rustcrypto, bench_vision32);
criterion_main!(hash);

