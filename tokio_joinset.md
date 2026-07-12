## Aws lambdaイベント通知で複数タスクが発生する場合、各タスクを非同期実行して結果を集約するコード。

Lambdaでイベントを受け取り、複数のタスクを並列処理して結果をまとめるケースですね。APIの呼び出しやデータベース操作など、待ち時間（I/Oバウンド）が発生する処理を高速化するのに非常に有効なアプローチです。

PythonのAWS SDK（`boto3`）は基本的に同期通信を行うため、Lambdaで並行処理を行う場合は標準ライブラリの **`concurrent.futures.ThreadPoolExecutor`** を使うのが最もシンプルで相性が良いです。

以下に、イベントからタスクリストを受け取り、非同期で実行して「成功」「失敗」の結果を集約する実用的なコード例をご紹介します。

### 実装コード (Python)

```python
import json
import concurrent.futures
import time
import logging

logger = logging.getLogger()
logger.setLevel(logging.INFO)

def process_single_task(task_data):
    """
    個別のタスクを実行する関数
    ※ ここにboto3の呼び出しや外部APIへのリクエストなどを記述します
    """
    task_id = task_data.get('id', 'unknown')
    
    # 意図的なエラーテスト（IDが3の場合はエラーを起こす）
    if task_id == 3:
        raise ValueError(f"Task {task_id} encountered an expected error.")
        
    # 疑似的な重い処理（例: API待ち時間）
    time.sleep(1)
    
    return {
        "task_id": task_id,
        "status": "success",
        "processed_data": f"Data for {task_id}"
    }

def lambda_handler(event, context):
    # イベントからタスクのリストを取得（例としてのデフォルト値を用意）
    tasks = event.get('tasks', [
        {'id': 1, 'data': 'A'},
        {'id': 2, 'data': 'B'},
        {'id': 3, 'data': 'C'}, # これがエラーになる想定
        {'id': 4, 'data': 'D'}
    ])

    results = []
    errors = []

    # ThreadPoolExecutorで複数タスクを並行実行 (max_workersで同時実行数を制御)
    with concurrent.futures.ThreadPoolExecutor(max_workers=5) as executor:
        # タスクをサブミットし、Futureオブジェクトと元のタスクデータを紐付ける辞書を作成
        future_to_task = {executor.submit(process_single_task, task): task for task in tasks}

        # as_completedで、処理が完了したものから順に結果を取得する
        for future in concurrent.futures.as_completed(future_to_task):
            task = future_to_task[future]
            try:
                # 処理結果を取得
                result = future.result()
                results.append(result)
                logger.info(f"Task {task.get('id')} succeeded.")
            except Exception as exc:
                # 特定のタスクが失敗しても、全体を止めずにエラーとして記録する
                errors.append({
                    "task_id": task.get('id'),
                    "error_message": str(exc)
                })
                logger.error(f"Task {task.get('id')} failed with error: {exc}")

    # 結果を集約して返す
    response_body = {
        "total_tasks": len(tasks),
        "successful_count": len(results),
        "failed_count": len(errors),
        "successes": results,
        "errors": errors
    }
    
    return {
        'statusCode': 200 if not errors else 207, # 207 Multi-Status (一部成功/一部失敗)
        'body': json.dumps(response_body)
    }

```

### この設計のポイント

* **他のタスクを巻き込まない**
一つのタスクが失敗（例外をスロー）しても、`future.result()` の取得時に `try-except` で個別にキャッチしているため、他の正常なタスクの実行や結果集約を妨げません。
* **動的な同時実行制御 (`max_workers`)**
対象のAPIやデータベースの接続上限（レートリミット）に合わせて `max_workers` を調整できます。
* **ステータスコードの工夫**
すべて成功した場合は `200 OK`、一部だけ失敗した場合は `207 Multi-Status` を返すようにすると、呼び出し元（API Gatewayなど）が状況を判定しやすくなります。

