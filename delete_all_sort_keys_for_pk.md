# RustからAWS DynamoDB の Partition Key を指定して、全ての Sort Key を削除するコード。

DynamoDBには、パーティションキー（Partition Key）を指定してそれに紐づく全てのソートキー（Sort Key）レコードを1回の操作で一括削除するAPIは用意されていません。

そのため、RustのAWS SDK (aws-sdk-dynamodb) を用いてこれを実現するには、「`Query`で対象のレコードを全て取得し、そのキー情報を使って`BatchWriteItem`で一括削除する」という2段階のプロセスを実装する必要があります。

# 実装のポイントと注意点

ページネーションの実装: パーティションキーを指定して`Query`を実行して対象を取得します 。DynamoDBの1回の`Query`操作は最大1MBのデータまでしか返しません 。データ量がこの制限を超える場合、DynamoDBは `last_evaluated_key` を返します 。すべてのレコードを取得するには、この値が `None` になるまで `.exclusive_start_key()` を使ってループ処理を行う必要があります 。

`BatchWriteItem` の制限: `batch_write_item` APIは1回の呼び出しで最大25件のリクエストしか処理できません。そのため、取得したレコードを25件ずつのチャンクに分割して削除リクエストを送信します。

プロジェクション式の活用: 削除に必要なのはパーティションキーとソートキーの属性だけです。`projection_expression` を使って通信量とメモリ消費を抑えます。

# Rust実装コード

以下に、対象のパーティションキーに紐づく全てのレコードを安全に取得・削除する完全な例を示します。

```rust
use aws_sdk_dynamodb::{
    types::{AttributeValue, DeleteRequest, WriteRequest},
    Client, Error,
};
use std::collections::HashMap;

/// 指定したパーティションキーに紐づくすべてのレコードを削除します
pub async fn delete_all_sort_keys_for_pk(
    client: &Client,
    table_name: &str,
    pk_name: &str, // パーティションキーの属性名 (例: "pk")
    sk_name: &str, // ソートキーの属性名 (例: "sk")
    pk_value: &str, // 削除対象のパーティションキーの値
) -> Result<(), Error> {
    let mut exclusive_start_key = None;
    let mut items_to_delete: Vec<HashMap<String, AttributeValue>> = Vec::new();

    // ==========================================
    // 1. 対象レコードのキーを全て取得 (Query & ページネーション)
    // ==========================================
    loop {
        let mut query_builder = client
            .query()
            .table_name(table_name)
            // キー条件式の指定
            .key_condition_expression("#pk = :pk_val")
            .expression_attribute_names("#pk", pk_name)
            .expression_attribute_values(":pk_val", AttributeValue::S(pk_value.to_string()))
            // 削除に必要なキー情報のみを取得してデータ通信量を節約
            .projection_expression(format!("#pk, #sk"))
            .expression_attribute_names("#sk", sk_name);

        // 前回のページネーションの続きがあれば適用
        if let Some(start_key) = &exclusive_start_key {
            query_builder = query_builder.set_exclusive_start_key(Some(start_key.clone()));
        }

        let response = query_builder.send().await?;

        // 取得したアイテムのキー情報を保持
        if let Some(items) = response.items {
            for item in items {
                if let (Some(pk), Some(sk)) = (item.get(pk_name), item.get(sk_name)) {
                    let mut key_map = HashMap::new();
                    key_map.insert(pk_name.to_string(), pk.clone());
                    key_map.insert(sk_name.to_string(), sk.clone());
                    items_to_delete.push(key_map);
                }
            }
        }

        // last_evaluated_keyを確認してループの継続判定
        exclusive_start_key = response.last_evaluated_key;
        if exclusive_start_key.is_none() {
            break; 
        }
    }

    if items_to_delete.is_empty() {
        println!("削除対象のレコードは見つかりませんでした。");
        return Ok(());
    }

    // ==========================================
    // 2. 取得したキー情報をもとに一括削除 (BatchWriteItem)
    // ==========================================
    // BatchWriteItemは最大25件の制限があるため、chunks(25)で分割して処理
    for chunk in items_to_delete.chunks(25) {
        let mut write_requests = Vec::new();

        for key in chunk {
            let delete_request = DeleteRequest::builder().set_key(Some(key.clone())).build();
            let write_request = WriteRequest::builder()
                .delete_request(delete_request)
                .build();
            write_requests.push(write_request);
        }

        // 25件(またはそれ以下)のバッチで削除を実行
        client
            .batch_write_item()
            .request_items(table_name, write_requests)
            .send()
            .await?;
    }

    println!("{}件のレコードを削除しました。", items_to_delete.len());

    Ok(())
}
```

このコードでは生の `AttributeValue::S(...)` を利用してクエリを行っていますが、以前の文脈で触れたように `serde_dynamo::to_attribute_value` などを併用すれば、より構造体ベースでの柔軟な型の変換（手動構築の排除）も可能です 。ただし、削除のフェーズにおいては上記のように純粋な `HashMap<String, AttributeValue>` を利用するアプローチが最も直接的で効率的です。
