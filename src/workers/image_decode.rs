//! Image decoding worker.
//!
//! Decodes metadata/dimensions always; decodes pixel data only if the
//! detected terminal protocol supports inline images.
// TODO(phase-5): Implement image decode with protocol-gated pixel preview.
