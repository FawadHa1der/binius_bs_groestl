// Copyright 2023 Ulvetanna Inc.

use super::{
	error::Error,
	zerocheck::{
		ZerocheckClaim, ZerocheckProof, ZerocheckProveOutput, ZerocheckReductor, ZerocheckRound,
		ZerocheckRoundClaim, ZerocheckWitness,
	},
};
use crate::{
	challenger::{CanObserve, CanSample},
	oracle::CompositePolyOracle,
	polynomial::{
		extrapolate_line, transparent::eq_ind::EqIndPartialEval, CompositionPoly,
		Error as PolynomialError, EvaluationDomain, MultilinearExtension, MultilinearQuery,
	},
	protocols::abstract_sumcheck::{
		self, check_evaluation_domain, finalize_evalcheck_claim, validate_rd_challenge,
		AbstractSumcheckEvaluator, AbstractSumcheckProver, AbstractSumcheckReductor, ProverState,
		ReducedClaim,
	},
	witness::MultilinearWitness,
};
use binius_field::{packed::get_packed_slice, ExtensionField, Field, PackedField, TowerField};
use getset::Getters;
use rayon::prelude::*;
use std::sync::Arc;
use tracing::instrument;

#[cfg(feature = "debug_validate_sumcheck")]
use super::zerocheck::validate_witness;

/// Prove a zerocheck to evalcheck reduction.
/// FS is the domain type.
#[instrument(skip_all, name = "zerocheck::prove")]
pub fn prove<'a, F, PW, FS, CW, CH>(
	claim: &ZerocheckClaim<F>,
	witness: ZerocheckWitness<'a, PW, CW>,
	domain: &EvaluationDomain<FS>,
	mut challenger: CH,
	switchover_fn: impl Fn(usize) -> usize,
) -> Result<ZerocheckProveOutput<F>, Error>
where
	F: TowerField + From<PW::Scalar>,
	PW: PackedField,
	PW::Scalar: TowerField + From<F> + ExtensionField<FS>,
	FS: Field,
	CW: CompositionPoly<PW>,
	CH: CanSample<F> + CanObserve<F>,
{
	let n_vars = witness.n_vars();
	let zerocheck_challenges = challenger.sample_vec(n_vars - 1);

	let zerocheck_prover: ZerocheckProver<F, PW, FS, _> =
		ZerocheckProver::new(domain, claim.clone(), witness, &zerocheck_challenges, switchover_fn)?;

	let (reduced_claim, rounds) =
		abstract_sumcheck::prove(claim.poly.n_vars(), zerocheck_prover, challenger)?;

	let evalcheck_claim = finalize_evalcheck_claim(&claim.poly, reduced_claim)?;

	let zerocheck_proof = ZerocheckProof { rounds };
	let output = ZerocheckProveOutput {
		evalcheck_claim,
		zerocheck_proof,
	};
	Ok(output)
}

