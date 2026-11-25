use anyhow::Result;
use std::collections::HashMap;
use std::future::Future;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use super::action::Action;

/// Unique identifier for async operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperationId(u64);

impl OperationId {
    /// Create a new operation ID
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

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

    /// Pending operation tracking (for potential cancellation)
    pending_ops: HashMap<OperationId, JoinHandle<()>>,

    /// Next operation ID
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
    ///
    /// # Returns
    /// Operation ID that can be used for tracking or cancellation
    pub fn spawn<F>(
        &mut self,
        future: F,
        on_error: impl Fn(anyhow::Error) -> Action + Send + 'static,
    ) -> OperationId
    where
        F: Future<Output = Result<Action>> + Send + 'static,
    {
        let tx = self.action_tx.clone();
        let op_id = self.allocate_op_id();

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
        op_id
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

    /// Cancel all pending operations
    ///
    /// Useful when shutting down or switching contexts where pending operations
    /// are no longer relevant.
    pub fn cancel_all(&mut self) {
        for (_, handle) in self.pending_ops.drain() {
            handle.abort();
        }
    }

    /// Allocate a new operation ID
    fn allocate_op_id(&mut self) -> OperationId {
        let id = self.next_op_id;
        self.next_op_id = self.next_op_id.wrapping_add(1);
        OperationId::new(id)
    }
}

impl Drop for AsyncManager {
    fn drop(&mut self) {
        // Cancel all pending operations on drop
        self.cancel_all();
    }
}
