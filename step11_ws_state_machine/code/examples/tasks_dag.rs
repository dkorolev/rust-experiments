use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use petgraph::{graphmap::GraphMap, Directed, Incoming, Outgoing};
use tokio::{sync::mpsc, task::JoinHandle, time::sleep};
use axum::extract::ws::{WebSocket, Message};

// Copy the necessary types from main.rs for this example
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct LogicalTimeAbsoluteMs(u64);

impl LogicalTimeAbsoluteMs {
    pub fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    pub fn as_millis(&self) -> u64 {
        self.0
    }
}

impl std::ops::Add<LogicalTimeDeltaMs> for LogicalTimeAbsoluteMs {
    type Output = LogicalTimeAbsoluteMs;

    fn add(self, rhs: LogicalTimeDeltaMs) -> Self::Output {
        LogicalTimeAbsoluteMs(self.0 + rhs.as_millis())
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct LogicalTimeDeltaMs(u64);

impl LogicalTimeDeltaMs {
    pub fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    pub fn as_millis(&self) -> u64 {
        self.0
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct MaroonTaskId(u64);

struct NextTaskIdGenerator {
    next_task_id: u64,
}

impl NextTaskIdGenerator {
    fn new() -> Self {
        Self { next_task_id: 1 }
    }

    fn next_task_id(&mut self) -> MaroonTaskId {
        let task_id = MaroonTaskId(self.next_task_id);
        self.next_task_id += 1;
        task_id
    }
}

trait Timer: Send + Sync + 'static {
    fn millis_since_start(&self) -> LogicalTimeAbsoluteMs;
}

struct WallTimeTimer {
    start_time: std::time::Instant,
}

impl WallTimeTimer {
    fn new() -> Self {
        Self { start_time: std::time::Instant::now() }
    }
}

impl Timer for WallTimeTimer {
    fn millis_since_start(&self) -> LogicalTimeAbsoluteMs {
        LogicalTimeAbsoluteMs::from_millis(self.start_time.elapsed().as_millis() as u64)
    }
}

trait Writer: Send + Sync + 'static {
    async fn write_text(
        &self, text: String, timestamp: Option<LogicalTimeAbsoluteMs>,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        Self: Send;
}

struct WebSocketWriter {
    sender: mpsc::Sender<String>,
    _task: tokio::task::JoinHandle<()>,
}

impl WebSocketWriter {
    fn new(socket: WebSocket) -> Self {
        let (sender, mut receiver) = mpsc::channel::<String>(100);
        let mut socket = socket;

        let task = tokio::spawn(async move {
            while let Some(text) = receiver.recv().await {
                let _ = socket.send(Message::Text(text.into())).await;
            }
        });

        Self { sender, _task: task }
    }
}

impl Writer for WebSocketWriter {
    async fn write_text(
        &self, text: String, _timestamp: Option<LogicalTimeAbsoluteMs>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.sender.send(text).await.map_err(Box::new)?;
        Ok(())
    }
}

// Simplified versions for the example
#[derive(Clone, Debug)]
enum MaroonTaskState {
    Completed,
    DelayedMessageTaskBegin,
    DelayedMessageTaskExecute,
}

#[derive(Debug)]
enum MaroonTaskRuntime {
    DelayedMessage(MaroonTaskRuntimeDelayedMessage),
}

#[derive(Clone, Debug)]
struct MaroonTaskRuntimeDelayedMessage {
    delay: LogicalTimeAbsoluteMs,
    message: String,
}

struct MaroonTask<W: Writer> {
    description: String,
    runtime: MaroonTaskRuntime,
    writer: Arc<W>,
    call_stack: Vec<MaroonTaskState>,
}

struct TimestampedMaroonTask {
    scheduled_timestamp: LogicalTimeAbsoluteMs,
    task_id: MaroonTaskId,
}

impl TimestampedMaroonTask {
    fn new(scheduled_timestamp: LogicalTimeAbsoluteMs, task_id: MaroonTaskId) -> Self {
        Self { scheduled_timestamp, task_id }
    }
}

impl Eq for TimestampedMaroonTask {}

impl PartialEq for TimestampedMaroonTask {
    fn eq(&self, other: &Self) -> bool {
        self.scheduled_timestamp == other.scheduled_timestamp
    }
}

impl std::cmp::Ord for TimestampedMaroonTask {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reversed order by design.
        other.scheduled_timestamp.cmp(&self.scheduled_timestamp)
    }
}

impl PartialOrd for TimestampedMaroonTask {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

struct MaroonRuntime<W: Writer> {
    task_id_generator: NextTaskIdGenerator,
    pending_operations: std::collections::BinaryHeap<TimestampedMaroonTask>,
    active_tasks: std::collections::HashMap<MaroonTaskId, MaroonTask<W>>,
}

impl<W: Writer> Default for MaroonRuntime<W> {
    fn default() -> Self {
        Self {
            task_id_generator: NextTaskIdGenerator::new(),
            pending_operations: Default::default(),
            active_tasks: Default::default(),
        }
    }
}

struct AppState<T: Timer, W: Writer> {
    fsm: Arc<tokio::sync::Mutex<MaroonRuntime<W>>>,
    quit_tx: mpsc::Sender<()>,
    timer: Arc<T>,
}

impl<T: Timer, W: Writer> AppState<T, W> {
    async fn schedule(
        &self, writer: Arc<W>, state: MaroonTaskState, runtime: MaroonTaskRuntime,
        scheduled_timestamp: LogicalTimeAbsoluteMs, task_description: String,
    ) {
        let mut fsm = self.fsm.lock().await;
        let task_id = fsm.task_id_generator.next_task_id();

        fsm
            .active_tasks
            .insert(task_id, MaroonTask { description: task_description, runtime, writer, call_stack: vec![state] });
        fsm.pending_operations.push(TimestampedMaroonTask::new(scheduled_timestamp, task_id));
    }
}

/// Given a DAG in a GraphMap, returns its "layers":
/// all zero‐indegree nodes first, then all nodes whose deps
/// lie entirely in earlier layers, etc.
fn compute_layers<N: Copy + Eq + std::hash::Hash + Ord>(
    graph: &GraphMap<N, (), Directed>,
) -> Vec<Vec<N>> {
    // 1. Compute initial indegrees
    let mut indegree: HashMap<N, usize> = graph
        .nodes()
        .map(|n| (n, graph.neighbors_directed(n, Incoming).count()))
        .collect();
    let mut remaining: HashSet<N> = graph.nodes().collect();
    let mut layers = Vec::new();

    while !remaining.is_empty() {
        // collect all zero‐indegree nodes
        let zero: Vec<N> = remaining
            .iter()
            .copied()
            .filter(|n| indegree.get(n).copied().unwrap_or(0) == 0)
            .collect();
        if zero.is_empty() {
            panic!("Cycle detected in task graph!");
        }
        // record this layer
        layers.push(zero.clone());
        // remove them and decrement successors
        for &n in &zero {
            remaining.remove(&n);
            for succ in graph.neighbors_directed(n, Outgoing) {
                *indegree.get_mut(&succ).unwrap() -= 1;
            }
        }
    }

    layers
}

// Mock writer for the example
struct MockWriter;

impl MockWriter {
    fn new() -> Self {
        Self
    }
}

impl Writer for MockWriter {
    async fn write_text(
        &self, text: String, _timestamp: Option<LogicalTimeAbsoluteMs>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!("Task output: {}", text);
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --- 1. Spin up the AppState ---
    let timer = Arc::new(WallTimeTimer::new());
    let (quit_tx, _quit_rx) = mpsc::channel::<()>(1);
    let app_state = Arc::new(AppState {
        fsm: Default::default(),
        quit_tx,
        timer: Arc::clone(&timer),
    });

    // Use a mock writer for this example
    let writer = Arc::new(MockWriter::new());

    // --- 2. Define a small DAG of four tasks: A → {B, C} → D ---
    //
    //   A
    //  / \
    // B   C
    //  \ /
    //   D
    //
    let mut graph = GraphMap::<&str, (), Directed>::new();
    for &node in &["A", "B", "C", "D"] {
        graph.add_node(node);
    }
    graph.add_edge("A", "B", ());
    graph.add_edge("A", "C", ());
    graph.add_edge("B", "D", ());
    graph.add_edge("C", "D", ());

    // --- 3. Compute "layers" so independent ones can run in parallel ---
    let layers = compute_layers(&graph);
    println!("Computed layers: {:?}", layers);
    // layers == vec![ vec!["A"], vec!["B","C"], vec!["D"] ]

    // --- 4. For each layer, schedule all tasks concurrently,
    //         then wait for them to finish before moving on. ----
    for (layer_idx, layer) in layers.into_iter().enumerate() {
        println!("Starting layer {}: {:?}", layer_idx, layer);

        // 4a. Spawn one scheduling‐job per node in this layer:
        let mut handles: Vec<JoinHandle<()>> = Vec::new();
        for &task_name in &layer {
            let state_clone = Arc::clone(&app_state);
            let writer_clone = Arc::clone(&writer);

            // here you can choose any MaroonTaskRuntime you like;
            // we'll just do a delayed‐message caricature:
            let runtime = MaroonTaskRuntime::DelayedMessage(
                MaroonTaskRuntimeDelayedMessage {
                    delay: LogicalTimeAbsoluteMs::from_millis(100 * (layer_idx as u64 + 1)),
                    message: format!("Hello from task `{}`", task_name),
                }
            );

            let initial_state = MaroonTaskState::DelayedMessageTaskBegin;
            let scheduled_ts = state_clone.timer.millis_since_start();
            let description = format!("Task `{}` (layer {})", task_name, layer_idx);

            handles.push(tokio::spawn(async move {
                state_clone
                    .schedule(
                        writer_clone,
                        initial_state,
                        runtime,
                        scheduled_ts,
                        description,
                    )
                    .await;
            }));
        }

        // 4b. Wait for all those `.schedule()` calls to have been issued.
        //     (They're non‐blocking: tasks actually start only after their delay.)
        futures::future::join_all(handles).await;

        // 4c. Now busy‐wait (or better, use your own notification) until
        //     this layer's tasks have actually completed.
        loop {
            let fsm = app_state.fsm.lock().await;
            // look at remaining active tasks whose description contains "layer X"
            let any_left = fsm.active_tasks
                .values()
                .any(|t| t.description.contains(&format!("layer {}", layer_idx)));
            if !any_left {
                break;
            }
            drop(fsm);
            sleep(Duration::from_millis(50)).await;
        }
        println!("Layer {} complete!", layer_idx);
    }

    println!("All layers done; all tasks finished.");
    Ok(())
}