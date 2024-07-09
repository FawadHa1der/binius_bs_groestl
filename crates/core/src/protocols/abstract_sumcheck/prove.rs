// Copyright 2024 Ulvetanna Inc.

use super::{AbstractSumcheckProver, AbstractSumcheckRound, ReducedClaim};
use crate::{
	challenger::{CanObserve, CanSample},
	polynomial::{
		Error as PolynomialError, MultilinearExtensionSpecialized, MultilinearPoly,
		MultilinearQuery,
	},
};
use binius_field::{Field, PackedField};
use binius_utils::array_2d::Array2D;
use rayon::prelude::*;
use std::{borrow::Borrow, cmp, ops::Range};

/// An individual multilinear polynomial in a multivariate composite.
#[derive(Debug)]
enum SumcheckMultilinear<P: PackedField, M: Send> {
	/// Small field polynomial - to be folded into large field at `switchover` round
	Transparent {
		switchover: usize,
		small_field_multilin: M,
	},
	/// Large field polynomial - halved in size each round
	Folded {
		large_field_folded_multilin: MultilinearExtensionSpecialized<P, P>,
	},
}

/// Parallel fold state, consisting of scratch area and result accumulator.
struct ParFoldStates<F: Field> {
	// Evaluations at 0, 1 and domain points, per MLE. Scratch space.
	evals_0: Array2D<F>,
	evals_1: Array2D<F>,
	evals_z: Array2D<F>,

	// Accumulated sums of evaluations over univariate domain.
	round_evals: Array2D<F>,
}

impl<F: Field> ParFoldStates<F> {
	fn new(n_multilinears: usize, n_round_evals: usize, n_states: usize) -> Self {
		Self {
			evals_0: Array2D::new(n_states, n_multilinears),
			evals_1: Array2D::new(n_states, n_multilinears),
			evals_z: Array2D::new(n_states, n_multilinears),
			round_evals: Array2D::new(n_states, n_round_evals),
		}
	}
}

/// Represents an object that can evaluate the composition function of a generalized sumcheck.
///
/// Generalizes handling of regular sumcheck and zerocheck protocols.
pub trait AbstractSumcheckEvaluator<P: PackedField>: Sync {
	type VertexState;

	/// The number of points to evaluate at.
	fn n_round_evals(&self) -> usize;

	/// Process and update the round evaluations with the evaluations at a hypercube vertex.
	///
	/// ## Arguments
	///
	/// * `i`: index of the hypercube vertex under processing
	/// * `vertex_state`: state of the hypercube vertex under processing
	/// * `evals_0`: the n multilinear polynomial evaluations at 0
	/// * `evals_1`: the n multilinear polynomial evaluations at 1
	/// * `evals_z`: a scratch buffer of size n for storing multilinear polynomial evaluations at a
	///              point z
	/// * `round_evals`: the accumulated evaluations for the round
	fn process_vertex(
		&self,
		i: usize,
		vertex_state: Self::VertexState,
		evals_0: &[P::Scalar],
		evals_1: &[P::Scalar],
		evals_z: &mut [P::Scalar],
		round_evals: &mut [P::Scalar],
	);

	/// Given evaluations of the round polynomial, interpolate and return monomial coefficients
	///
	/// ## Arguments
	///
	/// * `current_round_sum`: the claimed sum for the current round
	/// * `round_evals`: the computed evaluations of the round polynomial
	fn round_evals_to_coeffs(
		&self,
		current_round_sum: P::Scalar,
		round_evals: Vec<P::Scalar>,
	) -> Result<Vec<P::Scalar>, PolynomialError>;
}

