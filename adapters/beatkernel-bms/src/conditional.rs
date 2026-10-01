//! Streaming preparation-only RANDOM/IF scopes, retaining physical diagnostics.
use crate::{BmsError, BmsErrorKind};

enum Scope {
    Random {
        choice: Option<u32>,
        active: bool,
    },
    If {
        choice: Option<u32>,
        parent: bool,
        matched: bool,
        active: bool,
        saw_else: bool,
        line: usize,
    },
}
impl Scope {
    fn active(&self) -> bool {
        match self {
            Self::Random { active, .. } | Self::If { active, .. } => *active,
        }
    }
}
pub(crate) struct Conditional {
    scopes: Vec<Scope>,
    state: u64,
}
fn syntax(line: usize, message: &'static str) -> BmsError {
    BmsError::new(line, BmsErrorKind::Syntax(message))
}
fn positive(value: &str, line: usize) -> Result<u32, BmsError> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(syntax(line, "conditional operand must be positive u32"));
    }
    value
        .parse::<u32>()
        .ok()
        .filter(|n| *n != 0)
        .ok_or_else(|| syntax(line, "conditional operand must be positive u32"))
}
impl Conditional {
    pub(crate) fn new(seed: u64) -> Self {
        Self {
            scopes: Vec::new(),
            state: seed,
        }
    }
    fn active(&self) -> bool {
        self.scopes.last().is_none_or(Scope::active)
    }
    fn room(&self, line: usize) -> Result<(), BmsError> {
        if self.scopes.len() >= 128 {
            Err(BmsError::new(
                line,
                BmsErrorKind::Limit("conditional depth"),
            ))
        } else {
            Ok(())
        }
    }
    fn draw(&mut self, range: u32) -> u32 {
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^= z >> 31;
        ((u128::from(z) * u128::from(range)) >> 64) as u32 + 1
    }
    // true means this is selected payload; controls are always consumed here.
    pub(crate) fn payload(&mut self, command_line: &str, line: usize) -> Result<bool, BmsError> {
        let split = command_line
            .find(char::is_whitespace)
            .unwrap_or(command_line.len());
        let command = &command_line[..split];
        let value = command_line[split..].trim();
        if command.eq_ignore_ascii_case("RANDOM") || command.eq_ignore_ascii_case("SETRANDOM") {
            let n = positive(value, line)?;
            self.room(line)?;
            let active = self.active();
            let choice = if !active {
                None
            } else if command.eq_ignore_ascii_case("RANDOM") {
                Some(self.draw(n))
            } else {
                Some(n)
            };
            self.scopes.push(Scope::Random { choice, active });
        } else if command.eq_ignore_ascii_case("IF") {
            let n = positive(value, line)?;
            self.room(line)?;
            let choice = self
                .scopes
                .iter()
                .rev()
                .find_map(|scope| match scope {
                    Scope::Random { choice, .. } => Some(*choice),
                    _ => None,
                })
                .ok_or_else(|| syntax(line, "IF requires RANDOM or SETRANDOM scope"))?;
            let parent = self.active();
            let matched = choice == Some(n);
            self.scopes.push(Scope::If {
                choice,
                parent,
                matched,
                active: parent && matched,
                saw_else: false,
                line,
            });
        } else if command.eq_ignore_ascii_case("ELSEIF") {
            let n = positive(value, line)?;
            let Some(Scope::If {
                choice,
                parent,
                matched,
                active,
                saw_else,
                ..
            }) = self.scopes.last_mut()
            else {
                return Err(syntax(line, "ELSEIF requires current IF scope"));
            };
            if *saw_else {
                return Err(syntax(line, "ELSEIF after ELSE"));
            }
            let selected = !*matched && *choice == Some(n);
            *matched |= selected;
            *active = *parent && selected;
        } else if command.eq_ignore_ascii_case("ELSE") {
            if !value.is_empty() {
                return Err(syntax(line, "ELSE takes no operand"));
            }
            let Some(Scope::If {
                parent,
                matched,
                active,
                saw_else,
                ..
            }) = self.scopes.last_mut()
            else {
                return Err(syntax(line, "ELSE requires current IF scope"));
            };
            if *saw_else {
                return Err(syntax(line, "duplicate ELSE"));
            }
            *active = *parent && !*matched;
            *matched = true;
            *saw_else = true;
        } else if command.eq_ignore_ascii_case("ENDIF") {
            if !value.is_empty() {
                return Err(syntax(line, "ENDIF takes no operand"));
            }
            if !matches!(self.scopes.last(), Some(Scope::If { .. })) {
                return Err(syntax(line, "ENDIF crosses or lacks IF scope"));
            }
            self.scopes.pop();
        } else if command.eq_ignore_ascii_case("ENDRANDOM") {
            if !value.is_empty() {
                return Err(syntax(line, "ENDRANDOM takes no operand"));
            }
            if !matches!(self.scopes.last(), Some(Scope::Random { .. })) {
                return Err(syntax(line, "ENDRANDOM crosses or lacks RANDOM scope"));
            }
            self.scopes.pop();
        } else {
            return Ok(self.active());
        }
        Ok(false)
    }
    pub(crate) fn finish(&self) -> Result<(), BmsError> {
        if let Some(line) = self.scopes.iter().find_map(|s| match s {
            Scope::If { line, .. } => Some(*line),
            _ => None,
        }) {
            Err(syntax(line, "IF scope missing ENDIF"))
        } else {
            Ok(())
        }
    }
}