/// A zerocheck protocol prover.
///
/// To prove a zerocheck claim, supply a multivariate composite witness. In
/// some cases it makes sense to do so in an different yet isomorphic field PW (witness packed
/// field) which may preferable due to superior performance. One example of such operating field
/// would be `BinaryField128bPolyval`, which tends to be much faster than 128-bit tower field on x86
/// CPUs. The only constraint is that constituent MLEs should have MultilinearPoly impls for PW -
/// something which is trivially satisfied for MLEs with tower field scalars for claims in tower
/// field as well.
///
/// Prover state is instantiated via `new` method, followed by exactly $n\\_vars$ `execute_round` invocations.
/// Each of those takes in an optional challenge (None on first round and Some on following rounds) and
/// evaluation domain. Proof and Evalcheck claim are obtained via `finalize` call at the end.
#[derive(Debug, Getters)]
pub struct ZerocheckProver<'a, F, PW, FS, CW>
where
	F: Field + From<PW::Scalar>,
	PW: PackedField,
	PW::Scalar: From<F> + ExtensionField<FS>,
	FS: Field,
	CW: CompositionPoly<PW>,
{
	#[getset(get = "pub")]
	oracle: CompositePolyOracle<F>,
	composition: CW,
	domain: &'a EvaluationDomain<FS>,
	#[getset(get = "pub")]
	round_claim: ZerocheckRoundClaim<F>,

	round: usize,
	last_round_proof: Option<ZerocheckRound<F>>,
	state: ProverState<PW, MultilinearWitness<'a, PW>>,

	zerocheck_challenges: &'a [F],
	round_eq_ind: MultilinearExtension<PW>,

	// Junk (scratch space) at the start of each round
	// After prover computes ith round polynomial, represents evaluations of
	// * $Q_i(r_0, \ldots, r_{i-1}, X, x_{i+1}, \ldots, x_{n-1})$
	// for $X \in \{2, \ldots, d\}$, and $x_{i+1}, \ldots, x_{n-1} \in \{0, 1\}$
	// where
	// * $d$ is degree of polynomial in zerocheck claim
	// * $r_i$ is the verifier challenge at the end of round $i$.
	// with sorting that is lexicographically-inspired (lowest index = least significant).
	round_q: Vec<PW::Scalar>,
	// None initially
	// After ith round, represents the $\bar{Q}_i$ multilinear polynomial partially evaluated
	// at lowest $i$ variables with the $i$ verifier challenges received so far.
	round_q_bar: Option<MultilinearExtension<PW::Scalar>>,
	// inverse of domain[i] * domain[i] - 1 for i in 0, ..., d-2
	smaller_denom_inv: Vec<FS>,
	// Subdomain of domain, but without 0 and 1 (can be used for degree d-2 polynomials)
	smaller_domain: EvaluationDomain<FS>,
}

