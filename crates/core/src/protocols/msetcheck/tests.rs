// Copyright 2024 Ulvetanna Inc.

use crate::{
	oracle::MultilinearOracleSet,
	polynomial::MultilinearExtension,
	protocols::msetcheck::{prove, verify, MsetcheckClaim, MsetcheckProveOutput, MsetcheckWitness},
	witness::{MultilinearExtensionIndex, MultilinearWitness},
};
use binius_field::{
	underlier::WithUnderlier, BinaryField128b, BinaryField16b, BinaryField32b, BinaryField64b,
	ExtensionField, Field, PackedBinaryField1x128b, PackedField, TowerField,
};
use std::iter::{successors, Step};

fn create_polynomial<F: Field + Step, PW>(
	n_vars: usize,
	stride: usize,
	reversed: bool,
) -> MultilinearWitness<'static, PW>
where
	PW: PackedField<Scalar: ExtensionField<F>>,
{
	let mut values = successors(Some(F::ZERO), |&pred| F::forward_checked(pred, stride))
		.take(1 << n_vars)
		.collect::<Vec<_>>();

	if reversed {
		values.reverse();
	}

	MultilinearExtension::from_values(values)
		.unwrap()
		.specialize_arc_dyn()
}

#[test]
fn test_prove_verify_interaction() {
	type P = PackedBinaryField1x128b;
	type F = BinaryField128b;
	type U = <P as WithUnderlier>::Underlier;

	type F1 = BinaryField16b;
	type F2 = BinaryField32b;
	type F3 = BinaryField64b;
	let n_vars = 10;

	// Setup witness
	let t1_polynomial = create_polynomial::<F1, P>(n_vars, 13, false);
	let u1_polynomial = create_polynomial::<F1, P>(n_vars, 13, true);

	let t2_polynomial = create_polynomial::<F2, P>(n_vars, 19, false);
	let u2_polynomial = create_polynomial::<F2, P>(n_vars, 19, true);

	let t3_polynomial = create_polynomial::<F3, P>(n_vars, 29, false);
	let u3_polynomial = create_polynomial::<F3, P>(n_vars, 29, true);

	let t_polynomials = [
		t1_polynomial.clone(),
		t2_polynomial.clone(),
		t3_polynomial.clone(),
	];
	let u_polynomials = [
		u1_polynomial.clone(),
		u2_polynomial.clone(),
		u3_polynomial.clone(),
	];

	let witness = MsetcheckWitness::new(t_polynomials, u_polynomials).unwrap();

	// Setup claim
	let mut oracles = MultilinearOracleSet::<F>::new();
	let round_1_batch_1_id = oracles.add_committed_batch(n_vars, F1::TOWER_LEVEL);
	let round_1_batch_2_id = oracles.add_committed_batch(n_vars, F2::TOWER_LEVEL);
	let round_1_batch_3_id = oracles.add_committed_batch(n_vars, F3::TOWER_LEVEL);
	let [t1, u1] = oracles.add_committed_multiple(round_1_batch_1_id);
	let [t2, u2] = oracles.add_committed_multiple(round_1_batch_2_id);
	let [t3, u3] = oracles.add_committed_multiple(round_1_batch_3_id);

	let t_oracles = [t1, t2, t3].map(|id| oracles.oracle(id));
	let u_oracles = [u1, u2, u3].map(|id| oracles.oracle(id));

	let claim = MsetcheckClaim::new(t_oracles, u_oracles).unwrap();

	// challenges
	let gamma = F::new(0x123);
	let alpha = F::new(0x346);

	// PROVER
	let witness_index = MultilinearExtensionIndex::<U, F>::new();

	let prove_output =
		prove(&mut oracles.clone(), witness_index, &claim, witness, gamma, Some(alpha)).unwrap();

	let MsetcheckProveOutput {
		msetcheck_proof, ..
	} = prove_output;

	// VERIFIER
	verify(&mut oracles.clone(), &claim, gamma, Some(alpha), msetcheck_proof).unwrap();
}
