use tokio::sync::mpsc;
//
#[derive(Debug, Clone)]
pub enum AgentTask {
    Plan,
    ExecuteNode(String),
    RepairGraph,
    MaintainGraph,
    Shutdown,
}

#[derive(Clone)]
pub struct TaskQueue {
    tx: mpsc::UnboundedSender<AgentTask>,
}

impl TaskQueue {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<AgentTask>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Self { tx }, rx)
    }

    pub fn enqueue(&self, task: AgentTask) {
        let _ = self.tx.send(task);
    }
}
