/// LIFO stack of cleanup actions. Each setup step pushes its inverse.
/// On failure (or normal teardown), unwind in reverse order.
pub struct CleanupStack {
    actions: Vec<CleanupAction>,
}

struct CleanupAction {
    name: String,
    action: Box<dyn FnOnce() + Send>,
}

impl CleanupStack {
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
        }
    }

    /// Push a cleanup action. Will be executed LIFO on `run_all()`.
    pub fn push(&mut self, name: impl Into<String>, action: impl FnOnce() + Send + 'static) {
        self.actions.push(CleanupAction {
            name: name.into(),
            action: Box::new(action),
        });
    }

    /// Execute all cleanup actions in reverse order.
    pub fn run_all(self) {
        for action in self.actions.into_iter().rev() {
            tracing::debug!("cleanup: {}", action.name);
            (action.action)();
        }
    }

    /// Number of pending actions.
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

impl Default for CleanupStack {
    fn default() -> Self {
        Self::new()
    }
}