impl<'a, F, PW, FS, CW> ZerocheckProver<'a, F, PW, FS, CW>
where
	F: Field + From<PW::Scalar>,
	PW: PackedField,
	PW::Scalar: From<F> + ExtensionField<FS>,
	FS: Field,
	CW: CompositionPoly<PW>,
{
	/// Start a new zerocheck instance with claim in field `F`. Witness may be given in
	/// a different (but isomorphic) packed field PW. `switchover_fn` closure specifies
	/// switchover round number per multilinear polynomial as a function of its
	/// [`crate::polynomial::MultilinearPoly::extension_degree`] value.
	pub fn new(
		domain: &'a EvaluationDomain<FS>,
		claim: ZerocheckClaim<F>,
		witness: ZerocheckWitness<'a, PW, CW>,
		zerocheck_challenges: &'a [F],
		switchover_fn: impl Fn(usize) -> usize,
	) -> Result<Self, Error> {
		#[cfg(feature = "debug_validate_sumcheck")]
		validate_witness(&witness)?;

		let n_vars = claim.poly.n_vars();
		let degree = claim.poly.max_individual_degree();

		if degree == 0 {
			return Err(Error::PolynomialDegreeIsZero);
		}
		check_evaluation_domain(degree, domain)?;

		if witness.n_vars() != n_vars {
			return Err(Error::ProverClaimWitnessMismatch);
		}

		let needed_challenges = n_vars - 1;
		if zerocheck_challenges.len() < needed_challenges {
			return Err(Error::NotEnoughZerocheckChallenges);
		}
		let zerocheck_challenges =
			zerocheck_challenges[zerocheck_challenges.len() - needed_challenges..].as_ref();

		let state = ProverState::new(n_vars, witness.multilinears, switchover_fn)?;

		let composition = witness.composition;

		let round_claim = ZerocheckRoundClaim {
			partial_point: Vec::new(),
			current_round_sum: F::ZERO,
		};

		let pw_challenges = zerocheck_challenges
			.iter()
			.map(|&f| f.into())
			.collect::<Vec<PW::Scalar>>();
		let round_eq_ind =
			EqIndPartialEval::new(n_vars - 1, pw_challenges)?.multilinear_extension()?;

		let round_q = vec![PW::Scalar::default(); (1 << (n_vars - 1)) * (degree - 1)];
		let smaller_domain_points = domain.points()[2..].to_vec();
		let smaller_denom_inv = domain.points()[2..]
			.iter()
			.map(|&x| (x * (x - FS::ONE)).invert().unwrap())
			.collect::<Vec<_>>();
		let smaller_domain = EvaluationDomain::from_points(smaller_domain_points)?;

		let zerocheck_prover = ZerocheckProver {
			oracle: claim.poly,
			composition,
			domain,
			round_claim,
			round: 0,
			last_round_proof: None,
			zerocheck_challenges,
			round_eq_ind,
			state,
			round_q,
			round_q_bar: None,
			smaller_denom_inv,
			smaller_domain,
		};

		Ok(zerocheck_prover)
	}

	#[instrument(skip_all, name = "zerocheck::finalize")]
	fn finalize(mut self, prev_rd_challenge: Option<F>) -> Result<ReducedClaim<F>, Error> {
		// First round has no challenge, other rounds should have it
		validate_rd_challenge(prev_rd_challenge, self.round)?;

		if self.round != self.n_vars() {
			return Err(Error::PrematureFinalizeCall);
		}

		// Last reduction to obtain eval value at eval_point
		if let Some(prev_rd_challenge) = prev_rd_challenge {
			self.reduce_claim(prev_rd_challenge)?;
		}

		Ok(self.round_claim.into())
	}

	// Update the round_eq_ind for the next zerocheck round
	//
	// Let
	//  * $n$ be the number of variables in the zerocheck claim
	//  * $eq_k(X, Y)$ denote the equality indicator polynomial on $2 * k$ variables.
	//  * $\alpha_1, \ldots, \alpha_{n-1}$ be the $n-1$ zerocheck challenges
	// In round $i$, before computing the round polynomial, we seek the invariant that
	// * round_eq_ind is MLE of $eq_{n-i-1}(X, Y)$ partially evaluated at $Y = (\alpha_{i+1}, \ldots, \alpha_{n-1})$.
	//
	// To update the round_eq_ind, from $eq_{n-i}(X, \alpha_i, \ldots, \alpha_{n-1})$
	// to $eq_{n-i-1}(X, \alpha_{i+1}, \ldots, \alpha_{n-1})$, we sum consecutive hypercube evaluations.
	//
	// For example consider the hypercube evaluations of $eq_2(X, $\alpha_1, \alpha_2)$
	// * [$(1-\alpha_1) * (1-\alpha_2)$, $\alpha_1 * (1-\alpha_2)$, $(1-\alpha_1) * \alpha_2$, $\alpha_1 * \alpha_2$]
	// and consider the hypercube evaluations of $eq_1(X, \alpha_2)$
	// * [$(1-\alpha_2)$, $\alpha_2$]
	// We obtain the ith hypercube evaluation of $eq_1(X, \alpha_2)$ by summing the $(2*i)$ and $(2*i+1)$
	// hypercube evaluations of $eq_2(X, \alpha_1, \alpha_2)$.
	fn update_round_eq_ind(&mut self) -> Result<(), Error> {
		let current_evals = self.round_eq_ind.evals();
		let new_evals = (0..current_evals.len() >> 1)
			.into_par_iter()
			.map(|i| {
				PW::from_fn(|j| {
					let index = i * PW::WIDTH + j;
					let eval0 = get_packed_slice(current_evals, index << 1);
					let eval1 = get_packed_slice(current_evals, (index << 1) + 1);

					eval0 + eval1
				})
			})
			.collect();
		let new_multilin = MultilinearExtension::from_values(new_evals)?;
		self.round_eq_ind = new_multilin;

		Ok(())
	}

	// Updates auxiliary state that we store corresponding to Section 4 Optimizations in [Gruen24].
	// [Gruen24]: https://eprint.iacr.org/2024/108
	fn update_round_q(&mut self, prev_rd_challenge: PW::Scalar) -> Result<(), Error> {
		let rd_vars = self.n_vars() - self.round;
		let new_q_len = self.round_q.len() / 2;

		// Let d be the degree of the polynomial in the zerocheck claim.
		let specialized_q_values = if self.smaller_domain.size() == 0 {
			// Special handling for the d = 1 (multilinear) case
			vec![PW::Scalar::default(); 1 << rd_vars]
		} else if self.smaller_domain.size() == 1 {
			// We do not need to interpolate in this special d = 2 case
			std::mem::replace(&mut self.round_q, vec![PW::Scalar::default(); new_q_len])
		} else {
			// This is for the d >= 3 case
			//
			// Let r_i be the prev_rd_challenge
			// * Each d-1 sized chunk of round_q is, for some fixed x_{i+1}, ..., x_{n-1}, evaluations of
			// Q_i(r_0, ..., r_{i-1}, X, x_{i+1}, ..., x_{n-1}) at X = 2, ..., d
			// * Q_i is degree d-2 in X, so we can extrapolate each chunk into the evaluation
			// of Q_i(r_0, ..., r_{i-1}, X, x_{i+1}, ..., x_{n-1}) at X = r_i
			self.round_q
				.chunks(self.smaller_domain.size())
				.map(|chunk| self.smaller_domain.extrapolate(chunk, prev_rd_challenge))
				.collect::<Result<Vec<_>, _>>()?
		};

		// We currently have the evaluations of $\bar{Q}_i(r_0, \ldots, r_{i-1}, x_i, x_{i+1}, \ldots, x_{n-1})$
		// at $x_i, x_{i+1}, \ldots, x_{n-1} \in \{0, 1\}$. It is linear in $x_i$.
		// We first partially evaluate $\bar{Q}_i$ at  $x_i = r_i$.
		// Then, we update these evaluations at a fixed $x_{i+1}, \ldots, x_{n-1}$ by
		// adding $r_i * (r_i - 1) * Q_i(r_0, \ldots, r_{i-1}, 2, x_{i+1}, \ldots, x_{n-1})$ to obtain
		// evaluations of $\bar{Q}_{i+1}(r_0, \ldots, r_{i-1}, r_i, x_{i+1}, \ldots, x_{n-1})$.
		let coeff = prev_rd_challenge * (prev_rd_challenge - PW::Scalar::ONE);

		let mut new_q_bar_values = specialized_q_values;
		new_q_bar_values.par_iter_mut().for_each(|e| *e *= coeff);

		if let Some(prev_q_bar) = self.round_q_bar.as_mut() {
			let query = MultilinearQuery::<PW::Scalar>::with_full_query(&[prev_rd_challenge])?;
			let specialized_prev_q_bar = prev_q_bar.evaluate_partial_low(&query)?;
			let specialized_prev_q_bar_evals = specialized_prev_q_bar.evals();

			const CHUNK_SIZE: usize = 64;

			new_q_bar_values
				.par_chunks_mut(CHUNK_SIZE)
				.enumerate()
				.for_each(|(chunk_index, chunks)| {
					for (k, e) in chunks.iter_mut().enumerate() {
						*e += specialized_prev_q_bar_evals[chunk_index * CHUNK_SIZE + k];
					}
				});
		}
		self.round_q_bar = Some(MultilinearExtension::from_values(new_q_bar_values)?);

		// Step 3:
		// Truncate round_q
		self.round_q.truncate(new_q_len);

		Ok(())
	}

	fn compute_round_coeffs(&mut self) -> Result<Vec<PW::Scalar>, Error> {
		let degree = self.oracle.max_individual_degree();
		if degree == 1 {
			return Ok(vec![PW::Scalar::default()]);
		}

		let vertex_state_iterator = self.round_q.par_chunks_exact_mut(degree - 1);

		let round_coeffs = if self.round == 0 {
			let evaluator = ZerocheckFirstRoundEvaluator {
				degree,
				eq_ind: self.round_eq_ind.to_ref(),
				evaluation_domain: self.domain,
				domain_points: self.domain.points(),
				composition: &self.composition,
				denom_inv: &self.smaller_denom_inv,
			};
			self.state.calculate_round_coeffs(
				evaluator,
				self.round_claim.current_round_sum.into(),
				vertex_state_iterator,
			)
		} else {
			let evaluator = ZerocheckLaterRoundEvaluator {
				degree,
				eq_ind: self.round_eq_ind.to_ref(),
				round_zerocheck_challenge: self.zerocheck_challenges[self.round - 1].into(),
				evaluation_domain: self.domain,
				domain_points: self.domain.points(),
				composition: &self.composition,
				denom_inv: &self.smaller_denom_inv,
				round_q_bar: self
					.round_q_bar
					.as_ref()
					.expect("round_q_bar is Some after round 0")
					.to_ref(),
			};
			self.state.calculate_round_coeffs(
				evaluator,
				self.round_claim.current_round_sum.into(),
				vertex_state_iterator,
			)
		}?;

		Ok(round_coeffs)
	}

	#[instrument(skip_all, name = "zerocheck::execute_round")]
	fn execute_round(&mut self, prev_rd_challenge: Option<F>) -> Result<ZerocheckRound<F>, Error> {
		// First round has no challenge, other rounds should have it
		validate_rd_challenge(prev_rd_challenge, self.round)?;

		if self.round >= self.n_vars() {
			return Err(Error::TooManyExecuteRoundCalls);
		}

		// Rounds 1..n_vars-1 - Some(..) challenge is given
		if let Some(prev_rd_challenge) = prev_rd_challenge {
			let isomorphic_prev_rd_challenge = prev_rd_challenge.into();

			// Process switchovers of small field multilinears and folding of large field ones
			self.state.fold(isomorphic_prev_rd_challenge)?;

			// update the round eq indicator multilinear
			self.update_round_eq_ind()?;

			// update round_q and round_q_bar
			self.update_round_q(isomorphic_prev_rd_challenge)?;

			// Reduce Evalcheck claim
			self.reduce_claim(prev_rd_challenge)?;
		}

		// Compute Round Coeffs using the appropriate evaluator
		let round_coeffs = self.compute_round_coeffs()?;

		// Convert round_coeffs to F
		let coeffs = round_coeffs
			.clone()
			.into_iter()
			.map(Into::into)
			.collect::<Vec<F>>();

		let proof_round = ZerocheckRound { coeffs };
		self.last_round_proof = Some(proof_round.clone());

		self.round += 1;

		Ok(proof_round)
	}

	fn reduce_claim(&mut self, prev_rd_challenge: F) -> Result<(), Error> {
		let reductor = ZerocheckReductor {
			alphas: self.zerocheck_challenges,
		};
		let round_claim = self.round_claim.clone();
		let round_proof = self
			.last_round_proof
			.as_ref()
			.expect("round is at least 1 by invariant")
			.clone();

		let new_round_claim = reductor.reduce_round_claim(
			self.round - 1,
			round_claim,
			prev_rd_challenge,
			round_proof,
		)?;

		self.round_claim = new_round_claim;

		Ok(())
	}

	pub fn to_arc_dyn(self) -> ZerocheckProver<'a, F, PW, FS, Arc<dyn CompositionPoly<PW>>>
	where
		CW: 'static,
	{
		//https://github.com/rust-lang/rust/issues/86555
		ZerocheckProver::<'a, F, PW, FS, Arc<dyn CompositionPoly<PW>>> {
			oracle: self.oracle,
			composition: Arc::new(self.composition),
			domain: self.domain,
			round_claim: self.round_claim,
			round: self.round,
			last_round_proof: self.last_round_proof,
			state: self.state,
			zerocheck_challenges: self.zerocheck_challenges,
			round_eq_ind: self.round_eq_ind,
			round_q: self.round_q,
			round_q_bar: self.round_q_bar,
			smaller_denom_inv: self.smaller_denom_inv,
			smaller_domain: self.smaller_domain,
		}
	}
}

