// Copyright 2024 Irreducible Inc.

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
	#[error("codeword buffer must be at least 2**{log_code_len} elements")]
	BufferTooSmall { log_code_len: usize },
	#[error("field order must be at least 2**{log_domain_size}")]
	FieldTooSmall { log_domain_size: usize },
	#[error("domain size is less than 2**{log_required_domain_size}")]
	DomainTooSmall { log_required_domain_size: usize },
	#[error("the packing width must divide the code dimension")]
	PackingWidthMustDivideDimension,
	#[error("the input length must be a power of two")]
	PowerOfTwoLengthRequired,
	#[error("the field extension degree must be a power of two")]
	PowerOfTwoExtensionDegreeRequired,
	#[error("the stride cannot be greater than the packed width")]
	StrideGreaterThanPackedWidth,
	#[error("the batch size is greater than the number of elements")]
	BatchTooLarge,
}
