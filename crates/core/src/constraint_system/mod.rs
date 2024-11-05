// Copyright 2024 Irreducible Inc.

pub mod channel;
pub mod error;
mod prove;
pub mod validate;
mod verify;

use binius_field::{BinaryField1b, PackedField, TowerField};
use channel::{ChannelId, Flush};
pub use prove::prove;
pub use verify::verify;

use crate::{
	merkle_tree::{MerkleCap, MerkleTreeVCS},
	oracle::{ConstraintSet, MultilinearOracleSet},
	poly_commit::{batch_pcs, fri_pcs},
	protocols::{greedy_evalcheck::GreedyEvalcheckProof, sumcheck},
};

/// Contains the 3 things that place constraints on witness data in Binius
/// - virtual oracles
/// - polynomial constraints
/// - channel flushes
///
/// As a result, a ConstraintSystem allows us to validate all of these
/// constraints against a witness, as well as enabling generic prove/verify
#[derive(Debug, Clone)]
pub struct ConstraintSystem<P: PackedField<Scalar: TowerField>> {
	pub oracles: MultilinearOracleSet<P::Scalar>,
	pub table_constraints: Vec<ConstraintSet<P>>,
	pub flushes: Vec<Flush>,
	pub max_channel_id: ChannelId,
}

/// Constraint system proof with the standard PCS.
pub type Proof<F, Digest, Hash, Compress> = ProofGenericPCS<
	F,
	MerkleCap<Digest>,
	batch_pcs::Proof<fri_pcs::Proof<BinaryField1b, F, MerkleTreeVCS<F, Digest, Hash, Compress>>>,
>;

/// Constraint system proof with a generic [`crate::poly_commit::PolyCommitScheme`].
#[derive(Debug, Clone)]
pub struct ProofGenericPCS<F: TowerField, PCSComm, PCSProof> {
	pub commitments: Vec<PCSComm>,
	pub zerocheck_proof: sumcheck::Proof<F>,
	pub greedy_evalcheck_proof: GreedyEvalcheckProof<F>,
	pub pcs_proofs: Vec<PCSProof>,
}