impl<'a, F, PW, FS, CW> AbstractSumcheckProver<F> for ZerocheckProver<'a, F, PW, FS, CW>
where
	F: Field + From<PW::Scalar>,
	PW: PackedField,
	PW::Scalar: From<F> + ExtensionField<FS>,
	FS: Field,
	CW: CompositionPoly<PW>,
{
	type Error = Error;

	fn execute_round(
		&mut self,
		prev_rd_challenge: Option<F>,
	) -> Result<ZerocheckRound<F>, Self::Error> {
		ZerocheckProver::execute_round(self, prev_rd_challenge)
	}

	fn finalize(self, prev_rd_challenge: Option<F>) -> Result<ReducedClaim<F>, Self::Error> {
		ZerocheckProver::finalize(self, prev_rd_challenge)
	}

	fn batch_proving_consistent(&self, other: &Self) -> bool {
		let common = other.zerocheck_challenges.len();
		self.zerocheck_challenges[self.zerocheck_challenges.len() - common..]
			== *other.zerocheck_challenges
	}

	fn n_vars(&self) -> usize {
		self.oracle.n_vars()
	}
}

/// Evaluator for the first round of the zerocheck protocol.
///
/// In the first round, we do not need to evaluate at the point F::ONE, because the value is known
/// to be zero. This version of the zerocheck protocol uses the optimizations from section 3 of
/// [Gruen24].
///
/// [Gruen24]: https://eprint.iacr.org/2024/108
#[derive(Debug)]
pub struct ZerocheckFirstRoundEvaluator<'a, P, FS, C>
where
	P: PackedField<Scalar: ExtensionField<FS>>,
	FS: Field,
	C: CompositionPoly<P>,
{
	pub composition: &'a C,
	pub domain_points: &'a [FS],
	pub evaluation_domain: &'a EvaluationDomain<FS>,
	pub degree: usize,
	pub eq_ind: MultilinearExtension<P, &'a [P]>,
	pub denom_inv: &'a [FS],
}

