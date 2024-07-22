// Copyright 2024 Ulvetanna Inc.
use binius_field::{
	AESTowerField32b, BinaryField32b, PackedAESBinaryField32x8b, PackedBinaryField32x8b,
	PackedField,
};
use binius_hash::{
	FixedLenHasherDigest, Groestl256, HashDigest, HasherDigest, Vision32b, VisionHasher,
};
use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use groestl_crypto::{Digest, Groestl256 as GenericGroestl256};
use rand::{thread_rng, RngCore};
use std::{any::type_name, array};


use binius_hash::bs_groestl::*;
use rand::prelude::*;
use rayon::prelude::*;
use rayon::{collections::linked_list, prelude::*};
use std::{iter::repeat_with, marker::PhantomData, mem};
use binius_hash::{GroestlDigest, GroestlDigestCompression, GroestlHasher, Hasher};
use binius_field::PackedBinaryField16x8b;


fn bench_groestl(c: &mut Criterion) {
	let mut group = c.benchmark_group("groestl");

	let mut rng = thread_rng();

	const N: usize = 1 << 8;
    println!("n: {}", N);
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

// fn bench_groestl_rustcrypto(c: &mut Criterion) {
// 	let mut group = c.benchmark_group("groestl");

// 	let mut rng = thread_rng();

// 	const N: usize = 1 << 16;
// 	let mut data = [0u8; N];
// 	rng.fill_bytes(&mut data);

// 	group.throughput(Throughput::Bytes(N as u64));
// 	group.bench_function("Groestl256-RustCrypto", |bench| {
// 		bench.iter(|| GenericGroestl256::digest(data));
// 	});

// 	group.finish()
// }

// Function to convert a slice of PackedPrimitiveType to a slice of u8
// let data_bin: [PackedBinaryField32x8b; N] =
// array::from_fn(|_| PackedBinaryField32x8b::random(&mut rng));

fn packed_to_bytes(packed: &[PackedPrimitiveType]) -> &[PackedBinaryField32x8b] {
    unsafe {
        std::slice::from_raw_parts(
            packed.as_ptr() as *const PackedBinaryField32x8b,
            packed.len() * std::mem::size_of::<PackedPrimitiveType>(),
        )
    }
}


fn bench_groestl_long_data(c: &mut Criterion) {
    let mut group = c.benchmark_group("groestl");
    let n_hashes = 8192;
    let input_items_length = 131072;

    let default_input_value = PackedPrimitiveType {
        value: M128 { high: 0, low: 0 },
    };

    // Initialize the vector with default values using the vec! macro and an iterator
    let testdigests: Vec<ScaledPackedField<PackedPrimitiveType, 2>> = vec![
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
                .chunks_exact(16) // comes out to 16 in this instance, covert to chunks_exact instead of par_chunks_exact for sequential comparison 
                .enumerate()
                .map(|(index, chunk)| {
                    let chunk_bytes = packed_to_bytes(chunk);
                    // println!("chunk_bytes size: {}", chunk_bytes.len());
                    //  println!("Chunk index: {}", index);
                    HasherDigest::<_, Groestl256<_, _>>::hash(chunk_bytes)
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
            // let start_time = std::time::Instant::now();
            unsafe {
                binius_groestl_bs_hash(testdigests.as_mut_ptr(), testinput.as_mut_ptr(), input_items_length * size_of_packed_primitive, (input_items_length * size_of_packed_primitive)/n_hashes);
            }
            // let elapsed_time = start_time.elapsed();
            // println!("Elapsed time: {:?}", elapsed_time);
        }
    )});

	group.finish()
}

// fn bench_vision32(c: &mut Criterion) {
// 	let mut group = c.benchmark_group("vision");

// 	let mut rng = thread_rng();

// 	const N: usize = 1 << 14;
// 	let data = (0..N)
// 		.map(|_| BinaryField32b::random(&mut rng))
// 		.collect::<Vec<_>>();

// 	group.throughput(Throughput::Bytes((N * 4) as u64));
// 	group.bench_function(type_name::<Vision32b<BinaryField32b>>(), |bench| {
// 		bench.iter(|| FixedLenHasherDigest::<_, Vision32b<_>>::hash(data.as_slice()))
// 	});

// 	group.finish()
// }

// criterion_group!(hash, bench_groestl_long_data, bench_groestl_bitsliced, bench_groestl);
criterion_group!(hash, bench_groestl_bitsliced, bench_groestl_long_data);
criterion_main!(hash);

