# Propagating Rust Tracing Spans to Spawned Threads

[https://gemini.google.com/app/ddc3224b2a6d7570](https://gemini.google.com/app/ddc3224b2a6d7570)

*User prompt: rust tracing span is not inherited to spawned thread.*

Response: This happens because the tracing crate tracks the current active span using **thread-local storage**. When you spawn a new thread or an asynchronous task, it starts with a completely clean slate, losing the context of the parent thread.

To fix this, you have to explicitly pass the span into the new thread or task and enter it. Here is how to do it for both synchronous threads and asynchronous tasks.

### 1. Asynchronous Tasks (Tokio, async-std)

If you are using Tokio or another async runtime, you need the `Instrument` trait. You can attach a span to a future before passing it to `tokio::spawn`.

```rust
use tracing::{info, info_span, Instrument};

#[tokio::main]
async fn main() {
    let parent_span = info_span!("parent_async_task");

    // Method A: Instrument with a completely new span
    tokio::spawn(
        async move {
            info!("This is inside the async task");
        }
        .instrument(info_span!("child_async_task"))
    );

    // Method B: Inherit the CURRENT active span
    let _guard = parent_span.enter();
    tokio::spawn(
        async move {
            info!("This inherits 'parent_async_task'");
        }
        .in_current_span() // Automatically grabs the span from thread-local storage
    );
}
```

### 2. Standard Threads (std::thread)

For standard OS threads spawned via `std::thread::spawn`, you must clone the `Span`, move it into the closure, and use `.in_scope()` (or `.enter()`) to activate it inside the new thread.

```rust
use tracing::{info, info_span, Span};
use std::thread;

fn main() {
    let parent_span = info_span!("main_thread_span");
    let _guard = parent_span.enter();

    // 1. Grab a clone of the current span
    let span_to_move = Span::current();

    thread::spawn(move || {
        // 2. Execute the thread's work inside the scope of the moved span
        span_to_move.in_scope(|| {
            info!("This log belongs to 'main_thread_span'");
        });

        // Alternatively, you can use .enter():
        // let _guard = span_to_move.enter();
        // info!("This log belongs to 'main_thread_span'");
    }).join().unwrap();
}
```

**A quick tip on .enter() vs .in_scope():** Use `.in_scope(|| ...)` for closures and synchronous threads because it guarantees the span is exited when the closure finishes. Use `.enter()` if you need a RAII guard (usually required in async functions before `.await` points, though `.instrument()` is preferred there).

---

*User prompt: not interested in network boundaries. i'm writing aws lambda code handling sqs event. event handler function is decorated with `"#[tracing::instrument]"`. sub task is spawned with `JoinSet`. sub task's span should inherit event handler function's span.*

Response: Because your Lambda handler is decorated with `#[tracing::instrument]`, the span for the handler is automatically set as the "current" active span in thread-local storage while that function is executing.

When you spawn tasks into a `JoinSet`, you can capture this active span by importing the `tracing::Instrument` trait and appending `.in_current_span()` to the future right before passing it to `set.spawn()`.

Here is exactly how to do it:

```rust
use aws_lambda_events::event::sqs::SqsEvent;
use lambda_runtime::Error;
use tokio::task::JoinSet;
use tracing::{info, Instrument};

#[tracing::instrument(skip(event))]
pub async fn handle_sqs_event(event: SqsEvent) -> Result<(), Error> {
    info!("Starting SQS batch processing");
    let mut set = JoinSet::new();

    for record in event.records {
        // The async block is the future we are spawning.
        // We attach the current span to it before passing it to the JoinSet.
        set.spawn(
            async move {
                info!("Processing record"); // This log will include the parent's span data
                // ... your sub-task logic here ...
            }  
            .in_current_span() // <--- This captures the macro's span!
        );
    }

    // Wait for all sub-tasks to finish
    while let Some(res) = set.join_next().await {
        res??; // Handle JoinError and your internal task errors
    }

    Ok(())
}
```

### Pro-Tip: Child Spans for SQS Records

While `.in_current_span()` makes the sub-task share the *exact same span* as the parent handler, this can make CloudWatch logs confusing when processing an SQS batch concurrently—logs from different records will be interleaved under one identical span.

Usually, it is better to create a **child span** for each record. Because the handler span is active, any new span you create automatically becomes its child. You can do this using `.instrument()` instead:

```rust
use tracing::{info, info_span, Instrument};

#[tracing::instrument(skip(event))]
pub async fn handle_sqs_event(event: SqsEvent) -> Result<(), Error> {
    let mut set = JoinSet::new();

    for record in event.records {
        let msg_id = record.message_id.clone().unwrap_or_default();

        set.spawn(
            async move {
                info!("Processing record");
            }  
            // Creates a child span tied to this specific message,
            // naturally inheriting from the `handle_sqs_event` span.
            .instrument(info_span!("process_record", message_id = %msg_id))
        );
    }

    while let Some(res) = set.join_next().await {
        res??;
    }

    Ok(())
}
```

In AWS CloudWatch or X-Ray, this second approach gives you a main segment for the Lambda invocation, with neatly organized sub-segments for each parallel SQS record you processed in the JoinSet.

---