impl<'a, P, FS, C> AbstractSumcheckEvaluator<P> for ZerocheckFirstRoundEvaluator<'a, P, FS, C>
where
	P: PackedField<Scalar: ExtensionField<FS>>,
	FS: Field,
	C: CompositionPoly<P>,
{
	type VertexState = &'a mut [P::Scalar];
	fn n_round_evals(&self) -> usize {
		// In the first round of zerocheck we can uniquely determine the degree d
		// univariate round polynomial $R(X)$ with evaluations at X = 2, ..., d
		// because we know r(0) = r(1) = 0
		self.degree - 1
	}

	fn process_vertex(
		&self,
		i: usize,
		round_q_chunk: Self::VertexState,
		evals_0: &[P::Scalar],
		evals_1: &[P::Scalar],
		evals_z: &mut [P::Scalar],
		round_evals: &mut [P::Scalar],
	) {
		debug_assert!(i < self.eq_ind.size());

		let eq_ind_factor = self
			.eq_ind
			.evaluate_on_hypercube(i)
			.unwrap_or(P::Scalar::ZERO);

		// The rest require interpolation.
		for d in 2..self.domain_points.len() {
			evals_0
				.iter()
				.zip(evals_1.iter())
				.zip(evals_z.iter_mut())
				.for_each(|((&evals_0_j, &evals_1_j), evals_z_j)| {
					*evals_z_j = extrapolate_line::<P::Scalar, FS>(
						evals_0_j,
						evals_1_j,
						self.domain_points[d],
					);
				});

			let composite_value = self
				.composition
				.evaluate_scalar(evals_z)
				.expect("evals_z is initialized with a length of poly.composition.n_vars()");

			round_evals[d - 2] += composite_value * eq_ind_factor;
			round_q_chunk[d - 2] = composite_value * self.denom_inv[d - 2];
		}
	}

	fn round_evals_to_coeffs(
		&self,
		current_round_sum: P::Scalar,
		mut round_evals: Vec<P::Scalar>,
	) -> Result<Vec<P::Scalar>, PolynomialError> {
		debug_assert_eq!(current_round_sum, P::Scalar::ZERO);
		// We are given $r(2), \ldots, r(d)$.
		// From context, we infer that $r(0) = r(1) = 0$.
		round_evals.insert(0, P::Scalar::ZERO);
		round_evals.insert(0, P::Scalar::ZERO);

		let coeffs = self.evaluation_domain.interpolate(&round_evals)?;
		// We can omit the constant term safely
		let coeffs = coeffs[1..].to_vec();
		Ok(coeffs)
	}
}

