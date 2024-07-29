// Copyright 2024 Ulvetanna Inc.

use crate::{
	challenger::{CanObserve, CanSample, CanSampleBits, HashChallenger},
	linear_code::LinearCode,
	merkle_tree::MerkleTreeVCS,
	protocols::fri::{self, CommitOutput, FRIFolder, FRIVerifier, FoldRoundOutput},
	reed_solomon::reed_solomon::ReedSolomonCode,
};
use binius_field::{
	arch::OptimalUnderlier128b,
	as_packed_field::{PackScalar, PackedType},
	underlier::{Divisible, UnderlierType},
	BinaryField, BinaryField128b, BinaryField16b, BinaryField8b, ExtensionField, PackedExtension,
	PackedField, PackedFieldIndexable,
};
use binius_hash::{GroestlDigestCompression, GroestlHasher};
use binius_ntt::NTTOptions;
use rand::prelude::*;
use std::iter::repeat_with;

fn test_commit_prove_verify_success<U, F, FA>(
	folding_arity: usize,
	n_round_commitments: usize,
	log_inv_rate: usize,
) where
	U: UnderlierType + PackScalar<F> + PackScalar<FA> + PackScalar<BinaryField8b> + Divisible<u8>,
	F: BinaryField + ExtensionField<FA> + ExtensionField<BinaryField8b>,
	F: PackedField<Scalar = F>
		+ PackedExtension<BinaryField8b, PackedSubfield: PackedFieldIndexable>,
	FA: BinaryField,
	PackedType<U, F>: PackedFieldIndexable,
	PackedType<U, FA>: PackedFieldIndexable,
{
	let mut rng = StdRng::seed_from_u64(0);

	let log_dimension = folding_arity * (n_round_commitments + 1);
	let rs_code_packed = ReedSolomonCode::<PackedType<U, FA>>::new(
		log_dimension,
		log_inv_rate,
		NTTOptions::default(),
	)
	.unwrap();
	let n_test_queries = 1;

	let make_merkle_vcs = |log_len| {
		MerkleTreeVCS::<F, _, GroestlHasher<_>, _>::new(
			log_len,
			GroestlDigestCompression::<BinaryField8b>::default(),
		)
	};

	let merkle_vcs = make_merkle_vcs(rs_code_packed.log_len());
	let merkle_round_vcss = (1..=n_round_commitments)
		.map(|i| make_merkle_vcs(rs_code_packed.log_inv_rate() + i * folding_arity))
		.rev()
		.collect::<Vec<_>>();
	assert_eq!(merkle_round_vcss.len(), n_round_commitments);

	// Generate a random message
	let msg = repeat_with(|| <PackedType<U, F>>::random(&mut rng))
		.take(rs_code_packed.dim() / <PackedType<U, F>>::WIDTH)
		.collect::<Vec<_>>();

	// Prover commits the message
	let CommitOutput {
		commitment: codeword_commitment,
		committed: codeword_committed,
		codeword,
	} = fri::commit_message(&rs_code_packed, &merkle_vcs, &msg).unwrap();

	let mut challenger = <HashChallenger<_, GroestlHasher<_>>>::new();
	challenger.observe(codeword_commitment);

	// Run the prover to generate the proximity proof
	let rs_code = ReedSolomonCode::<FA>::new(
		rs_code_packed.log_dim(),
		rs_code_packed.log_inv_rate(),
		NTTOptions::default(),
	)
	.unwrap();
	let mut round_prover = FRIFolder::new(
		&rs_code,
		<PackedType<U, F>>::unpack_scalars(&codeword),
		&merkle_vcs,
		&merkle_round_vcss,
		&codeword_committed,
		folding_arity,
	)
	.unwrap();

	let mut prover_challenger = challenger.clone();
	let mut round_commitments = Vec::with_capacity(round_prover.n_rounds() - 1);
	for _i in 0..round_prover.n_rounds() {
		let challenge = prover_challenger.sample();
		let fold_round_output = round_prover.execute_fold_round(challenge).unwrap();
		match fold_round_output {
			FoldRoundOutput::NoCommitment => {}
			FoldRoundOutput::Commitment(round_commitment) => {
				prover_challenger.observe(round_commitment);
				round_commitments.push(round_commitment);
			}
		}
	}

	let (final_value, query_prover) = round_prover.finalize().unwrap();
	prover_challenger.observe(final_value);

	let query_proofs = repeat_with(|| {
		let index = prover_challenger.sample_bits(rs_code.log_len());
		query_prover.prove_query(index)
	})
	.take(n_test_queries)
	.collect::<Result<Vec<_>, _>>()
	.unwrap();

	// Now run the verifier
	let mut verifier_challenger = challenger.clone();
	let mut verifier_challenges = Vec::with_capacity(rs_code.log_dim());

	assert_eq!(round_commitments.len(), n_round_commitments);
	for commitment in round_commitments.iter() {
		verifier_challenges.append(&mut verifier_challenger.sample_vec(folding_arity));
		verifier_challenger.observe(*commitment);
	}

	verifier_challenges.append(&mut verifier_challenger.sample_vec(folding_arity));
	verifier_challenger.observe(final_value);

	let verifier = FRIVerifier::new(
		&rs_code,
		&merkle_vcs,
		&merkle_round_vcss,
		&codeword_commitment,
		&round_commitments,
		&verifier_challenges,
		final_value,
		folding_arity,
	)
	.unwrap();

	assert_eq!(query_proofs.len(), n_test_queries);
	for query_proof in query_proofs {
		let index = verifier_challenger.sample_bits(rs_code.log_len());
		verifier.verify_query(index, query_proof).unwrap();
	}
}

#[test]
fn test_commit_prove_verify_success_128b() {
	test_commit_prove_verify_success::<OptimalUnderlier128b, BinaryField128b, BinaryField16b>(
		1, 7, 2,
	);
	test_commit_prove_verify_success::<OptimalUnderlier128b, BinaryField128b, BinaryField16b>(
		2, 4, 2,
	);
	test_commit_prove_verify_success::<OptimalUnderlier128b, BinaryField128b, BinaryField16b>(
		3, 3, 2,
	);
}
