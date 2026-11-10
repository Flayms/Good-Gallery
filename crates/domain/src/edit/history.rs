//! Undo and redo over recipes (REQ-021-07).

use super::*;

/// Undo and redo over recipes (REQ-021-07): one entry per finished change.
#[derive(Debug, Clone, PartialEq)]
pub struct History {
    states: Vec<Recipe>,
    at: usize,
}

/// Entries kept; older ones drop out.
pub(super) const HISTORY_LIMIT: usize = 100;

impl History {
    pub fn new(recipe: Recipe) -> Self {
        Self {
            states: vec![recipe],
            at: 0,
        }
    }

    pub fn current(&self) -> &Recipe {
        &self.states[self.at]
    }

    /// Records `recipe` as the newest state, dropping what could have been redone; nothing for an unchanged recipe.
    pub fn push(&mut self, recipe: Recipe) {
        if recipe == *self.current() {
            return;
        }
        self.states.truncate(self.at + 1);
        self.states.push(recipe);
        if self.states.len() > HISTORY_LIMIT {
            self.states.remove(0);
        }
        self.at = self.states.len() - 1;
    }

    pub fn can_undo(&self) -> bool {
        self.at > 0
    }

    pub fn can_redo(&self) -> bool {
        self.at + 1 < self.states.len()
    }

    pub fn undo(&mut self) -> bool {
        let can = self.can_undo();
        if can {
            self.at -= 1;
        }
        can
    }

    pub fn redo(&mut self) -> bool {
        let can = self.can_redo();
        if can {
            self.at += 1;
        }
        can
    }
}