/// Evaluator for the later rounds of the zerocheck protocol.
///
/// This version of the zerocheck protocol uses the optimizations from section 3 and 4 of [Gruen24].
///
/// [Gruen24]: https://eprint.iacr.org/2024/108
#[derive(Debug)]
pub struct ZerocheckLaterRoundEvaluator<'a, P, FS, C>
where
	P: PackedField<Scalar: ExtensionField<FS>>,
	FS: Field,
	C: CompositionPoly<P>,
{
	pub composition: &'a C,
	pub domain_points: &'a [FS],
	pub evaluation_domain: &'a EvaluationDomain<FS>,
	pub degree: usize,
	pub eq_ind: MultilinearExtension<P, &'a [P]>,
	pub round_zerocheck_challenge: P::Scalar,
	pub denom_inv: &'a [FS],
	pub round_q_bar: MultilinearExtension<P::Scalar, &'a [P::Scalar]>,
}

impl<'a, P, FS, C> AbstractSumcheckEvaluator<P> for ZerocheckLaterRoundEvaluator<'a, P, FS, C>
where
	P: PackedField<Scalar: ExtensionField<FS>>,
	FS: Field,
	C: CompositionPoly<P>,
{
	type VertexState = &'a mut [P::Scalar];
	fn n_round_evals(&self) -> usize {
		// We can uniquely derive the degree d univariate round polynomial r from evaluations at
		// X = 1, ..., d because we have an identity that relates r(0), r(1), and the current
		// round's claimed sum
		self.degree
	}

	fn process_vertex(
		&self,
		i: usize,
		round_q_chunk: Self::VertexState,
		evals_0: &[P::Scalar],
		evals_1: &[P::Scalar],
		evals_z: &mut [P::Scalar],
		round_evals: &mut [P::Scalar],
	) {
		let q_bar_zero = self
			.round_q_bar
			.evaluate_on_hypercube(i << 1)
			.unwrap_or(P::Scalar::ZERO);
		let q_bar_one = self
			.round_q_bar
			.evaluate_on_hypercube((i << 1) + 1)
			.unwrap_or(P::Scalar::ZERO);
		debug_assert!(i < self.eq_ind.size());

		let eq_ind_factor = self
			.eq_ind
			.evaluate_on_hypercube(i)
			.unwrap_or(P::Scalar::ZERO);

		// We can replace constraint polynomial evaluations at C(r, 1, x) with Q_i_bar(r, 1, x)
		// See section 4 of [https://eprint.iacr.org/2024/108] for details
		round_evals[0] += q_bar_one * eq_ind_factor;

		// The rest require interpolation.
		for d in 2..self.domain_points.len() {
			evals_0
				.iter()
				.zip(evals_1.iter())
				.zip(evals_z.iter_mut())
				.for_each(|((&evals_0_j, &evals_1_j), evals_z_j)| {
					*evals_z_j = extrapolate_line::<P::Scalar, FS>(
						evals_0_j,
						evals_1_j,
						self.domain_points[d],
					);
				});

			let composite_value = self
				.composition
				.evaluate_scalar(evals_z)
				.expect("evals_z is initialized with a length of poly.composition.n_vars()");

			round_evals[d - 1] += composite_value * eq_ind_factor;

			// We compute Q_i(r, domain[d], x) values with minimal additional work (linear extrapolation, multiplication, and inversion)
			// and cache these values for later use. These values will help us update Q_i_bar into Q_{i+1}_bar, which will in turn
			// help us avoid next round's constraint polynomial evaluations at X = 1.
			// For more details, see section 4 of [https://eprint.iacr.org/2024/108]
			let specialized_qbar_eval =
				extrapolate_line(q_bar_zero, q_bar_one, self.domain_points[d]);
			round_q_chunk[d - 2] =
				(composite_value - specialized_qbar_eval) * self.denom_inv[d - 2];
		}
	}

	fn round_evals_to_coeffs(
		&self,
		current_round_sum: P::Scalar,
		mut round_evals: Vec<P::Scalar>,
	) -> Result<Vec<P::Scalar>, PolynomialError> {
		// This is a subsequent round of a sumcheck that came from zerocheck, given $r(1), \ldots, r(d)$
		// Letting $s$ be the current round's claimed sum, and $\alpha_i$ the ith zerocheck challenge
		// we have the identity $r(0) = \frac{1}{1 - \alpha_i} * (s - \alpha_i * r(1))$
		// which allows us to compute the value of $r(0)$

		let alpha = self.round_zerocheck_challenge;
		let alpha_bar = P::Scalar::ONE - alpha;
		let one_evaluation = round_evals[0];
		let zero_evaluation_numerator = current_round_sum - one_evaluation * alpha;
		let zero_evaluation_denominator_inv = alpha_bar.invert().unwrap();
		let zero_evaluation = zero_evaluation_numerator * zero_evaluation_denominator_inv;

		round_evals.insert(0, zero_evaluation);

		let coeffs = self.evaluation_domain.interpolate(&round_evals)?;
		// We can omit the constant term safely
		let coeffs = coeffs[1..].to_vec();

		Ok(coeffs)
	}
}
