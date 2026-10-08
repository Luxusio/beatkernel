use crate::{
    rational::{decimal, Ratio},
    BmsChart, BmsError, BmsErrorKind,
};

/// Declared RANK code, without engine-specific timing windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BmsRank {
    /// RANK 0.
    VeryHard,
    /// RANK 1.
    Hard,
    /// RANK 2.
    Normal,
    /// RANK 3.
    Easy,
    /// RANK 4.
    VeryEasy,
}

impl BmsRank {
    /// Parses an ASCII integer code 0..4 with optional plus and leading zeros.
    /// At most 18 digits are accepted; errors identify document-wide line 0.
    pub fn parse(value: &str) -> Result<Self, BmsError> {
        let digits = value.strip_prefix('+').unwrap_or(value);
        if digits.len() > 18 {
            return Err(BmsError::new(0, BmsErrorKind::Limit("decimal precision")));
        }
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(BmsError::new(
                0,
                BmsErrorKind::Syntax("RANK requires a plain ASCII integer"),
            ));
        }
        match decimal(value, 0)?.n {
            0 => Ok(Self::VeryHard),
            1 => Ok(Self::Hard),
            2 => Ok(Self::Normal),
            3 => Ok(Self::Easy),
            4 => Ok(Self::VeryEasy),
            _ => Err(BmsError::new(
                0,
                BmsErrorKind::Syntax("RANK requires a code from 0 through 4"),
            )),
        }
    }

    /// Returns the original numeric RANK code.
    pub const fn code(self) -> u8 {
        match self {
            Self::VeryHard => 0,
            Self::Hard => 1,
            Self::Normal => 2,
            Self::Easy => 3,
            Self::VeryEasy => 4,
        }
    }
}

/// Exact nonnegative DEFEXRANK percentage, with no timing baseline implied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BmsDefExRank(Ratio);

impl BmsDefExRank {
    /// Parses an exact decimal percentage with optional plus and at most 18 digits.
    /// Zero is valid metadata; errors identify document-wide line 0.
    pub fn parse(value: &str) -> Result<Self, BmsError> {
        Self::parse_at(value, 0)
    }

    pub(crate) fn parse_at(value: &str, line: usize) -> Result<Self, BmsError> {
        decimal(value, line).map(Self)
    }

    /// Returns the reduced percentage numerator, before any engine scaling.
    pub const fn numerator(self) -> i128 {
        self.0.n
    }

    /// Returns the positive reduced percentage denominator.
    pub const fn denominator(self) -> i128 {
        self.0.d
    }
}

/// Caller-selected precedence when both validated headers are declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BmsRankPrecedence {
    /// Prefer RANK, falling back to a declared DEFEXRANK.
    RankFirst,
    /// Prefer DEFEXRANK, falling back to a declared RANK.
    DefExRankFirst,
}

/// Selected declaration retaining its header identity, without timing presets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BmsJudgeDifficulty {
    /// A validated RANK declaration.
    Rank(BmsRank),
    /// A validated DEFEXRANK percentage declaration.
    DefExRank(BmsDefExRank),
}

/// Checked, copied rank declarations; absent headers remain absent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BmsRankMetadata {
    rank: Option<BmsRank>,
    defexrank: Option<BmsDefExRank>,
}

impl BmsRankMetadata {
    /// Returns the validated RANK declaration, if present.
    pub const fn rank(self) -> Option<BmsRank> {
        self.rank
    }

    /// Returns the validated DEFEXRANK percentage, if present.
    pub const fn defexrank(self) -> Option<BmsDefExRank> {
        self.defexrank
    }

    /// Selects a declared value using explicit precedence; both absent yields None.
    /// This does not synthesize a default or claim physical-header ordering.
    pub fn resolve(self, policy: BmsRankPrecedence) -> Option<BmsJudgeDifficulty> {
        let rank = self.rank.map(BmsJudgeDifficulty::Rank);
        let defexrank = self.defexrank.map(BmsJudgeDifficulty::DefExRank);
        match policy {
            BmsRankPrecedence::RankFirst => rank.or(defexrank),
            BmsRankPrecedence::DefExRankFirst => defexrank.or(rank),
        }
    }
}

impl BmsChart {
    /// Validates both raw rank headers and returns a copied metadata snapshot.
    /// Malformed values error at line 0 even when a caller would prefer the other
    /// header. Source timing, rules and raw metadata remain unchanged.
    pub fn judge_rank_metadata(&self) -> Result<BmsRankMetadata, BmsError> {
        let rank = self
            .metadata
            .get("RANK")
            .map(|value| BmsRank::parse(value))
            .transpose()?;
        let defexrank = self
            .metadata
            .get("DEFEXRANK")
            .map(|value| BmsDefExRank::parse(value))
            .transpose()?;
        Ok(BmsRankMetadata { rank, defexrank })
    }
}
