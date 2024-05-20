// Copyright 2024 Ulvetanna Inc.
use cfg_if::cfg_if;
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use groestl_crypto::{Digest, Groestl256 as GenericGroestl256};
use rand::{thread_rng, RngCore};
use binius_hash::bs_groestl::*;
use rand::prelude::*;
use rayon::prelude::*;
use rayon::{collections::linked_list, prelude::*};
use std::{iter::repeat_with, marker::PhantomData, mem};
use binius_hash::{GroestlDigest, GroestlDigestCompression, GroestlHasher, Hasher};
use binius_field::PackedBinaryField16x8b;
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

// Function to convert a slice of PackedPrimitiveType to a slice of u8
fn packed_to_bytes(packed: &[PackedPrimitiveType]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(
            packed.as_ptr() as *const u8,
            packed.len() * std::mem::size_of::<PackedPrimitiveType>(),
        )
    }
}

fn bench_groestl(c: &mut Criterion) {
    let mut group = c.benchmark_group("groestl");
    let n_hashes = 8192;
    let input_items_length = 131072;
    let size_of_packed_primitive = 16; // bytes
    let size_of_each_digest = 32; // bytes

    let default_input_value = PackedPrimitiveType {
        value: M128 { high: 0, low: 0 },
    };

    // Initialize the vector with default values using the vec! macro and an iterator
    let mut testdigests: Vec<ScaledPackedField<PackedPrimitiveType, 2>> = vec![
        ScaledPackedField {
            elements: [default_input_value, default_input_value]
        };
        n_hashes
    ];

    let mut rng = thread_rng();
    let mut testinput = vec![random_packed_primitive(&mut rng); input_items_length];

    group.throughput(Throughput::Bytes((input_items_length * std::mem::size_of::<PackedPrimitiveType>()) as u64));
    group.bench_function("Groestl256-nonbitsliced", |bench| {
        bench.iter(|| {
            testinput
                .par_chunks_exact(16) // comes out to 16 in this instance
                .map(|chunk| {
                    let chunk_bytes = packed_to_bytes(chunk);
                    GenericGroestl256::digest(chunk_bytes)
                })
                .collect::<Vec<_>>(); // Collect the results into a Vec
        });
    });

    group.finish();
}


// Function to generate a random PackedPrimitiveType
fn random_packed_primitive(rng: &mut impl Rng) -> PackedPrimitiveType {
    PackedPrimitiveType {
        value: M128 {
            high: rng.gen(),
            low: rng.gen(),
        },
    }
}

fn bench_groestl_bitsliced(c: &mut Criterion) {
	let mut group = c.benchmark_group("groestl");
	let n_hashes = 8192;
	let input_items_length = 131072;
	// let size_of_packed_primitive = std::mem::size_of::<PackedPrimitiveType>();
	let size_of_packed_primitive = 16; //bytes
	let size_of_each_digest = 32; //bytes

    let default_input_value = PackedPrimitiveType {
        value: M128 { high: 0, low: 0 },
    };

    // Initialize the vector with default values using the vec! macro and an iterator
    let mut testdigests: Vec<ScaledPackedField<PackedPrimitiveType, 2>> = vec![
        ScaledPackedField {
            elements: [default_input_value, default_input_value]
        };
        n_hashes
    ];
	let mut rng = thread_rng();
	let mut testinput = vec![random_packed_primitive(&mut rng); input_items_length];

	// const N: usize = 8192;
	// let mut data = [0u8; N];
	// rng.fill_bytes(&mut testinput);

    group.throughput(Throughput::Bytes((input_items_length * std::mem::size_of::<PackedPrimitiveType>()) as u64));
	group.bench_function("Groestl256-bitsliced", |bench| {
		bench.iter(|| {
			unsafe {
				binius_groestl_bs_hash(testdigests.as_mut_ptr(), testinput.as_mut_ptr(), input_items_length * size_of_packed_primitive, n_hashes * size_of_each_digest);
			}
		}
	)});

	group.finish()
}

fn bench_groestl_avx512(c: &mut Criterion) {
	bench_groestl_avx512_inner(c);
}
criterion_group!(hash, bench_groestl, bench_groestl_avx512, bench_groestl_bitsliced);
// criterion_group!(hash, bench_groestl, bench_groestl_avx512, bench_groestl_bitsliced);
criterion_main!(hash);