> **補足: `asyncio` について**
> もし `boto3` を一切使わず、`aiohttp` などの非同期専用ライブラリのみで外部APIを叩く場合は `asyncio.gather()` を使う手法もあります。しかし、Lambda内でAWSリソース（S3やDynamoDBなど）を操作するケースが大半かと思いますので、同期ライブラリのまま手軽に並行処理化できる上記のスレッドプール方式が最もトラブルが少なくおすすめです。

## 実装コードはrustです。

Rustでの実装ですね！Rustはメモリ安全でありながら、非同期ランタイム（`tokio`）を使った並行処理が非常に強力です。AWS Lambdaの公式Rustランタイムと組み合わせることで、高速かつコスト効率の良い処理が実現できます。

RustのAWS SDK（`aws-sdk-rust`）はデフォルトで非同期処理に対応しているため、Pythonの時のようにスレッドプールを意識しなくても、**`futures::future::join_all`** を使うだけでシンプルに非同期タスクの集約が可能です。

以下に実装例をご紹介します。

### 1. 依存関係 (`Cargo.toml`)

必要なクレート（ライブラリ）を追加します。非同期ランタイムの `tokio` や、Lambdaランタイム、そしてJSON処理のための `serde` を使用します。

```toml
[package]
name = "lambda-async-tasks"
version = "0.1.0"
edition = "2021"

[dependencies]
lambda_runtime = "0.13" # AWS Lambdaの公式Rustランタイム
tokio = { version = "1", features = ["macros"] } # 非同期ランタイム
serde = { version = "1.0", features = ["derive"] } # シリアライズ/デシリアライズ
serde_json = "1.0"
futures = "0.3" # 非同期タスクの集約 (join_all) に使用
tracing = "0.1" # ロギング用
tracing-subscriber = "0.3"

```

### 2. 実装コード (`src/main.rs`)

```rust
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;

// --- データモデルの定義 ---

#[derive(Deserialize, Debug)]
struct InputEvent {
    #[serde(default)]
    tasks: Vec<Task>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct Task {
    id: u32,
    data: String,
}

#[derive(Serialize, Debug)]
struct TaskSuccess {
    task_id: u32,
    status: String,
    processed_data: String,
}

#[derive(Serialize, Debug)]
struct TaskError {
    task_id: u32,
    error_message: String,
}

#[derive(Serialize, Debug)]
struct OutputResponse {
    total_tasks: usize,
    successful_count: usize,
    failed_count: usize,
    successes: Vec<TaskSuccess>,
    errors: Vec<TaskError>,
}

// --- メインの処理関数 ---

/// 個別のタスクを非同期で実行する関数
async fn process_single_task(task: Task) -> Result<TaskSuccess, TaskError> {
    // 疑似的な非同期の待ち時間 (APIコールやDB操作の代わり)
    sleep(Duration::from_millis(500)).await;

    // 意図的なエラーテスト（IDが3の場合はエラーを起こす）
    if task.id == 3 {
        return Err(TaskError {
            task_id: task.id,
            error_message: format!("Task {} encountered an expected error.", task.id),
        });
    }

    Ok(TaskSuccess {
        task_id: task.id,
        status: "success".to_string(),
        processed_data: format!("Processed payload: {}", task.data),
    })
}

/// Lambdaのハンドラー関数
async fn function_handler(event: LambdaEvent<InputEvent>) -> Result<OutputResponse, Error> {
    let tasks = event.payload.tasks;
    let total_tasks = tasks.len();

    tracing::info!("Received {} tasks to process", total_tasks);

    // 1. タスクごとのFuture（非同期タスク）を生成
    let mut futures = Vec::new();
    for task in tasks {
        futures.push(process_single_task(task));
    }

    // 2. すべてのタスクを並行実行し、完了を待つ
    // join_allを使うことで、タスクが同時に走り、全て終わるまで待機します
    let results = futures::future::join_all(futures).await;

    // 3. 結果を成功と失敗に振り分ける
    let mut successes = Vec::new();
    let mut errors = Vec::new();

    for result in results {
        match result {
            Ok(success) => {
                tracing::info!("Task {} succeeded.", success.task_id);
                successes.push(success);
            }
            Err(error) => {
                tracing::error!("Task {} failed: {}", error.task_id, error.error_message);
                errors.push(error);
            }
        }
    }

    // 4. 集約した結果を返す
    Ok(OutputResponse {
        total_tasks,
        successful_count: successes.len(),
        failed_count: errors.len(),
        successes,
        errors,
    })
}

// --- エントリーポイント ---

#[tokio::main]
async fn main() -> Result<(), Error> {
    // CloudWatch Logs向けにロガーを初期化
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        // AWS Lambdaの環境ではタイムスタンプ等が自動付与されるため、重複を防ぐ設定
        .without_time()
        .init();

    // ハンドラーの登録と実行
    let func = service_fn(function_handler);
    lambda_runtime::run(func).await?;
    
    Ok(())
}

```

