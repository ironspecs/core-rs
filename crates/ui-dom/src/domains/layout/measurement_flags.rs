//! Fixed measurement class flags carried on every per-node measurement record.
//!
//! This file owns the `MeasurementFlags` bit-set and its const builders and
//! queries. Its purpose is to keep the measurement class bits (preserve
//! white-space mode, replaced-ratio presence) literal and cheap to compare
//! without owning the record or the layout pass that sets them.
//!
//! Invariants and rules:
//!
//! - bit 0 is `PRESERVE_MODE`, bit 1 is `REPLACED_RATIO`
//! - bits 2-4 are reserved (`main_influences_cross` at bit 2, anonymous-run
//!   boundary start/end at bits 3/4) and stay false in step 1
//! - bits 5-7 are reserved and stay zero

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeasurementFlags {
    /// Raw bit field. Bit 0 is `PRESERVE_MODE`, bit 1 is `REPLACED_RATIO`,
    /// bits 2-4 are reserved, bits 5-7 are reserved zero.
    pub bits: u8,
}

impl MeasurementFlags {
    /// White-space mode class is preserve (`pre`/`pre-wrap`/`break-spaces`)
    /// versus collapse (`normal`/`pre-line`).
    pub const PRESERVE_MODE: u8 = 1u8 << 0;
    /// Replaced element with an intrinsic aspect ratio (img with a ratio).
    pub const REPLACED_RATIO: u8 = 1u8 << 1;

    /// The empty flag set: no class bits and all reserved bits zero.
    #[must_use]
    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    /// Returns whether the preserve white-space mode class bit is set.
    #[must_use]
    pub const fn preserve_mode(self) -> bool {
        self.bits & Self::PRESERVE_MODE != 0
    }

    /// Returns whether the replaced-ratio bit is set.
    #[must_use]
    pub const fn replaced_ratio(self) -> bool {
        self.bits & Self::REPLACED_RATIO != 0
    }

    /// Returns `self` with the preserve white-space mode class bit set.
    #[must_use]
    pub const fn with_preserve_mode(self) -> Self {
        Self {
            bits: self.bits | Self::PRESERVE_MODE,
        }
    }

    /// Returns `self` with the replaced-ratio bit set.
    #[must_use]
    pub const fn with_replaced_ratio(self) -> Self {
        Self {
            bits: self.bits | Self::REPLACED_RATIO,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_const_fns_set_and_query_their_bits() {
        let empty = MeasurementFlags::empty();
        assert_eq!(empty.bits, 0);
        assert!(!empty.preserve_mode());
        assert!(!empty.replaced_ratio());

        let preserve = empty.with_preserve_mode();
        assert_eq!(preserve.bits, MeasurementFlags::PRESERVE_MODE);
        assert!(preserve.preserve_mode());
        assert!(!preserve.replaced_ratio());

        let replaced = empty.with_replaced_ratio();
        assert_eq!(replaced.bits, MeasurementFlags::REPLACED_RATIO);
        assert!(!replaced.preserve_mode());
        assert!(replaced.replaced_ratio());

        let both = empty.with_preserve_mode().with_replaced_ratio();
        assert!(both.preserve_mode());
        assert!(both.replaced_ratio());
        assert_eq!(
            both.bits,
            MeasurementFlags::PRESERVE_MODE | MeasurementFlags::REPLACED_RATIO
        );
    }

    #[test]
    fn flags_reserved_bits_stay_zero() {
        let empty = MeasurementFlags::empty();
        let both = empty.with_preserve_mode().with_replaced_ratio();

        // Bits 2-7 are reserved in step 1 and must stay zero for every builder.
        let named_mask = MeasurementFlags::PRESERVE_MODE | MeasurementFlags::REPLACED_RATIO;
        assert_eq!(empty.bits & !named_mask, 0);
        assert_eq!(both.bits & !named_mask, 0);
    }
}
