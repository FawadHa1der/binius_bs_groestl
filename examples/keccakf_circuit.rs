// Copyright 2024 Irreducible Inc.

use anyhow::Result;
use binius_circuits::builder::ConstraintSystemBuilder;
use binius_core::{challenger::new_hasher_challenger, constraint_system};
use binius_field::{arch::OptimalUnderlier128b, BinaryField128b, BinaryField32b, BinaryField8b};
use binius_hal::make_portable_backend;
use binius_hash::{GroestlDigestCompression, GroestlHasher};
use binius_math::DefaultEvaluationDomainFactory;
use binius_utils::{
	checked_arithmetics::log2_ceil_usize, rayon::adjust_thread_pool, tracing::init_tracing,
};
use clap::{value_parser, Parser};

const LOG_ROWS_PER_PERMUTATION: usize = 11;

#[derive(Debug, Parser)]
struct Args {
	/// The number of permutations to verify.
	#[arg(short, long, default_value_t = 8, value_parser = value_parser!(u32).range(1 << 3..))]
	n_permutations: u32,
	/// The negative binary logarithm of the Reed–Solomon code rate.
	#[arg(long, default_value_t = 1, value_parser = value_parser!(u32).range(1..))]
	log_inv_rate: u32,
}

fn main() -> Result<()> {
	type U = OptimalUnderlier128b;
	const SECURITY_BITS: usize = 100;

	adjust_thread_pool()
		.as_ref()
		.expect("failed to init thread pool");

	let args = Args::parse();

	let _guard = init_tracing().expect("failed to initialize tracing");

	println!("Verifying {} Keccak-f permutations", args.n_permutations);

	let log_n_permutations = log2_ceil_usize(args.n_permutations as usize);

	let mut builder = ConstraintSystemBuilder::<U, BinaryField128b>::new_with_witness();
	let _state_out = binius_circuits::keccakf::keccakf(
		&mut builder,
		log_n_permutations + LOG_ROWS_PER_PERMUTATION,
	);

	let witness = builder
		.take_witness()
		.expect("builder created with witness");
	let constraint_system = builder.build()?;

	let domain_factory = DefaultEvaluationDomainFactory::default();
	let challenger = new_hasher_challenger::<_, GroestlHasher<_>>();
	let backend = make_portable_backend();

	let proof = constraint_system::prove::<
		U,
		BinaryField128b,
		BinaryField8b,
		BinaryField32b,
		_,
		_,
		GroestlHasher<BinaryField128b>,
		GroestlDigestCompression<BinaryField8b>,
		_,
		_,
	>(
		&constraint_system,
		args.log_inv_rate as usize,
		SECURITY_BITS,
		witness,
		&domain_factory,
		challenger.clone(),
		&backend,
	)?;

	constraint_system::verify::<BinaryField128b, BinaryField8b, BinaryField32b, _, _, _, _, _, _>(
		&constraint_system,
		args.log_inv_rate as usize,
		SECURITY_BITS,
		&domain_factory,
		proof,
		challenger.clone(),
	)?;

	Ok(())
}