/// A prover state for a generalized sumcheck protocol.
///
/// The family of generalized sumcheck protocols includes regular sumcheck and zerocheck. The
/// zerocheck case permits many important optimizations, enumerated in [Gruen24]. These algorithms
/// are used to prove the interactive multivariate sumcheck protocol in the specific case that the
/// polynomial is a composite of multilinears. This prover state is responsible for updating and
/// evaluating the composed multilinears.
///
/// Once initialized, the expected caller behavior is to alternate invocations of
/// [`Self::calculate_round_coeffs`] and [`Self::fold`], for a total of `n_rounds` calls to each.
///
/// We associate with each multilinear a `switchover` round number, which controls small field
/// optimization and corresponding time/memory tradeoff. In rounds $0, \ldots, switchover-1$ the
/// partial evaluation of a specific multilinear is obtained by doing $2^{n\\_vars - round}$ inner
/// products, with total time complexity proportional to the number of polynomial coefficients.
///
/// After switchover the inner products are stored in a new MLE in large field, which is halved on
/// each round. There are two tradeoffs at play:
///   1) Pre-switchover rounds perform Small * Large field multiplications, but do $2^{round}$ as many of them.
///   2) Pre-switchover rounds require no additional memory, but initial folding allocates a new MLE in a
///      large field that is $2^{switchover}$ times smaller - for example for 1-bit polynomial and 128-bit large
///      field a switchover of 7 would require additional memory identical to the polynomial size.
///
/// NB. Note that `switchover=0` does not make sense, as first round is never folded.//
///
/// [Gruen24]: https://eprint.iacr.org/2024/108
#[derive(Debug)]
pub struct ProverState<PW, M: Send>
where
	PW: PackedField,
	M: MultilinearPoly<PW> + Sync,
{
	multilinears: Vec<SumcheckMultilinear<PW, M>>,
	query: Option<MultilinearQuery<PW>>,
	round: usize,
}

