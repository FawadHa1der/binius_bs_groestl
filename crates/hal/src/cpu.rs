// Copyright 2024 Ulvetanna Inc.

use crate::{
	sumcheck_round_calculator::{calculate_first_round_evals, calculate_later_round_evals},
	utils::tensor_product,
	zerocheck::{ZerocheckCpuBackendHelper, ZerocheckRoundInput, ZerocheckRoundParameters},
	ComputationBackend, Error, MultilinearPoly, MultilinearQueryRef, RoundEvals, SumcheckEvaluator,
	SumcheckMultilinear,
};
use binius_field::{ExtensionField, Field, PackedExtension, PackedField, RepackedExtension};
use binius_math::CompositionPoly;
use std::fmt::Debug;
use tracing::instrument;

/// Implementation of ComputationBackend for the default Backend that uses the CPU for all computations.
#[derive(Clone, Debug)]
pub struct CpuBackend;

pub fn make_portable_backend() -> CpuBackend {
	CpuBackend
}

impl ComputationBackend for CpuBackend {
	type Vec<P: Send + Sync + Debug + 'static> = Vec<P>;

	fn to_hal_slice<P: Debug + Send + Sync + 'static>(v: Vec<P>) -> Self::Vec<P> {
		v
	}

	#[instrument(skip_all)]
	fn tensor_product_full_query<P: PackedField>(
		&self,
		query: &[P::Scalar],
	) -> Result<Self::Vec<P>, Error> {
		tensor_product(query)
	}

	#[instrument(skip_all)]
	fn zerocheck_compute_round_coeffs<F, PW, FDomain>(
		&self,
		params: &ZerocheckRoundParameters,
		input: &ZerocheckRoundInput<F, PW, FDomain>,
		handler: &mut dyn ZerocheckCpuBackendHelper<F, PW, FDomain>,
	) -> Result<Vec<PW::Scalar>, Error>
	where
		F: Field,
		PW: PackedField,
		PW::Scalar: From<F> + Into<F>,
		FDomain: Field,
	{
		// Zerocheck involves too much complicated logic, and instead of moving that logic here,
		// callback back to the zerocheck protocols crate.
		handler.handle_zerocheck_round(params, input)
	}

	fn sumcheck_compute_first_round_evals<FDomain, FBase, F, PBase, P, M, Evaluator, Composition>(
		&self,
		n_vars: usize,
		multilinears: &[SumcheckMultilinear<P, M>],
		evaluators: &[Evaluator],
		evaluation_points: &[FDomain],
	) -> Result<Vec<RoundEvals<P::Scalar>>, Error>
	where
		FDomain: Field,
		FBase: ExtensionField<FDomain>,
		F: Field + ExtensionField<FDomain> + ExtensionField<FBase>,
		PBase: PackedField<Scalar = FBase> + PackedExtension<FDomain>,
		P: PackedField<Scalar = F> + PackedExtension<FDomain> + RepackedExtension<PBase>,
		M: MultilinearPoly<P> + Send + Sync,
		Evaluator: SumcheckEvaluator<PBase, P, Composition> + Sync,
		Composition: CompositionPoly<P>,
	{
		calculate_first_round_evals(n_vars, multilinears, evaluators, evaluation_points)
	}

	fn sumcheck_compute_later_round_evals<FDomain, F, P, M, Evaluator, Composition>(
		&self,
		n_vars: usize,
		tensor_query: Option<MultilinearQueryRef<P>>,
		multilinears: &[SumcheckMultilinear<P, M>],
		evaluators: &[Evaluator],
		evaluation_points: &[FDomain],
	) -> Result<Vec<RoundEvals<P::Scalar>>, Error>
	where
		FDomain: Field,
		F: Field + ExtensionField<FDomain>,
		P: PackedField<Scalar = F> + PackedExtension<FDomain>,
		M: MultilinearPoly<P> + Send + Sync,
		Evaluator: SumcheckEvaluator<P, P, Composition> + Sync,
		Composition: CompositionPoly<P>,
	{
		calculate_later_round_evals(
			n_vars,
			tensor_query,
			multilinears,
			evaluators,
			evaluation_points,
		)
	}
}
