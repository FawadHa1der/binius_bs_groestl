// Copyright 2024 Ulvetanna Inc.

use super::{
	batch_prove::SumcheckProver,
	prover_state::{ProverState, SumcheckEvaluator},
};
use crate::{
	polynomial::{
		CompositionPoly, Error as PolynomialError, MultilinearComposite, MultilinearPoly,
	},
	protocols::sumcheck_v2::{
		common::{CompositeSumClaim, RoundCoeffs},
		error::Error,
	},
};
use binius_field::{ExtensionField, Field, PackedExtension, PackedField};
use binius_hal::ComputationBackend;
use binius_math::{EvaluationDomain, EvaluationDomainFactory};
use binius_utils::bail;
use itertools::izip;
use rayon::prelude::*;
use stackalloc::stackalloc_with_default;
use std::{marker::PhantomData, ops::Range};

pub fn validate_witness<F, P, M, Composition>(
	multilinears: &[M],
	sum_claims: impl IntoIterator<Item = CompositeSumClaim<F, Composition>>,
) -> Result<(), Error>
where
	F: Field,
	P: PackedField<Scalar = F>,
	M: MultilinearPoly<P> + Send + Sync,
	Composition: CompositionPoly<P>,
{
	let n_vars = multilinears
		.first()
		.map(|multilinear| multilinear.n_vars())
		.unwrap_or_default();
	for multilinear in multilinears.iter() {
		if multilinear.n_vars() != n_vars {
			bail!(Error::NumberOfVariablesMismatch);
		}
	}

	let multilinears = multilinears.iter().collect::<Vec<_>>();

	for (i, claim) in sum_claims.into_iter().enumerate() {
		let CompositeSumClaim {
			composition,
			sum: expected_sum,
		} = claim;
		let witness = MultilinearComposite::new(n_vars, composition, multilinears.clone())?;
		let sum = (0..(1 << n_vars))
			.into_par_iter()
			.map(|j| witness.evaluate_on_hypercube(j))
			.try_reduce(|| F::ZERO, |a, b| Ok(a + b))?;

		if sum != expected_sum {
			bail!(Error::SumcheckNaiveValidationFailure {
				composition_index: i,
			});
		}
	}
	Ok(())
}

pub struct RegularSumcheckProver<FDomain, P, Composition, M, Backend>
where
	FDomain: Field,
	P: PackedField,
	M: MultilinearPoly<P> + Send + Sync,
	Backend: ComputationBackend,
{
	n_vars: usize,
	state: ProverState<FDomain, P, M, Backend>,
	compositions: Vec<Composition>,
	domains: Vec<EvaluationDomain<FDomain>>,
}

impl<F, FDomain, P, Composition, M, Backend>
	RegularSumcheckProver<FDomain, P, Composition, M, Backend>
where
	F: Field + ExtensionField<FDomain>,
	FDomain: Field,
	P: PackedField<Scalar = F> + PackedExtension<FDomain>,
	Composition: CompositionPoly<P>,
	M: MultilinearPoly<P> + Send + Sync,
	Backend: ComputationBackend,
{
	pub fn new(
		multilinears: Vec<M>,
		composite_claims: impl IntoIterator<Item = CompositeSumClaim<F, Composition>>,
		evaluation_domain_factory: impl EvaluationDomainFactory<FDomain>,
		switchover_fn: impl Fn(usize) -> usize,
		backend: Backend,
	) -> Result<Self, Error> {
		let composite_claims = composite_claims.into_iter().collect::<Vec<_>>();
		for claim in composite_claims.iter() {
			if claim.composition.n_vars() != multilinears.len() {
				bail!(Error::InvalidComposition {
					expected_n_vars: multilinears.len(),
				});
			}
		}

		let claimed_sums = composite_claims
			.iter()
			.map(|composite_claim| composite_claim.sum)
			.collect();

		let domains = composite_claims
			.iter()
			.map(|composite_claim| {
				let degree = composite_claim.composition.degree();
				evaluation_domain_factory.create(degree + 1)
			})
			.collect::<Result<Vec<_>, _>>()
			.map_err(Error::MathError)?;

		let compositions = composite_claims
			.into_iter()
			.map(|claim| claim.composition)
			.collect();

		let evaluation_points = domains
			.iter()
			.max_by_key(|domain| domain.points().len())
			.map_or_else(|| Vec::new(), |domain| domain.points().to_vec());

		let state = ProverState::new(
			multilinears,
			claimed_sums,
			evaluation_points,
			switchover_fn,
			backend,
		)?;
		let n_vars = state.n_vars();

		Ok(Self {
			n_vars,
			state,
			compositions,
			domains,
		})
	}
}

