// Copyright 2023 Ulvetanna Inc.

use super::{
	error::Error,
	sumcheck::{
		reduce_sumcheck_claim_final, reduce_sumcheck_claim_round, reduce_zerocheck_claim_round,
		SumcheckClaim, SumcheckRound, SumcheckRoundClaim,
	},
	VerificationError,
};
use crate::{oracle::CompositePolyOracle, protocols::evalcheck::EvalcheckClaim};
use binius_field::Field;

/// Verifies a sumcheck round reduction proof.
///
/// Given a round proof which are the coefficients of a univariate polynomial and the sampled challenge, evaluate the
/// polynomial at the challenge point and reduce to a sumcheck claim over the partially evaluated polynomial.
///
/// Returns the evaluation point and the claimed evaluation.
pub fn verify_round<F: Field>(
	claim: SumcheckRoundClaim<F>,
	challenge: F,
	proof: SumcheckRound<F>,
) -> Result<SumcheckRoundClaim<F>, Error> {
	reduce_sumcheck_claim_round(claim, challenge, proof)
}

/// Verifies a sumcheck round reduction proof.
///
/// Given a round proof which are the coefficients of a univariate polynomial and the sampled challenge, evaluate the
/// polynomial at the challenge point and reduce to a sumcheck claim over the partially evaluated polynomial.
///
/// Returns the evaluation point and the claimed evaluation.
pub fn verify_zerocheck_round<F: Field>(
	claim: SumcheckRoundClaim<F>,
	challenge: F,
	proof: SumcheckRound<F>,
	zerocheck_challenge: Option<F>,
) -> Result<SumcheckRoundClaim<F>, Error> {
	reduce_zerocheck_claim_round(claim, challenge, proof, zerocheck_challenge)
}

/// Verifies a sumcheck reduction proof final step, after all rounds completed.
///
/// Returns the evaluation point and the claimed evaluation.
pub fn verify_final<F: Field>(
	poly_oracle: &CompositePolyOracle<F>,
	claim: SumcheckRoundClaim<F>,
) -> Result<EvalcheckClaim<F>, Error> {
	reduce_sumcheck_claim_final(poly_oracle, claim)
}

/// Verifies a batch sumcheck proof final step, reducing the final claim to evaluation claims.
pub fn batch_verify_final<F: Field>(
	claims: &[SumcheckClaim<F>],
	batch_coeffs: &[F],
	evals: &[F],
	final_claim: SumcheckRoundClaim<F>,
) -> Result<Vec<EvalcheckClaim<F>>, Error> {
	let SumcheckRoundClaim {
		partial_point: eval_point,
		current_round_sum: final_eval,
	} = final_claim;

	// Check that oracles are in descending order by n_vars
	if claims
		.windows(2)
		.any(|pair| pair[0].n_vars() < pair[1].n_vars())
	{
		return Err(Error::OraclesOutOfOrder);
	}

	let n_rounds = claims.first().map(|claim| claim.n_vars()).unwrap_or(0);

	if eval_point.len() != n_rounds {
		return Err(VerificationError::NumberOfRounds.into());
	}
	if claims.len() != batch_coeffs.len() {
		return Err(VerificationError::NumberOfBatchCoeffs.into());
	}
	if evals.len() != claims.len() {
		return Err(VerificationError::NumberOfFinalEvaluations.into());
	}

	let batched_eval = evals
		.iter()
		.zip(batch_coeffs)
		.map(|(eval, coeff)| *eval * *coeff)
		.sum::<F>();

	assert_eq!(batched_eval, final_eval);

	let eval_claims = evals
		.iter()
		.zip(claims)
		.map(|(eval, claim)| EvalcheckClaim {
			poly: claim.poly.clone(),
			eval_point: eval_point[n_rounds - claim.n_vars()..].to_vec(),
			eval: *eval,
			is_random_point: true,
		})
		.collect();

	Ok(eval_claims)
}

pub fn setup_first_round_claim<F: Field>(claim: &SumcheckClaim<F>) -> SumcheckRoundClaim<F> {
	SumcheckRoundClaim {
		partial_point: vec![],
		current_round_sum: claim.sum,
	}
}
