//! `FaceSource` — one face's source in the shared text font catalog.
//!
//! This file owns the `FaceSource` enum that names where a catalog face's
//! bytes come from: static embedded vendor bytes, or consumer-installed bytes
//! retained behind an `Arc`. It exists so the catalog's source list is a
//! closed two-variant shape (text seam S3, `TEXT_SEAM_DESIGN.md` D4).
//!
//! Invariants and rules:
//!
//! - this file owns only the `FaceSource` struct and its `impl` block
//! - consumer sources retain their bytes (`Arc<Vec<u8>>`) so the render world
//!   can register them into its `FontSystem` without a file path

use std::sync::Arc;

use cosmic_text::fontdb::Source as FontDbSource;

use super::FontFaceId;

/// A face source: static embedded bytes, or consumer-installed bytes with the
/// face's allocated id.
#[derive(Clone, Debug)]
pub(crate) enum FaceSource {
    Embedded(&'static [u8]),
    ConsumerInstalled { bytes: Arc<Vec<u8>>, id: FontFaceId },
}

impl FaceSource {
    /// The face's id: embedded faces derive it from their catalog position;
    /// consumer faces carry it explicitly.
    #[must_use]
    pub(crate) fn face_id(&self, index: usize) -> FontFaceId {
        match self {
            Self::Embedded(_) => {
                FontFaceId::embedded_at(u32::try_from(index).expect("embedded face index fits u32"))
            }
            Self::ConsumerInstalled { id, .. } => *id,
        }
    }

    /// The face as a fontdb source, for installing into a `FontSystem`.
    #[must_use]
    pub(crate) fn font_db_source(&self) -> FontDbSource {
        let binary: Arc<dyn AsRef<[u8]> + Send + Sync> = match self {
            Self::Embedded(bytes) => Arc::new(*bytes),
            Self::ConsumerInstalled { bytes, .. } => Arc::<Vec<u8>>::clone(bytes),
        };
        FontDbSource::Binary(binary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_id_derives_embedded_by_position_and_consumer_by_id() {
        let embedded = FaceSource::Embedded(&[]);
        assert_eq!(embedded.face_id(2), FontFaceId::EMBEDDED_SOURCE_HAN_SANS_SC,);

        let consumer = FaceSource::ConsumerInstalled {
            bytes: Arc::new(vec![0, 1, 2]),
            id: FontFaceId::from_raw(9),
        };
        assert_eq!(consumer.face_id(0), FontFaceId::from_raw(9));
    }

    #[test]
    fn font_db_source_wraps_bytes_as_binary() {
        let consumer = FaceSource::ConsumerInstalled {
            bytes: Arc::new(vec![0, 1, 2]),
            id: FontFaceId::from_raw(9),
        };
        match consumer.font_db_source() {
            FontDbSource::Binary(source) => {
                let bytes: &[u8] = source.as_ref().as_ref();
                assert_eq!(bytes, &[0, 1, 2]);
            }
            #[allow(unreachable_patterns)]
            _ => panic!("font_db_source should produce a Binary source"),
        }
    }
}