impl<F, FDomain, P, Composition, M, Backend> SumcheckProver<F>
	for RegularSumcheckProver<FDomain, P, Composition, M, Backend>
where
	F: Field + ExtensionField<FDomain>,
	FDomain: Field,
	P: PackedField<Scalar = F> + PackedExtension<FDomain>,
	Composition: CompositionPoly<P>,
	M: MultilinearPoly<P> + Send + Sync,
	Backend: ComputationBackend,
{
	fn n_vars(&self) -> usize {
		self.n_vars
	}

	fn fold(&mut self, challenge: F) -> Result<(), Error> {
		self.state.fold(challenge)?;
		Ok(())
	}

	fn execute(&mut self, batch_coeff: F) -> Result<RoundCoeffs<F>, Error> {
		let evaluators = izip!(&self.compositions, &self.domains)
			.map(|(composition, evaluation_domain)| RegularSumcheckEvaluator {
				composition,
				evaluation_domain,
				_marker: PhantomData,
			})
			.collect::<Vec<_>>();

		self.state.calculate_round_coeffs(&evaluators, batch_coeff)
	}

	fn finish(self) -> Result<Vec<F>, Error> {
		self.state.finish()
	}
}

struct RegularSumcheckEvaluator<'a, P, FDomain, Composition>
where
	P: PackedField,
	FDomain: Field,
{
	composition: &'a Composition,
	evaluation_domain: &'a EvaluationDomain<FDomain>,
	_marker: PhantomData<P>,
}

impl<'a, F, P, FDomain, Composition> SumcheckEvaluator<P>
	for RegularSumcheckEvaluator<'a, P, FDomain, Composition>
where
	F: Field + ExtensionField<FDomain>,
	P: PackedField<Scalar = F> + PackedExtension<FDomain>,
	FDomain: Field,
	Composition: CompositionPoly<P>,
{
	fn eval_point_indices(&self) -> Range<usize> {
		// NB: We skip evaluation of $r(X)$ at $X = 0$ as it is derivable from the
		// current_round_sum - $r(1)$.
		1..self.composition.degree() + 1
	}

	fn process_subcube_at_eval_point(
		&self,
		_subcube_vars: usize,
		_subcube_index: usize,
		sparse_batch_query: &[&[P]],
	) -> P {
		let row_len = sparse_batch_query.first().map_or(0, |row| row.len());

		stackalloc_with_default(row_len, |evals| {
			self.composition
				.sparse_batch_evaluate(sparse_batch_query, evals)
				.expect("correct by query construction invariant");

			evals.iter().copied().sum()
		})
	}

	fn round_evals_to_coeffs(
		&self,
		last_round_sum: F,
		mut round_evals: Vec<F>,
	) -> Result<Vec<F>, PolynomialError> {
		// Given $r(1), \ldots, r(d+1)$, letting $s$ be the current round's claimed sum,
		// we can compute $r(0)$ using the identity $r(0) = s - r(1)$
		round_evals.insert(0, last_round_sum - round_evals[0]);

		let coeffs = self.evaluation_domain.interpolate(&round_evals)?;
		Ok(coeffs)
	}
}
