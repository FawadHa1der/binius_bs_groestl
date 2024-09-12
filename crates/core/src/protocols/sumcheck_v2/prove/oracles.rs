// Copyright 2024 Ulvetanna Inc.

use super::{RegularSumcheckProver, ZerocheckProver};
use crate::{
	oracle::{Constraint, ConstraintPredicate, ConstraintSet, TypeErasedComposition},
	polynomial::MultilinearPoly,
	protocols::sumcheck_v2::{CompositeSumClaim, Error},
	witness::{MultilinearExtensionIndex, MultilinearWitness},
};
use binius_field::{
	as_packed_field::{PackScalar, PackedType},
	underlier::UnderlierType,
	ExtensionField, Field, PackedFieldIndexable,
};
use binius_hal::ComputationBackend;
use binius_math::EvaluationDomainFactory;
use binius_utils::bail;
use itertools::Itertools;

pub type OracleZerocheckProver<'a, FDomain, P, Backend> =
	ZerocheckProver<FDomain, P, TypeErasedComposition<P>, MultilinearWitness<'a, P>, Backend>;

pub type OracleSumcheckProver<'a, FDomain, P, Backend> =
	RegularSumcheckProver<FDomain, P, TypeErasedComposition<P>, MultilinearWitness<'a, P>, Backend>;

/// Construct zerocheck prover from the constraint set. Fails when constraint set contains regular sumchecks.
pub fn constraint_set_zerocheck_prover<'a, U, FW, FDomain, Backend>(
	constraint_set: ConstraintSet<PackedType<U, FW>>,
	witness: &MultilinearExtensionIndex<'a, U, FW>,
	evaluation_domain_factory: impl EvaluationDomainFactory<FDomain>,
	switchover_fn: impl Fn(usize) -> usize + Clone,
	zerocheck_challenges: &[FW],
	backend: Backend,
) -> Result<OracleZerocheckProver<'a, FDomain, PackedType<U, FW>, Backend>, Error>
where
	U: UnderlierType + PackScalar<FW> + PackScalar<FDomain>,
	FW: ExtensionField<FDomain>,
	FDomain: Field,
	PackedType<U, FW>: PackedFieldIndexable,
	Backend: ComputationBackend,
{
	let (constraints, multilinears) = split_constraint_set(constraint_set, witness)?;

	let mut zeros = Vec::new();

	for Constraint {
		composition,
		predicate,
	} in constraints
	{
		match predicate {
			ConstraintPredicate::Zero => zeros.push(composition),
			_ => bail!(Error::MixedBatchingNotSupported),
		}
	}

	let prover = ZerocheckProver::new(
		multilinears,
		zeros,
		zerocheck_challenges,
		evaluation_domain_factory,
		switchover_fn,
		backend,
	)?;

	Ok(prover)
}

/// Construct regular sumcheck prover from the constraint set. Fails when constraint set contains zerochecks.
pub fn constraint_set_sumcheck_prover<'a, U, FW, FDomain, Backend>(
	constraint_set: ConstraintSet<PackedType<U, FW>>,
	witness: &MultilinearExtensionIndex<'a, U, FW>,
	evaluation_domain_factory: impl EvaluationDomainFactory<FDomain>,
	switchover_fn: impl Fn(usize) -> usize + Clone,
	backend: Backend,
) -> Result<OracleSumcheckProver<'a, FDomain, PackedType<U, FW>, Backend>, Error>
where
	U: UnderlierType + PackScalar<FW> + PackScalar<FDomain>,
	FW: ExtensionField<FDomain>,
	FDomain: Field,
	PackedType<U, FW>: PackedFieldIndexable,
	Backend: ComputationBackend,
{
	let (constraints, multilinears) = split_constraint_set(constraint_set, witness)?;

	let mut sums = Vec::new();

	for Constraint {
		composition,
		predicate,
	} in constraints
	{
		match predicate {
			ConstraintPredicate::Sum(sum) => sums.push(CompositeSumClaim { composition, sum }),
			_ => bail!(Error::MixedBatchingNotSupported),
		}
	}

	let prover = RegularSumcheckProver::new(
		multilinears,
		sums,
		evaluation_domain_factory,
		switchover_fn,
		backend,
	)?;

	Ok(prover)
}

type ConstraintsAndMultilinears<'a, P> = (Vec<Constraint<P>>, Vec<MultilinearWitness<'a, P>>);

fn split_constraint_set<'a, U, FW>(
	constraint_set: ConstraintSet<PackedType<U, FW>>,
	witness: &MultilinearExtensionIndex<'a, U, FW>,
) -> Result<ConstraintsAndMultilinears<'a, PackedType<U, FW>>, Error>
where
	U: UnderlierType + PackScalar<FW>,
	FW: Field,
{
	let ConstraintSet {
		oracle_ids,
		constraints,
	} = constraint_set;

	let multilinears = oracle_ids
		.iter()
		.map(|&oracle_id| witness.get_multilin_poly(oracle_id))
		.collect::<Result<Vec<_>, _>>()?;

	if multilinears
		.iter()
		.tuple_windows()
		.any(|(a, b)| a.n_vars() != b.n_vars())
	{
		bail!(Error::ConstraintSetNumberOfVariablesMismatch);
	}

	Ok((constraints, multilinears))
}
