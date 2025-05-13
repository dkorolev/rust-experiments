use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use petgraph::{algo::toposort, GraphMap, Directed, Incoming, Outgoing};
use tokio::{sync::mpsc, task::JoinHandle, time::sleep};
use axum::extract::ws::WebSocket;
use your_crate::{
    AppState, LogicalTimeAbsoluteMs, LogicalTimeDeltaMs,
    MaroonTaskRuntime, MaroonTaskRuntimeDelayedMessage, MaroonTaskState,
    WebSocketWriter, WallTimeTimer,
};

/// Given a DAG in a GraphMap, returns its “layers”:
/// all zero‐indegree nodes first, then all nodes whose deps
/// lie entirely in earlier layers, etc.
fn compute_layers<N: Copy + Eq + std::hash::Hash>(
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
            .filter(|n| indegree[n] == 0)
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

#[tokio::main]
async fn main() {
    // --- 1. Spin up the AppState (same as your main) ---
    let timer = Arc::new(WallTimeTimer::new());
    let (quit_tx, _quit_rx) = mpsc::channel::<()>(1);
    let app_state = Arc::new(AppState {
        fsm: Default::default(),
        quit_tx,
        timer: Arc::clone(&timer),
    });

    // We'll use a single WebSocketWriter just to collect output in this example.
    // In real life you'd accept ws upgrades, etc.
    let fake_socket = WebSocket::from_raw_socket(
        tokio_test::io::Builder::new().build(),
        Default::default(),
        None,
    );
    let writer = Arc::new(WebSocketWriter::new(fake_socket));

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

    // --- 3. Compute “layers” so independent ones can run in parallel ---
    let layers = compute_layers(&graph);
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
        //     (They’re non‐blocking: tasks actually start only after their delay.)
        futures::future::join_all(handles).await;

        // 4c. Now busy‐wait (or better, use your own notification) until
        //     this layer’s tasks have actually completed.
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
}