### Rust実装におけるポイント

* **`futures::future::join_all` の活用**
非同期関数 (`process_single_task`) を呼び出すと、実行待ち状態の「Future」が返ってきます。これを配列（`Vec`）に詰め込み、`join_all` に渡すことで、指定したすべてのタスクが **一斉に並行実行** されます。AWS SDKのAPIコール（DynamoDBやS3へのリクエスト等）もこの方法で並行処理できます。
* **安全なエラーハンドリング (`Result`)**
Rustでは例外（Exception）の代わりに `Result<Ok, Err>` を使います。タスク内でエラーが発生しても全体がクラッシュすることはなく、集約ループ内の `match` で安全に「成功リスト」と「失敗リスト」に振り分けることができます。
* **スレッドプールの意識が不要**
`tokio` ランタイムが内部で効率的にタスクをスケジューリングしてくれるため、Pythonのような `max_workers` の明示的な管理は基本的には不要です（もしAPIのレートリミット等で同時実行数を意図的に制限したい場合は、`futures::stream::StreamExt` の `buffer_unordered` を使う方法に切り替えることも可能です）。

## TokioのJoinSetで実装してください。

Tokioの `JoinSet` を使った実装ですね！素晴らしい選択です。

先ほどの `futures::future::join_all` は「1つのタスク（スレッド）の中で複数の処理を並行して切り替える」**ものでしたが、`JoinSet` を使うと**「Tokioランタイム上で新しいタスクとしてそれぞれをSpawn（生成）し、複数のスレッドで真の並列処理を行う」ことが可能になります。

AWS SDKを使った通信だけでなく、計算処理などが混ざる場合でもパフォーマンスを最大限に引き出せます。また、`futures` クレートに依存せず `tokio` だけで完結するのもメリットです。

### 1. 依存関係 (`Cargo.toml`)

`futures` クレートが不要になるため削除し、`tokio` の機能をフルで使えるようにします。

```toml
[package]
name = "lambda-joinset-tasks"
version = "0.1.0"
edition = "2021"

[dependencies]
lambda_runtime = "0.13"
tokio = { version = "1", features = ["full"] } # JoinSetを使うためフル機能(または "rt" フィーチャー)を有効化
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tracing = "0.1"
tracing-subscriber = "0.3"

```

### 2. 実装コード (`src/main.rs`)

