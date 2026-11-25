use anyhow::Result;
use std::collections::HashMap;
use std::future::Future;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use super::action::Action;

/// Async operation manager
///
/// Manages asynchronous operations (like Vault API calls) and bridges them
/// with the synchronous TUI event loop. Async operations send completion
/// actions through a channel that is polled in the main event loop.
pub struct AsyncManager {
    /// Runtime handle for spawning tasks
    runtime: tokio::runtime::Handle,

    /// Channel for completed actions
    action_tx: UnboundedSender<Action>,
    action_rx: UnboundedReceiver<Action>,

    /// Operations that have not finished, by the number they were given.
    /// They are aborted when the manager is dropped.
    pending_ops: HashMap<u64, JoinHandle<()>>,

    /// Number the next operation gets
    next_op_id: u64,
}

impl AsyncManager {
    /// Create a new async manager
    pub fn new(runtime: tokio::runtime::Handle) -> Self {
        let (action_tx, action_rx) = unbounded_channel();

        Self {
            runtime,
            action_tx,
            action_rx,
            pending_ops: HashMap::new(),
            next_op_id: 0,
        }
    }

    /// Get the action sender (for passing to components)
    pub fn action_sender(&self) -> UnboundedSender<Action> {
        self.action_tx.clone()
    }

    /// Spawn an async operation that will send an action when complete
    ///
    /// # Arguments
    /// * `future` - The async operation to execute
    /// * `on_error` - Function to convert errors into actions
    pub fn spawn<F>(
        &mut self,
        future: F,
        on_error: impl Fn(anyhow::Error) -> Action + Send + 'static,
    ) where
        F: Future<Output = Result<Action>> + Send + 'static,
    {
        let tx = self.action_tx.clone();
        let op_id = self.next_op_id;
        self.next_op_id = self.next_op_id.wrapping_add(1);

        let handle = self.runtime.spawn(async move {
            match future.await {
                Ok(action) => {
                    let _ = tx.send(action);
                }
                Err(e) => {
                    let _ = tx.send(on_error(e));
                }
            }
        });

        self.pending_ops.insert(op_id, handle);
    }

    /// Poll for completed actions
    ///
    /// Should be called in the main event loop to retrieve actions from
    /// completed async operations. Returns all available actions without blocking.
    pub fn poll_completions(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();

        // Drain all available actions from the channel
        while let Ok(action) = self.action_rx.try_recv() {
            actions.push(action);
        }

        // Clean up completed operations
        self.pending_ops.retain(|_, handle| !handle.is_finished());

        actions
    }

    /// Whether any operation is still running
    pub fn has_pending(&self) -> bool {
        self.pending_ops
            .values()
            .any(|handle| !handle.is_finished())
    }

    /// Number of operations spawned so far
    #[cfg(test)]
    pub fn spawned_count(&self) -> u64 {
        self.next_op_id
    }

    /// Cancel all pending operations
    ///
    /// Useful when shutting down or switching contexts where pending operations
    /// are no longer relevant.
    pub fn cancel_all(&mut self) {
        for (_, handle) in self.pending_ops.drain() {
            handle.abort();
        }
    }
}

impl Drop for AsyncManager {
    fn drop(&mut self) {
        // Cancel all pending operations on drop
        self.cancel_all();
    }
}
