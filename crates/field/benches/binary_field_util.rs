// Copyright 2024 Ulvetanna Inc.

use std::iter::repeat_with;

use binius_field::{BinaryField128b, BinaryField1b, BinaryField32b, ExtensionField, PackedField};
use criterion::{
	criterion_group, criterion_main, measurement::WallTime, BenchmarkGroup, Criterion,
};
use binius_field::bitsliced_mul::*;
use binius_field::bitsliced_mul::bs_mul::transpose_mul;

pub fn bench_inner_product_par<FX, PX, PY>(
	group: &mut BenchmarkGroup<WallTime>,
	name: &str,
	counts: impl Iterator<Item = usize>,
) where
	PX: PackedField<Scalar = FX>,
	PY: PackedField,
	FX: ExtensionField<PY::Scalar>,
{
	let mut rng = rand::thread_rng();
	for count in counts {
		let xs = repeat_with(|| PX::random(&mut rng))
			.take(count)
			.collect::<Vec<PX>>();
		let ys = repeat_with(|| PY::random(&mut rng))
			.take(count)
			.collect::<Vec<PY>>();
		let mut product_sum =  BinaryField128b::new(0);
		let test_a = BinaryField128b::new(273837094833434465575073686153488115946);
		let test_b = BinaryField128b::new(273837094833434465575073686153488115333);
		let mut mul_result = BinaryField128b::new(273837094833434465575073686153488112221);

		let mul_length = 64;

		let mut z_128= vec![4u128; mul_length];
		let mut y_128= vec![5u128; mul_length];
		let mut x_128= vec![0u128; mul_length];


		group.bench_function(format!("{name}/{count}"), |bench| {
			 bench.iter(|| {
				
				// for _ in 0..128{	
				// 	mul_result = mul_result * test_a; 
				// 	// product_sum = product_sum + mul_result;
				// }
				unsafe {
					transpose_mul(
						x_128.as_mut_ptr() as *mut u128,
						y_128.as_mut_ptr() as *mut u128,
						z_128.as_mut_ptr() as *mut u128,
					);
				}

			}
		);
		});
	}
}

fn inner_product_par(c: &mut Criterion) {
	let mut group = c.benchmark_group("inner_product_par");
	let counts = [64, 128usize, 512, 1024, 8192, 1 << 20];
	bench_inner_product_par::<_, BinaryField128b, BinaryField1b>(
		&mut group,
		"128bx1b",
		counts.iter().copied(),
	);
	bench_inner_product_par::<_, BinaryField128b, BinaryField32b>(
		&mut group,
		"128bx32b",
		counts.iter().copied(),
	);
	bench_inner_product_par::<_, BinaryField128b, BinaryField128b>(
		&mut group,
		"128bx128b",
		counts.iter().copied(),
	);
}

criterion_group!(binary_field_utils, inner_product_par);
criterion_main!(binary_field_utils);
