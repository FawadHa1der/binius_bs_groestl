// Copyright 2023 Ulvetanna Inc.

use super::{Error, VerificationError};
use crate::{
	oracle::CompositePolyOracle,
	polynomial::{evaluate_univariate, CompositionPoly, MultilinearComposite, MultilinearPoly},
	protocols::{
		abstract_sumcheck::{
			AbstractSumcheckClaim, AbstractSumcheckProof, AbstractSumcheckReductor,
			AbstractSumcheckRound, AbstractSumcheckRoundClaim,
		},
		evalcheck::EvalcheckClaim,
	},
};
use binius_field::{Field, PackedField};

pub type SumcheckRound<F> = AbstractSumcheckRound<F>;
pub type SumcheckProof<F> = AbstractSumcheckProof<F>;

#[derive(Debug)]
pub struct SumcheckProveOutput<F: Field> {
	pub evalcheck_claim: EvalcheckClaim<F>,
	pub sumcheck_proof: SumcheckProof<F>,
}

#[derive(Debug, Clone)]
pub struct SumcheckClaim<F: Field> {
	pub poly: CompositePolyOracle<F>,
	pub sum: F,
}

impl<F: Field> SumcheckClaim<F> {
	pub fn n_vars(&self) -> usize {
		self.poly.n_vars()
	}
}

impl<F: Field> From<SumcheckClaim<F>> for AbstractSumcheckClaim<F> {
	fn from(value: SumcheckClaim<F>) -> Self {
		Self {
			n_vars: value.poly.n_vars(),
			sum: value.sum,
		}
	}
}

/// Polynomial must be representable as a composition of multilinear polynomials
pub type SumcheckWitness<P, C, M> = MultilinearComposite<P, C, M>;

pub type SumcheckRoundClaim<F> = AbstractSumcheckRoundClaim<F>;

pub struct SumcheckReductor;

impl<F: Field> AbstractSumcheckReductor<F> for SumcheckReductor {
	type Error = Error;

	fn reduce_round_claim(
		&self,
		_round: usize,
		claim: AbstractSumcheckRoundClaim<F>,
		challenge: F,
		round_proof: AbstractSumcheckRound<F>,
	) -> Result<AbstractSumcheckRoundClaim<F>, Self::Error> {
		reduce_intermediate_round_claim_helper(claim, challenge, round_proof)
	}
}

fn reduce_intermediate_round_claim_helper<F: Field>(
	claim: SumcheckRoundClaim<F>,
	challenge: F,
	proof: SumcheckRound<F>,
) -> Result<SumcheckRoundClaim<F>, Error> {
	let SumcheckRoundClaim {
		mut partial_point,
		current_round_sum,
	} = claim;

	let SumcheckRound { mut coeffs } = proof;
	if coeffs.is_empty() {
		return Err(VerificationError::NumberOfCoefficients.into());
	}

	// The prover has sent coefficients for the purported ith round polynomial
	// * $r_i(X) = \sum_{j=0}^d a_j * X^j$
	// However, the prover has not sent the highest degree coefficient $a_d$.
	// The verifier will need to recover this missing coefficient.
	//
	// Let $s$ denote the current round's claimed sum.
	// The verifier expects the round polynomial $r_i$ to satisfy the identity
	// * $s = r_i(0) + r_i(1)$
	// Using
	//     $r_i(0) = a_0$
	//     $r_i(1) = \sum_{j=0}^d a_j$
	// There is a unique $a_d$ that allows $r_i$ to satisfy the above identity.
	// Specifically
	//     $a_d = s - a_0 - \sum_{j=0}^{d-1} a_j$
	//
	// Not sending the whole round polynomial is an optimization.
	// In the unoptimized version of the protocol, the verifier will halt and reject
	// if given a round polynomial that does not satisfy the above identity.
	let last_coeff = current_round_sum - coeffs[0] - coeffs.iter().sum::<F>();
	coeffs.push(last_coeff);
	let new_round_sum = evaluate_univariate(&coeffs, challenge);

	partial_point.push(challenge);

	Ok(SumcheckRoundClaim {
		partial_point,
		current_round_sum: new_round_sum,
	})
}

pub fn validate_witness<F, PW, CW, M>(
	claim: &SumcheckClaim<F>,
	witness: &SumcheckWitness<PW, CW, M>,
) -> Result<(), Error>
where
	F: Field + From<PW::Scalar>,
	PW: PackedField<Scalar: From<F>>,
	CW: CompositionPoly<PW>,
	M: MultilinearPoly<PW> + Sync,
{
	let log_size = witness.n_vars();

	let sum = (0..(1 << log_size))
		.try_fold(PW::Scalar::ZERO, |acc, i| witness.evaluate_on_hypercube(i).map(|res| res + acc));

	if sum? == claim.sum.into() {
		Ok(())
	} else {
		Err(Error::NaiveValidation)
	}
}