```rust
use lambda_runtime::{service_fn, Error, LambdaEvent};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::sleep;

// --- データモデルの定義 ---

#[derive(Deserialize, Debug)]
struct InputEvent {
    #[serde(default)]
    tasks: Vec<Task>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct Task {
    id: u32,
    data: String,
}

#[derive(Serialize, Debug)]
struct TaskSuccess {
    task_id: u32,
    status: String,
    processed_data: String,
}

#[derive(Serialize, Debug)]
struct TaskError {
    task_id: u32,
    error_message: String,
}

#[derive(Serialize, Debug)]
struct OutputResponse {
    total_tasks: usize,
    successful_count: usize,
    failed_count: usize,
    successes: Vec<TaskSuccess>,
    errors: Vec<TaskError>,
}

// --- メインの処理関数 ---

/// 個別のタスクを非同期で実行する関数
async fn process_single_task(task: Task) -> Result<TaskSuccess, TaskError> {
    sleep(Duration::from_millis(500)).await;

    // 意図的なエラーテスト
    if task.id == 3 {
        return Err(TaskError {
            task_id: task.id,
            error_message: format!("Task {} encountered an expected error.", task.id),
        });
    }

    Ok(TaskSuccess {
        task_id: task.id,
        status: "success".to_string(),
        processed_data: format!("Processed payload: {}", task.data),
    })
}

/// Lambdaのハンドラー関数
async fn function_handler(event: LambdaEvent<InputEvent>) -> Result<OutputResponse, Error> {
    let tasks = event.payload.tasks;
    let total_tasks = tasks.len();

    tracing::info!("Received {} tasks to process", total_tasks);

    // 1. JoinSetの初期化
    let mut set = JoinSet::new();

    // 2. タスクをそれぞれSpawn（生成）してJoinSetに追加
    for task in tasks {
        // spawn() に渡す非同期ブロック/関数は 'static ライフタイムを要求するため、
        // task の所有権ごと関数に渡します
        set.spawn(process_single_task(task));
    }

    let mut successes = Vec::new();
    let mut errors = Vec::new();

    // 3. 完了したタスクから順に結果を取り出す (join_next)
    while let Some(join_result) = set.join_next().await {
        match join_result {
            // JoinErrorがなく、タスク自体が正常終了（Ok）し、その中身もOkだった場合
            Ok(Ok(success)) => {
                tracing::info!("Task {} succeeded.", success.task_id);
                successes.push(success);
            }
            // JoinErrorはないが、タスク内部の処理がエラー（Err）を返した場合
            Ok(Err(error)) => {
                tracing::error!("Task {} failed: {}", error.task_id, error.error_message);
                errors.push(error);
            }
            // JoinErrorが発生した場合（タスクがパニックを起こした、またはキャンセルされた等）
            Err(join_err) => {
                tracing::error!("A task panicked or was cancelled: {}", join_err);
                errors.push(TaskError {
                    task_id: 0, // パニック時はIDが特定できないため0などのデフォルト値やOptionを検討します
                    error_message: format!("Task execution error (panic/cancel): {}", join_err),
                });
            }
        }
    }

    // 4. 集約した結果を返す
    Ok(OutputResponse {
        total_tasks,
        successful_count: successes.len(),
        failed_count: errors.len(),
        successes,
        errors,
    })
}

// --- エントリーポイント ---

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .without_time()
        .init();

    let func = service_fn(function_handler);
    lambda_runtime::run(func).await?;
    
    Ok(())
}

```

### JoinSet 実装での重要な変更点

* **`set.spawn()` による別タスクへの切り出し**
タスクを `JoinSet` に追加すると、即座にバックグラウンドで実行が開始されます。これらはTokioのワーカースレッド上で独立して動くため、CPUを効率的に利用できます。
* **完了順に処理 (`join_next`)**
`join_all` は配列の順序を維持して結果を返しますが、`JoinSet::join_next()` は「終わったものから順番に」結果を返します。これにより、すべてのタスクを待たずに、完了した順からメモリ解放や後処理を行うことができます。
* **パニック（Panic）の安全な補足 (`JoinError`)**
もし `process_single_task` の内部で `unwrap()` に失敗してパニック（クラッシュ）が起きたとしても、プログラム全体は落ちません。`join_next` が `Err(join_err)` を返してパニックをキャッチしてくれるため、Lambda全体が異常終了するのを防ぐことができます。

