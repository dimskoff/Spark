//! Undo / redo of commands (ADR 0005, section 2).
//!
//! A [`History`] is the undo stack of one document. Record every applied command with [`History::record`] (or let
//! [`History::apply`] apply and record in one call). Undo applies the stored inverse, redo re-applies the command.
//!
//! Dragging a gizmo sends many `SetProperty` commands for the same node and path; they coalesce into one undo step
//! (the first inverse, the last value). A step ends when something else is recorded, on undo / redo, or when the UI
//! calls [`History::seal`] (mouse released). This is the explicit merge key of the ADR: `(node, path)`.

use crate::command::{Applied, Command, CommandError};
use crate::node_id::NodeId;
use crate::world::World;

/// One undo step.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Shown in the UI ("Set transform.position").
    pub label: String,
    /// Re-applied by redo (generated ids made explicit).
    pub forward: Command,
    /// Applied by undo.
    pub inverse: Command,
    /// Consecutive `SetProperty` commands with the same key coalesce.
    pub merge_key: Option<(NodeId, String)>,
}

/// Undo / redo stacks of one document.
#[derive(Clone, Debug)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    limit: usize,
    /// The next record starts a new step even if its merge key matches the last one.
    sealed: bool,
}

impl Default for History {
    fn default() -> Self {
        Self::new(1000)
    }
}

impl History {
    /// A history that keeps at most `limit` undo steps (the oldest are dropped).
    pub fn new(limit: usize) -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), limit: limit.max(1), sealed: true }
    }

    /// Applies `cmd` to the world and records it. Nothing is recorded on error.
    pub fn apply(&mut self, world: &mut World, cmd: Command) -> Result<Applied, CommandError> {
        let applied = world.apply(&cmd)?;
        self.record(cmd, applied.clone());
        Ok(applied)
    }

    /// Records a command that was applied with [`World::apply`]. Clears the redo stack.
    pub fn record(&mut self, cmd: Command, applied: Applied) {
        self.redo.clear();
        let forward = applied.resolved.unwrap_or(cmd);
        let key = match &forward {
            Command::SetProperty { node, path, .. } => Some((node.clone(), path.clone())),
            _ => None,
        };
        if let (false, Some(k), Some(last)) = (self.sealed, &key, self.undo.last_mut()) {
            if last.merge_key.as_ref() == Some(k) {
                last.forward = forward; // keeps the first inverse: undo goes back to before the drag
                return;
            }
        }
        self.sealed = false;
        let label = forward.label();
        self.undo.push(Entry { label, forward, inverse: applied.inverse, merge_key: key });
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }

    /// Ends the current step: the next `SetProperty` starts a new undo step even for the same node and path.
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Label of the step undo would revert.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }

    /// Label of the step redo would re-apply.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    /// Number of undo steps.
    pub fn len(&self) -> usize {
        self.undo.len()
    }

    pub fn is_empty(&self) -> bool {
        self.undo.is_empty()
    }

    /// Forgets everything (after a scene load or a hot reload: old node ids may be gone).
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.sealed = true;
    }

    /// Reverts the last step. Returns the nodes that changed (empty when there was nothing to undo). On error the
    /// step stays on the undo stack.
    pub fn undo(&mut self, world: &mut World) -> Result<Vec<NodeId>, CommandError> {
        self.sealed = true;
        let Some(e) = self.undo.pop() else { return Ok(Vec::new()) };
        match world.apply(&e.inverse) {
            Ok(a) => {
                self.redo.push(e);
                Ok(a.changed)
            }
            Err(err) => {
                self.undo.push(e);
                Err(err)
            }
        }
    }

    /// Re-applies the last undone step. Returns the nodes that changed. On error the step stays on the redo stack.
    pub fn redo(&mut self, world: &mut World) -> Result<Vec<NodeId>, CommandError> {
        self.sealed = true;
        let Some(e) = self.redo.pop() else { return Ok(Vec::new()) };
        match world.apply(&e.forward) {
            Ok(a) => {
                self.undo.push(e);
                Ok(a.changed)
            }
            Err(err) => {
                self.redo.push(e);
                Err(err)
            }
        }
    }
}
