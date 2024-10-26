// Copyright 2024 Ulvetanna Inc.

use crate::{
	composition::TrivariateProduct,
	protocols::sumcheck::{
		BatchSumcheckOutput, CompositeSumClaim, Error, SumcheckClaim, VerificationError,
	},
};
use binius_field::{util::eq, Field};
use binius_utils::{bail, sorting::is_sorted_ascending};
use getset::CopyGetters;

#[derive(Debug, CopyGetters)]
pub struct GPASumcheckClaim<F: Field> {
	#[getset(get_copy = "pub")]
	n_vars: usize,
	sum: F,
}

impl<F: Field> GPASumcheckClaim<F> {
	pub fn new(n_vars: usize, sum: F) -> Result<Self, Error> {
		Ok(Self { n_vars, sum })
	}
}

pub fn reduce_to_sumchecks<F: Field>(
	claims: &[GPASumcheckClaim<F>],
) -> Result<Vec<SumcheckClaim<F, TrivariateProduct>>, Error> {
	// Check that the claims are in descending order by n_vars
	if !is_sorted_ascending(claims.iter().map(|claim| claim.n_vars()).rev()) {
		bail!(Error::ClaimsOutOfOrder);
	}

	let sumcheck_claims = claims
		.iter()
		.map(|claim| {
			let GPASumcheckClaim { n_vars, sum, .. } = claim;

			let composite_sum = CompositeSumClaim {
				composition: TrivariateProduct {},
				sum: *sum,
			};

			SumcheckClaim::<F, TrivariateProduct>::new(
				*n_vars,
				TrivariateProduct {}.n_vars(),
				vec![composite_sum],
			)
		})
		.collect::<Result<Vec<_>, _>>()?;

	Ok(sumcheck_claims)
}

/// Verify the validity of the sumcheck outputs for a reduced GPA sumcheck.
///
/// This takes in the output of the reduced sumcheck protocol and returns the output for the
/// GPA sumcheck instance. This simply strips off the multilinear evaluation of the eq indicator
/// polynomial and verifies that the value is correct.
pub fn verify_sumcheck_outputs<F: Field>(
	claims: &[GPASumcheckClaim<F>],
	gpa_challenges: &[F],
	sumcheck_output: BatchSumcheckOutput<F>,
) -> Result<BatchSumcheckOutput<F>, Error> {
	let BatchSumcheckOutput {
		challenges: sumcheck_challenges,
		mut multilinear_evals,
	} = sumcheck_output;

	// Check that the claims are in descending order by n_vars
	if !is_sorted_ascending(claims.iter().map(|claim| claim.n_vars()).rev()) {
		bail!(Error::ClaimsOutOfOrder);
	}

	let max_n_vars = claims
		.first()
		.map(|claim| claim.n_vars())
		.unwrap_or_default();

	assert_eq!(gpa_challenges.len(), max_n_vars);
	assert_eq!(sumcheck_challenges.len(), max_n_vars);

	let mut eq_ind_eval = F::ONE;
	let mut last_n_vars = 0;
	for (claim, multilinear_evals) in claims.iter().zip(multilinear_evals.iter_mut()).rev() {
		assert_eq!(multilinear_evals.len(), 3);

		while last_n_vars < claim.n_vars() {
			let sumcheck_challenge = sumcheck_challenges[last_n_vars];
			let gpa_challenge = gpa_challenges[last_n_vars];
			eq_ind_eval *= eq(sumcheck_challenge, gpa_challenge);
			last_n_vars += 1;
		}

		let multilinear_evals_last = multilinear_evals
			.pop()
			.expect("checked above that multilinear_evals length is at least 1");
		if eq_ind_eval != multilinear_evals_last {
			return Err(VerificationError::IncorrectEqIndEvaluation.into());
		}
	}

	Ok(BatchSumcheckOutput {
		challenges: sumcheck_challenges,
		multilinear_evals,
	})
}

#[cfg(test)]
mod tests {
	use crate::{
		challenger::new_hasher_challenger,
		protocols::{
			gkr_gpa::gpa_sumcheck::{
				prove::GPAProver,
				verify::{reduce_to_sumchecks, verify_sumcheck_outputs, GPASumcheckClaim},
			},
			sumcheck,
		},
		transparent::eq_ind::EqIndPartialEval,
	};
	use binius_field::{BinaryField128b, BinaryField32b, BinaryField8b, Field, PackedField};
	use binius_hal::{make_portable_backend, MultilinearExtension};
	use binius_hash::GroestlHasher;
	use binius_math::IsomorphicEvaluationDomainFactory;
	use p3_challenger::CanSample;
	use rand::{rngs::StdRng, Rng, SeedableRng};

	fn generate_poly_helper<P>(
		mut rng: impl Rng,
		n_vars: usize,
		n_multilinears: usize,
	) -> Vec<MultilinearExtension<P>>
	where
		P: PackedField,
	{
		(0..n_multilinears)
			.map(|_| {
				let values = (0..(1 << n_vars))
					.map(|_| PackedField::random(&mut rng))
					.collect();
				MultilinearExtension::from_values(values).unwrap()
			})
			.collect()
	}

	#[test]
	fn test_prove_verify_gpa_sumcheck() {
		type F = BinaryField32b;
		type FDomain = BinaryField8b;
		type FE = BinaryField128b;
		let mut rng = StdRng::seed_from_u64(0);
		let backend = make_portable_backend();
		let domain_factory = IsomorphicEvaluationDomainFactory::<FDomain>::default();
		let n_vars = 4;

		let multilins = generate_poly_helper::<F>(&mut rng, n_vars, 2)
			.into_iter()
			.map(|mle| mle.specialize_arc_dyn::<FE>())
			.collect::<Vec<_>>();
		let mut challenger = new_hasher_challenger::<_, GroestlHasher<_>>();
		let challenges: Vec<FE> = challenger.sample_vec(n_vars);

		let poly_mle_evals = (0..(1 << n_vars))
			.map(|i| {
				multilins[0].evaluate_on_hypercube(i).unwrap()
					* multilins[1].evaluate_on_hypercube(i).unwrap()
			})
			.collect::<Vec<_>>();

		let eq_r = EqIndPartialEval::new(n_vars, challenges.clone())
			.unwrap()
			.multilinear_extension::<FE, _>(&backend)
			.unwrap();

		let mut sum = FE::ZERO;
		(0..(1 << n_vars)).for_each(|i| {
			sum += poly_mle_evals[i] * eq_r.evaluate_on_hypercube(i).unwrap();
		});

		let poly_mle = MultilinearExtension::from_values(poly_mle_evals)
			.unwrap()
			.specialize_arc_dyn();

		let prover = GPAProver::<FDomain, _, _, _>::new(
			multilins.try_into().unwrap(),
			poly_mle,
			sum,
			domain_factory.clone(),
			&challenges,
			&backend,
		)
		.unwrap();

		let (_, proof) = sumcheck::batch_prove(vec![prover], challenger.clone()).unwrap();

		let claim = GPASumcheckClaim::new(n_vars, sum).unwrap();

		let sumcheck_claims = reduce_to_sumchecks(&[claim]).unwrap();

		let batch_output =
			sumcheck::batch_verify(&sumcheck_claims, proof, challenger.clone()).unwrap();

		let claim = GPASumcheckClaim::new(n_vars, sum).unwrap();
		verify_sumcheck_outputs(&[claim], &challenges, batch_output).unwrap();
	}
}