impl<PW, M> ProverState<PW, M>
where
	PW: PackedField,
	M: MultilinearPoly<PW> + Sync + Send,
{
	pub fn new(
		n_rounds: usize,
		multilinears: impl IntoIterator<Item = M>,
		switchover_fn: impl Fn(usize) -> usize,
	) -> Result<Self, PolynomialError> {
		let mut max_query_vars = 1;
		let multilinears = multilinears
			.into_iter()
			.map(|small_field_multilin| {
				if small_field_multilin.n_vars() != n_rounds {
					return Err(PolynomialError::IncorrectNumberOfVariables {
						expected: n_rounds,
						actual: small_field_multilin.n_vars(),
					});
				}

				let switchover = switchover_fn(small_field_multilin.extension_degree());
				max_query_vars = cmp::max(max_query_vars, switchover);
				Ok(SumcheckMultilinear::Transparent {
					switchover,
					small_field_multilin,
				})
			})
			.collect::<Result<_, _>>()?;

		let query = Some(MultilinearQuery::new(max_query_vars)?);

		Ok(Self {
			multilinears,
			query,
			round: 0,
		})
	}

	/// Fold all stored multilinears with the verifier challenge received in the previous round.
	///
	/// This manages whether to partially evaluate the multilinear at an extension point
	/// (post-switchover) or to store the extended tensor product of the received queries
	/// (pre-switchover).
	///
	/// See struct documentation for more details on the generalized sumcheck proving algorithm.
	pub fn fold(&mut self, prev_rd_challenge: PW::Scalar) -> Result<(), PolynomialError> {
		let &mut Self {
			ref mut multilinears,
			ref mut query,
			ref mut round,
			..
		} = self;

		*round += 1;

		// Update query (has to be done before switchover)
		if let Some(prev_query) = query.take() {
			let expanded_query = prev_query.update(&[prev_rd_challenge])?;
			query.replace(expanded_query);
		}

		// Partial query (for folding)
		let partial_query = MultilinearQuery::with_full_query(&[prev_rd_challenge])?;

		// Perform switchover and/or folding
		let any_transparent_left = multilinears
			.par_iter_mut()
			.map(|multilin| -> Result<bool, PolynomialError> {
				let mut any_transparent_left = false;
				match *multilin {
					SumcheckMultilinear::Transparent {
						switchover,
						ref small_field_multilin,
					} => {
						if switchover <= *round {
							let query_ref = query.as_ref().expect(
								"query is guaranteed to be Some while there are transparent \
								multilinears remaining",
							);
							// At switchover, perform inner products in large field and save them
							// in a newly created MLE.
							let large_field_folded_multilin = small_field_multilin
								.borrow()
								.evaluate_partial_low(query_ref)?;

							*multilin = SumcheckMultilinear::Folded {
								large_field_folded_multilin,
							};
						} else {
							any_transparent_left = true;
						}
					}

					SumcheckMultilinear::Folded {
						ref mut large_field_folded_multilin,
					} => {
						// Post-switchover, simply halve large field MLE.
						*large_field_folded_multilin =
							large_field_folded_multilin.evaluate_partial_low(&partial_query)?;
					}
				}

				Ok(any_transparent_left)
			})
			.reduce(|| Ok(false), |lhs, rhs| Ok(lhs? || rhs?))?;

		// All folded large field - tensor is no more needed.
		if !any_transparent_left {
			*query = None;
		}

		Ok(())
	}

	/// Compute the sum of the partial polynomial evaluations over the hypercube.
	pub fn calculate_round_coeffs<S>(
		&self,
		evaluator: impl AbstractSumcheckEvaluator<PW, VertexState = S>,
		current_round_sum: PW::Scalar,
		vertex_state_iterator: impl IndexedParallelIterator<Item = S>,
	) -> Result<Vec<PW::Scalar>, PolynomialError> {
		// Extract multilinears & round
		let &Self {
			ref multilinears,
			round,
			..
		} = self;

		// Handling different cases separately for more inlining opportunities
		// (especially in early rounds)
		let any_transparent = multilinears
			.iter()
			.any(|ml| matches!(ml, SumcheckMultilinear::Transparent { .. }));
		let any_folded = multilinears
			.iter()
			.any(|ml| matches!(ml, SumcheckMultilinear::Folded { .. }));

		match (any_transparent, any_folded) {
			(true, false) => {
				if round == 0 {
					// All transparent, first round - direct sampling
					self.calculate_round_coeffs_helper(
						Self::only_transparent,
						Self::direct_sample,
						evaluator,
						vertex_state_iterator,
						current_round_sum,
					)
				} else {
					// All transparent, rounds 1..n_vars - small field inner product
					self.calculate_round_coeffs_helper(
						Self::only_transparent,
						|multilin, indices, evals0, evals1, col| {
							self.subcube_inner_product(multilin, indices, evals0, evals1, col)
						},
						evaluator,
						vertex_state_iterator,
						current_round_sum,
					)
				}
			}

			// All folded - direct sampling
			(false, true) => self.calculate_round_coeffs_helper(
				Self::only_folded,
				Self::direct_sample,
				evaluator,
				vertex_state_iterator,
				current_round_sum,
			),

			// Heterogeneous case
			_ => self.calculate_round_coeffs_helper(
				|x| x,
				|sc_multilin, indices, evals0, evals1, col| match sc_multilin {
					SumcheckMultilinear::Transparent {
						small_field_multilin,
						..
					} => self.subcube_inner_product(
						small_field_multilin.borrow(),
						indices,
						evals0,
						evals1,
						col,
					),

					SumcheckMultilinear::Folded {
						large_field_folded_multilin,
					} => Self::direct_sample(
						large_field_folded_multilin,
						indices,
						evals0,
						evals1,
						col,
					),
				},
				evaluator,
				vertex_state_iterator,
				current_round_sum,
			),
		}
	}

	// The gist of sumcheck - summing over evaluations of the multivariate composite on evaluation domain
	// for the remaining variables: there are `round-1` already assigned variables with values from large
	// field, and `rd_vars = n_vars - round` remaining variables that are being summed over. `eval01` closure
	// computes 0 & 1 evaluations at some index - either by performing inner product over assigned variables
	// pre-switchover or directly sampling MLE representation during first round or post-switchover.
	fn calculate_round_coeffs_helper<'b, T, S>(
		&'b self,
		precomp: impl Fn(&'b SumcheckMultilinear<PW, M>) -> T,
		eval01: impl Fn(T, Range<usize>, &mut Array2D<PW::Scalar>, &mut Array2D<PW::Scalar>, usize)
			+ Sync,
		evaluator: impl AbstractSumcheckEvaluator<PW, VertexState = S>,
		vertex_state_iterator: impl IndexedParallelIterator<Item = S>,
		current_round_sum: PW::Scalar,
	) -> Result<Vec<PW::Scalar>, PolynomialError>
	where
		T: Copy + Sync + 'b,
		M: 'b,
	{
		let n_multilinears = self.multilinears.len();
		let n_round_evals = evaluator.n_round_evals();

		// When possible to pre-process unpacking sumcheck multilinears, we do so.
		// For performance, it's ideal to hoist this out of the tight loop.
		let precomps = self.multilinears.iter().map(precomp).collect::<Vec<_>>();

		/// Process batches of vertices in parallel, accumulating the round evaluations.
		const BATCH_SIZE: usize = 64;

		let evals = vertex_state_iterator
			.chunks(BATCH_SIZE)
			.enumerate()
			.fold(
				|| ParFoldStates::new(n_multilinears, n_round_evals, BATCH_SIZE),
				|mut par_fold_states, (vertex, vertex_states)| {
					let begin = vertex * BATCH_SIZE;
					let end = begin + vertex_states.len();
					for (j, precomp) in precomps.iter().enumerate() {
						eval01(
							*precomp,
							begin..end,
							&mut par_fold_states.evals_0,
							&mut par_fold_states.evals_1,
							j,
						);
					}

					for (k, vertex_state) in vertex_states.into_iter().enumerate() {
						evaluator.process_vertex(
							begin + k,
							vertex_state,
							par_fold_states.evals_0.get_row(k),
							par_fold_states.evals_1.get_row(k),
							par_fold_states.evals_z.get_row_mut(k),
							par_fold_states.round_evals.get_row_mut(k),
						);
					}

					par_fold_states
				},
			)
			.map(|states| states.round_evals.sum_rows())
			// Simply sum up the fold partitions.
			.reduce(
				|| vec![PW::Scalar::ZERO; n_round_evals],
				|mut overall_round_evals, partial_round_evals| {
					overall_round_evals
						.iter_mut()
						.zip(partial_round_evals.iter())
						.for_each(|(f, s)| *f += s);
					overall_round_evals
				},
			);

		evaluator.round_evals_to_coeffs(current_round_sum, evals)
	}

	// Note the generic parameter - this method samples small field in first round and
	// large field post-switchover.
	#[inline]
	fn direct_sample<MD>(
		multilin: MD,
		indices: Range<usize>,
		evals_0: &mut Array2D<PW::Scalar>,
		evals_1: &mut Array2D<PW::Scalar>,
		col_index: usize,
	) where
		MD: MultilinearPoly<PW>,
	{
		for (k, i) in indices.enumerate() {
			evals_0[(k, col_index)] = multilin
				.evaluate_on_hypercube(i << 1)
				.expect("eval 0 within range");
			evals_1[(k, col_index)] = multilin
				.evaluate_on_hypercube((i << 1) + 1)
				.expect("eval 1 within range");
		}
	}

	#[inline]
	fn subcube_inner_product(
		&self,
		multilin: &M,
		indices: Range<usize>,
		evals_0: &mut Array2D<PW::Scalar>,
		evals_1: &mut Array2D<PW::Scalar>,
		col_index: usize,
	) {
		let query = self.query.as_ref().expect("tensor present by invariant");

		multilin
			.evaluate_subcube(indices, query, evals_0, evals_1, col_index)
			.expect("indices within range");
	}

	fn only_transparent(sc_multilin: &SumcheckMultilinear<PW, M>) -> &M {
		match sc_multilin {
			SumcheckMultilinear::Transparent {
				small_field_multilin,
				..
			} => small_field_multilin.borrow(),
			_ => panic!("all transparent by invariant"),
		}
	}

	fn only_folded(
		sc_multilin: &SumcheckMultilinear<PW, M>,
	) -> &MultilinearExtensionSpecialized<PW, PW> {
		match sc_multilin {
			SumcheckMultilinear::Folded {
				large_field_folded_multilin,
			} => large_field_folded_multilin,
			_ => panic!("all folded by invariant"),
		}
	}
}

pub fn prove<F, CH, E>(
	n_vars: usize,
	mut sumcheck_prover: impl AbstractSumcheckProver<F, Error = E>,
	mut challenger: CH,
) -> Result<(ReducedClaim<F>, Vec<AbstractSumcheckRound<F>>), E>
where
	F: Field,
	CH: CanSample<F> + CanObserve<F>,
	E: From<PolynomialError> + Sync,
{
	let mut prev_rd_challenge = None;
	let mut rd_proofs = Vec::with_capacity(n_vars);

	for _round_no in 0..n_vars {
		let sumcheck_round = sumcheck_prover.execute_round(prev_rd_challenge)?;
		challenger.observe_slice(&sumcheck_round.coeffs);
		prev_rd_challenge = Some(challenger.sample());
		rd_proofs.push(sumcheck_round);
	}

	let reduced_claim = sumcheck_prover.finalize(prev_rd_challenge)?;

	Ok((reduced_claim, rd_proofs))
}
