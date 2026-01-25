use std::collections::HashMap;
use std::future::Future;
use anyhow::Result;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
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
    pub fn spawn<F>(&mut self, future: F, on_error: impl Fn(anyhow::Error) -> Action + Send + 'static) -> OperationId
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

    /// Spawn a simple async operation without error handling
    ///
    /// If the future returns an error, it will be logged but not sent as an action.
    pub fn spawn_simple<F>(&mut self, future: F) -> OperationId
    where
        F: Future<Output = Result<Action>> + Send + 'static,
    {
        self.spawn(future, |e| {
            tracing::error!("Async operation failed: {}", e);
            Action::SetError {
                message: format!("Operation failed: {}", e),
            }
        })
    }

    /// Send an action directly (useful for components)
    ///
    /// This allows components to trigger actions without spawning async tasks.
    pub fn send_action(&self, action: Action) -> Result<()> {
        self.action_tx
            .send(action)
            .map_err(|e| anyhow::anyhow!("Failed to send action: {}", e))
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

    /// Cancel a pending operation
    ///
    /// Aborts the async task associated with the given operation ID.
    /// Note: This is a forceful abort and may leave operations in an inconsistent state.
    pub fn cancel_operation(&mut self, op_id: OperationId) {
        if let Some(handle) = self.pending_ops.remove(&op_id) {
            handle.abort();
        }
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

    /// Get the number of pending operations
    pub fn pending_count(&self) -> usize {
        self.pending_ops.len()
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Duration;

    #[tokio::test]
    async fn test_async_manager_creation() {
        let handle = tokio::runtime::Handle::current();
        let manager = AsyncManager::new(handle);
        assert_eq!(manager.pending_count(), 0);
    }

    #[tokio::test]
    async fn test_send_action() {
        let handle = tokio::runtime::Handle::current();
        let manager = AsyncManager::new(handle);

        let result = manager.send_action(Action::Noop);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_spawn_simple() {
        let handle = tokio::runtime::Handle::current();
        let mut manager = AsyncManager::new(handle);

        let _op_id = manager.spawn_simple(async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok(Action::Noop)
        });

        assert_eq!(manager.pending_count(), 1);

        // Wait a bit for the operation to complete
        tokio::time::sleep(Duration::from_millis(50)).await;

        let actions = manager.poll_completions();
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], Action::Noop));
    }

    #[tokio::test]
    async fn test_cancel_operation() {
        let handle = tokio::runtime::Handle::current();
        let mut manager = AsyncManager::new(handle);

        let op_id = manager.spawn_simple(async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok(Action::Noop)
        });

        assert_eq!(manager.pending_count(), 1);
        manager.cancel_operation(op_id);

        // Wait a bit to ensure cancellation takes effect
        tokio::time::sleep(Duration::from_millis(10)).await;
        manager.poll_completions(); // This will clean up finished tasks

        assert_eq!(manager.pending_count(), 0);
    }
}
