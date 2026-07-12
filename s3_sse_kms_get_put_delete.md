## SSE-KMSカスタマー管理鍵で暗号化されたS3をLambda, CloudFrontからGetObject, PutObject, DeleteObject する。

SSE-KMSの**カスタマー管理鍵（CMK）**で暗号化されたS3バケットに対して、LambdaやCloudFrontから安全にアクセスするためには、S3の権限だけでなく**KMSキーポリシーの正しい設定**が最大の肝になります。

全体像の理解を深めるために、必要な設定（IAM、KMSキーポリシー、バケットポリシー、CloudFront OAC）を整理しました。

---

## 1. AWS Lambda の設定

LambdaがS3に対して `GetObject`, `PutObject`, `DeleteObject` を行うには、**LambdaのIAMロール**にS3とKMS両方の権限が必要です。

### 必要なIAMポリシー (Lambda用)

Lambdaの実行ロールに以下のポリシーをアタッチします。

```json
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Sid": "S3Access",
            "Effect": "Allow",
            "Action": [
                "s3:GetObject",
                "s3:PutObject",
                "s3:DeleteObject"
            ],
            "Resource": "arn:aws:s3:::<あなたのバケット名>/*"
        },
        {
            "Sid": "KMSAccess",
            "Effect": "Allow",
            "Action": [
                "kms:Decrypt",
                "kms:GenerateDataKey"
            ],
            "Resource": "<カスタマー管理鍵のARN>"
        }
    ]
}

```

* `PutObject`（暗号化）の際には `kms:GenerateDataKey` が必要になります。
* `GetObject`（復号）の際には `kms:Decrypt` が必要になります。
* `DeleteObject` はオブジェクトのメタデータ削除のみであるため、通常KMS権限は不要ですが、S3の権限として必要です。

---

## 2. CloudFront の設定 (OACの利用)

CloudFrontからS3バケットへ直接 `GetObject` する場合、**OAC (Origin Access Control)** を使用します。現在、CloudFront OACはSSE-KMS（カスタマー管理鍵）による暗号化バケットからの読み込みに対応しています。

### 設定のポイント

1. CloudFrontのオリジン設定で「Origin access control settings (recommended)」を選択し、OACを作成・適用します。
2. CloudFrontがS3バケットおよびKMSキーにアクセスできるように、後述の**S3バケットポリシー**と**KMSキーポリシー**を調整します。

---

## 3. KMSキーポリシーの設定（超重要）

ここが一番の盲点になりやすいポイントです。カスタマー管理鍵（CMK）を利用する場合、**KMSキーポリシー側で明示的にアクセスを許可**しないと、たとえIAM側で許可していても拒否（Access Denied）されます。

KMSキーポリシーの `Statement` に以下を追加してください。

```json
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Sid": "AllowLambdaAccess",
            "Effect": "Allow",
            "Principal": {
                "AWS": "arn:aws:iam::<アカウントID>:role/<Lambdaの実行ロール名>"
            },
            "Action": [
                "kms:Decrypt",
                "kms:GenerateDataKey"
            ],
            "Resource": "*"
        },
        {
            "Sid": "AllowCloudFrontOACAccess",
            "Effect": "Allow",
            "Principal": {
                "Service": "cloudfront.amazonaws.com"
            },
            "Action": "kms:Decrypt",
            "Resource": "*",
            "Condition": {
                "ArnEquals": {
                    "aws:SourceArn": "arn:aws:cloudfront::<アカウントID>:distribution/<CloudFrontディストリビューションID>"
                }
            }
        }
    ]
}

```

* **Lambda向け**: `kms:GenerateDataKey` と `kms:Decrypt` を許可します。
* **CloudFront向け**: 読み込み（`GetObject`）だけなので `kms:Decrypt` のみでOKです。セキュリティを高めるため、`Condition` で特定のディストリビューションARNに絞り込みます。

---

## 4. S3 バケットポリシーの設定

最後に、S3バケット側でLambdaとCloudFrontからのリクエストを受け付けるようにポリシーを設定します（※Lambdaが同じAWSアカウント内にある場合、Lambda側のIAMポリシーだけで十分なケースもありますが、明示しておくと確実です）。

```json
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Sid": "AllowCloudFrontOAC",
            "Effect": "Allow",
            "Principal": {
                "Service": "cloudfront.amazonaws.com"
            },
            "Action": "s3:GetObject",
            "Resource": "arn:aws:s3:::<あなたのバケット名>/*",
            "Condition": {
                "StringEquals": {
                    "AWS:SourceArn": "arn:aws:cloudfront::<アカウントID>:distribution/<CloudFrontディストリビューションID>"
                }
            }
        },
        {
            "Sid": "AllowLambda",
            "Effect": "Allow",
            "Principal": {
                "AWS": "arn:aws:iam::<アカウントID>:role/<Lambdaの実行ロール名>"
            },
            "Action": [
                "s3:GetObject",
                "s3:PutObject",
                "s3:DeleteObject"
            ],
            "Resource": "arn:aws:s3:::<あなたのバケット名>/*"
        }
    ]
}

```

---

## 💡 トラブルシューティングのヒント

もし設定後に `Access Denied` が発生した場合は、以下の順番で確認してみてください。

1. **CloudFront経由でエラーになる場合**: KMSキーポリシーの `cloudfront.amazonaws.com` の設定（特にディストリビューションARN）が間違っていないか。
2. **LambdaのPutObjectでエラーになる場合**: LambdaのIAMロールおよびKMSキーポリシーに `kms:GenerateDataKey` が含まれているか（`kms:Encrypt` だけでは足りないことが多いです）。

## CloudFrontからもPutObjectでファイルアップロードをする場合、KMSキーポリシー、S3バケットポリシーのActionにs3:PutObjectを追加すれば良いですか。

方向性は非常に良いですが、1点だけ惜しいところがあります！

S3バケットポリシーに追加するのは `s3:PutObject` で正解ですが、KMSキーポリシーに追加するのはS3のアクションではなく、KMSのアクション（`kms:GenerateDataKey`）になります。

CloudFront（OAC）経由でS3へファイルをアップロード（PutObject）し、かつSSE-KMSで暗号化するためには、以下の3つの設定を変更する必要があります。

### 1. KMSキーポリシーの修正

ファイルを暗号化して保存するためには、データキーを生成する権限が必要です。CloudFrontからのアクセス許可に `kms:GenerateDataKey` を追加します。

```json
{
    "Sid": "AllowCloudFrontOACAccess",
    "Effect": "Allow",
    "Principal": {
        "Service": "cloudfront.amazonaws.com"
    },
    "Action": [
        "kms:Decrypt",
        "kms:GenerateDataKey" // ← これを追加
    ],
    "Resource": "*",
    "Condition": {
        "StringEquals": {
            "AWS:SourceArn": "arn:aws:cloudfront::<アカウントID>:distribution/<CloudFrontディストリビューションID>"
        }
    }
}

```

※ マルチパートアップロードを利用する場合は、S3の仕様上 `kms:Decrypt` も必要になるため、両方許可しておくのがベストプラクティスです。

### 2. S3バケットポリシーの修正

CloudFrontに対する許可アクションに `s3:PutObject` を追加します。

```json
{
    "Sid": "AllowCloudFrontOAC",
    "Effect": "Allow",
    "Principal": {
        "Service": "cloudfront.amazonaws.com"
    },
    "Action": [
        "s3:GetObject",
        "s3:PutObject" // ← これを追加
    ],
    "Resource": "arn:aws:s3:::<あなたのバケット名>/*",
    "Condition": {
        "StringEquals": {
            "AWS:SourceArn": "arn:aws:cloudfront::<アカウントID>:distribution/<CloudFrontディストリビューションID>"
        }
    }
}

```

### 3. CloudFrontのビヘイビア（Cache Behavior）の設定（重要）

ポリシーの設定だけでなく、CloudFront自体が `PUT` リクエストを受け付けるように設定を変更する必要があります。

1. CloudFrontのコンソールから対象のディストリビューションを開く
2. **Behaviors**（ビヘイビア）タブから、対象のパス（アップロード先のパス）の設定を編集
3. **Allowed HTTP methods** を `GET, HEAD, OPTIONS, PUT, POST, PATCH, DELETE` に変更する
4. （推奨）アップロード用のパスはキャッシュさせないように、**Cache policy** を `CachingDisabled` に設定する

これで、CloudFrontを経由したセキュアな `PutObject`（SSE-KMS暗号化付き）が可能になります！

## RustからSSE-KMS暗号化されたS3オブジェクトをGetObjectするコード、PutObjectするコード、DeleteObjectするコード。

Rustの `aws-sdk-s3` クレートを使用して、SSE-KMS（カスタマー管理鍵）で暗号化されたS3オブジェクトを操作（`PutObject`, `GetObject`, `DeleteObject`）するサンプルコードです。

### 重要なポイント

* **PutObject (アップロード)**: S3バケット側にデフォルトのKMS暗号化設定がされている場合は指定不要で自動暗号化されますが、**コードから明示的にKMSキーを指定**することも可能です。以下のサンプルでは明示的に指定する方法を記載します。
* **GetObject (ダウンロード)**: S3がオブジェクトのメタデータから自動的にKMSキーを特定して復号するため、**コード内でKMSキーIDや暗号化方式を指定する必要はありません**（実行環境のIAMロールに `kms:Decrypt` 権限があればシームレスに取得できます）。
* **DeleteObject (削除)**: オブジェクトのメタデータを削除するだけなので、KMSに関連するパラメータは不要です。

### 1. `Cargo.toml` の設定

必要な依存関係を追加します（バージョンは適宜最新のものに調整してください）。

```toml
[dependencies]
aws-config = "1.1"
aws-sdk-s3 = "1.4"
tokio = { version = "1", features = ["full"] }

```

### 2. サンプルコード (`src/main.rs`)

```rust
use aws_config::BehaviorVersion;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::ServerSideEncryption;
use aws_sdk_s3::Client;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 環境変数やプロファイルからAWS設定を読み込み
    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let client = Client::new(&config);

    let bucket_name = "your-bucket-name";
    let object_key = "example.txt";
    // カスタマー管理鍵のARN または エイリアスを指定
    let kms_key_arn = "arn:aws:kms:ap-northeast-1:123456789012:key/xxxx-xxxx-xxxx-xxxx"; 
    
    // アップロードするローカルファイル
    let upload_file_path = Path::new("local_example.txt");
    // ダウンロード先のローカルファイル
    let download_file_path = "downloaded_example.txt";

    // 1. PutObject (SSE-KMS暗号化を指定してアップロード)
    println!("--- PutObject ---");
    put_object_with_kms(&client, bucket_name, object_key, upload_file_path, kms_key_arn).await?;

    // 2. GetObject (ダウンロード)
    println!("--- GetObject ---");
    get_object(&client, bucket_name, object_key, download_file_path).await?;

    // 3. DeleteObject (削除)
    println!("--- DeleteObject ---");
    delete_object(&client, bucket_name, object_key).await?;

    Ok(())
}

/// SSE-KMSカスタマー管理鍵を明示してファイルをアップロードする
async fn put_object_with_kms(
    client: &Client,
    bucket: &str,
    key: &str,
    file_path: &Path,
    kms_key_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // 非同期ストリームとしてファイルを読み込む（メモリを圧迫しない）
    let body = ByteStream::from_path(file_path).await?;

    let _response = client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(body)
        // SSE-KMS方式を指定
        .server_side_encryption(ServerSideEncryption::AwsKms)
        // 使用するKMSキーのARNを指定
        .ssekms_key_id(kms_key_id)
        .send()
        .await?;

    println!("PutObject成功: s3://{}/{} にアップロードしました。", bucket, key);
    Ok(())
}

/// オブジェクトをダウンロードする (KMSキーの指定は不要)
async fn get_object(
    client: &Client,
    bucket: &str,
    key: &str,
    dest_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // GetObjectリクエスト。暗号化されているかどうかに関わらず、
    // IAM権限(s3:GetObject, kms:Decrypt)があればS3が自動で復号して返します。
    let response = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await?;

    // レスポンスのByteStreamをAsyncReadに変換
    let mut body_reader = response.body.into_async_read();
    
    // 保存先のファイルを非同期で作成
    let mut file = tokio::fs::File::create(dest_path).await?;
    
    // ストリームからファイルへデータを書き込む
    tokio::io::copy(&mut body_reader, &mut file).await?;

    println!("GetObject成功: {} に保存しました。", dest_path);
    Ok(())
}

/// オブジェクトを削除する
async fn delete_object(
    client: &Client,
    bucket: &str,
    key: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    client
        .delete_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await?;

    println!("DeleteObject成功: s3://{}/{} を削除しました。", bucket, key);
    Ok(())
}

```

### 実行時の注意点

1. **テスト用ファイルの準備**: 実行前にカレントディレクトリに `local_example.txt` という名前のファイルを作成しておいてください。
2. **S3バケットのデフォルト暗号化との関係**:
* S3バケット自体に「デフォルトの暗号化（SSE-KMS）」が設定されており、そこにKMSキーが指定されている場合、`put_object_with_kms` 関数内の `.server_side_encryption()` や `.ssekms_key_id()` の呼び出しを**省略**しても、バケットのデフォルト設定に従って自動的にSSE-KMSで暗号化されます。
* バケットのデフォルト設定とは異なる特定のKMSキーを使用したい場合や、設定をコードで強制したい場合は上記サンプルのように明示的に指定します。

## SSE-KMSカスタマー管理鍵の場合、put_object()にssekms_key_id()を省略すると、どのようなエラーになりますか。

SSE-KMSを利用する際、`put_object()` の呼び出しで `.ssekms_key_id()` を省略した場合、**実は必ずしもエラーになるとは限りません**。

S3バケット側の設定や、リクエストの他のパラメータによって、以下の**3つのパターンのいずれか**の挙動になります。最も注意すべきは、エラーにならずに「意図しない鍵」で暗号化されてしまうケースです。

### パターン1: AWS管理鍵（`aws/s3`）で暗号化されて成功する（一番ありがちな罠）

コード内で以下のように「KMSを使う」ことだけを指定し、キーIDを省略した場合です。

```rust
.server_side_encryption(ServerSideEncryption::AwsKms)
// .ssekms_key_id(...) を省略

```

この場合、エラーにはならずアップロードは**成功**します。しかし、S3は指定がないと判断し、アカウントに用意されている無料の**AWS管理鍵（`aws/s3`）を自動的に使用**して暗号化してしまいます。「カスタマー管理鍵（CMK）で暗号化する」という本来の要件を満たせなくなるため、注意が必要です。

### パターン2: バケットのデフォルト暗号化設定が適用されて成功する

暗号化方式もキーIDも両方省略した場合です。

```rust
// .server_side_encryption(...) も省略
// .ssekms_key_id(...) も省略

```

この場合も**成功**します。S3バケット自体の「デフォルトの暗号化」設定が適用されます。バケットのデフォルト暗号化に該当のカスタマー管理鍵が設定されていれば、意図した通りにカスタマー管理鍵で暗号化されます。

### パターン3: `Access Denied` エラーになる（バケットポリシーで強制している場合）

セキュリティ要件の厳しい環境では、S3バケットポリシーで「特定のカスタマー管理鍵を明示的に指定しないとアップロードを許可しない」という設定（`s3:x-amz-server-side-encryption-aws-kms-key-id` 条件キーを使用した制限）を行っていることがあります。

この設定がされているバケットに対して `.ssekms_key_id()` を省略すると、S3がリクエストを拒否し、**`AccessDenied` (HTTP 403) エラー** になります。

Rustの `aws-sdk-s3` では、以下のようなエラーとしてキャッチされます。

```rust
// 返り値の Result が Err となり、中身が SdkError になる
Err(aws_sdk_s3::error::SdkError::ServiceError(err))

```

エラーを出力すると、以下のように `Access Denied` であることが示されます。
`ServiceError: PutObjectError: AccessDenied: Access Denied`

---

### まとめ

* **キーIDを省略すると、デフォルトではエラーにならず、AWS管理鍵（`aws/s3`）が使われてしまう危険性がある。**
* **確実にカスタマー管理鍵を使わせたい場合は、S3バケットポリシーで指定を強制させる（これにより省略時は `Access Denied` エラーにできる）。**
* **バケットにデフォルトの暗号化が正しく設定されていれば、Rust側では一切指定しなくても（省略しても）正しく暗号化される。**

## Ssekms_key_id()のみ指定して、server_side_encryption()を省略した場合、どのような結果になりますか。

`.ssekms_key_id()` のみを指定し、`.server_side_encryption()`（暗号化方式の指定）を省略した場合、アップロードは成功せず、**S3側からエラー（HTTP 400 Bad Request）が返され失敗**します。

### なぜエラーになるのか？

Amazon S3のAPIの仕様によるものです。S3に対して「この特定のカスタマー管理鍵を使って暗号化してほしい（KMSキーIDの指定）」とパラメータを渡す場合、前提として「暗号化方式にはSSE-KMSを使用する」という宣言を**必ずセットで送る必要があります**。

APIの基盤となるHTTPリクエストヘッダーの仕様としては以下のようになっています。

* キーIDの指定: `x-amz-server-side-encryption-aws-kms-key-id: <キーのARN>`
* 暗号化方式の指定: `x-amz-server-side-encryption: aws:kms` **（← これが欠落していると判断される）**

### 発生するエラーの種類

Rustの `aws-sdk-s3` クレートでこのリクエストを実行すると、APIが不正な引数を受け取ったことを示す `InvalidArgument` エラーとしてキャッチされます。

エラーメッセージを印字すると、以下のように「KMSキーIDを指定するリクエストでは、暗号化方式として `aws:kms` の指定が必須である」という内容が出力されます。

```rust
// 返り値は Err(aws_sdk_s3::error::SdkError::ServiceError(err)) となる
ServiceError: PutObjectError: InvalidArgument: Requests specifying Server Side Encryption with AWS KMS key Ids must provide header x-amz-server-side-encryption with value aws:kms

```

### まとめ

「KMSキーを指定したつもりがAWS管理鍵になっていた」といった前述の事故（省略による自動フォールバック）が起きるのは `.server_side_encryption()` だけを指定した時です。

逆に **`.ssekms_key_id()` を指定する時は、必ず `.server_side_encryption(ServerSideEncryption::AwsKms)` とセットで記述**しなければならないと覚えておくと安全です。

## Rustコード put_object_with_kms() の kms_key_id 引数を Option<&str> にして、client.put_object() 呼び出し後の fluent style はそのままで、Some, Noneで場合分けしてメソッド追加、省略したい。

RustのAWS SDKが採用している fluent builder（メソッドチェーン）で、条件によってパラメータを追加したり省略したりする場合、主に **2つのアプローチ** があります。

SDKが用意している `Option` を直接受け取れる **`set_XXX` メソッドを使う方法**（最もスッキリ書けます）と、**ビルダー変数をシャドーイング（またはミュータブルに）して分岐させる方法**です。

それぞれのサンプルコードを提示します。

### アプローチ1: `set_XXX` メソッドを使う（おすすめ）

AWS SDKの各ビルダーには、通常のメソッド（例: `ssekms_key_id`）に加えて、`Option` 型を直接受け取る `set_` プレフィックスの付いたメソッド（例: `set_ssekms_key_id`）が必ず用意されています。

これを利用すると、`if` 文でチェーンを断ち切ることなく、美しい fluent style を維持したまま実装できます。

```rust
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::ServerSideEncryption;
use aws_sdk_s3::Client;
use std::path::Path;

async fn put_object_with_kms(
    client: &Client,
    bucket: &str,
    key: &str,
    file_path: &Path,
    kms_key_id: Option<&str>, // Option<&str> に変更
) -> Result<(), Box<dyn std::error::Error>> {
    let body = ByteStream::from_path(file_path).await?;

    // kms_key_id が Some なら AwsKms をセットし、None なら None にする
    let sse_type = kms_key_id.map(|_| ServerSideEncryption::AwsKms);
    // set_ssekms_key_id は Option<String> を要求するため変換
    let key_id_string = kms_key_id.map(|s| s.to_string());

    let _response = client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(body)
        // ここから条件付き付与
        .set_server_side_encryption(sse_type)
        .set_ssekms_key_id(key_id_string)
        // 送信
        .send()
        .await?;

    println!("PutObject成功: s3://{}/{}", bucket, key);
    Ok(())
}

```

### アプローチ2: Builderを変数に受けて再代入する

「どうしても `if let` 等を使って条件分岐のブロックを明示したい」という場合は、Builderのインスタンスを一度変数（ミュータブル、もしくはシャドーイング）に受けることで対応できます。

Fluent builder の各メソッドは `self` を消費して新しい `self` を返すため、以下のように上書きしていくことができます。

```rust
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::ServerSideEncryption;
use aws_sdk_s3::Client;
use std::path::Path;

async fn put_object_with_kms(
    client: &Client,
    bucket: &str,
    key: &str,
    file_path: &Path,
    kms_key_id: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let body = ByteStream::from_path(file_path).await?;

    // Builder を変数 request に束縛（mut にする）
    let mut request = client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(body);

    // Option の中身がある場合のみチェーンを追加して再代入
    if let Some(key_id) = kms_key_id {
        request = request
            .server_side_encryption(ServerSideEncryption::AwsKms)
            .ssekms_key_id(key_id);
    }

    // 最後に send() を呼ぶ
    let _response = request.send().await?;

    println!("PutObject成功: s3://{}/{}", bucket, key);
    Ok(())
}

```

### まとめ

* **アプローチ1 (`set_XXX`)**: コードの行数が増えず、一つの大きなメソッドチェーンとして宣言的に書けるため、RustにおけるAWS SDKのベストプラクティスとして最も推奨されます。
* **アプローチ2 (再代入)**: 設定するパラメータの数が多く、分岐のロジックが複雑（例: Aの時はパラメータXとYを追加し、Bの時はZを追加するなど）な場合に可読性が上がります。


## S3オブジェクト暗号化がSSE-S3だった場合、kms_key_idを指定するとエラーになりますか？

はい、**エラーになります**。

S3オブジェクトの暗号化方式として **SSE-S3**（Rust SDKでは `ServerSideEncryption::Aes256`）を指定しつつ、KMSキーID（`.ssekms_key_id()`）を同時に指定した場合、アップロードのリクエストはS3側で拒否され失敗します。

### なぜエラーになるのか？

**SSE-S3** は、Amazon S3側が完全に自動で鍵の生成と管理を行う方式（AES-256）です。そのため、ユーザー側で特定の鍵（KMSキーなど）を指定することは仕様上できません。

S3のAPIは、リクエストの中に「KMSキーID」が含まれているのを発見すると、「このリクエストはSSE-KMSを利用しようとしている」と解釈します。しかし、同時に指定されている暗号化方式が `aws:kms`（SSE-KMS）ではなく `AES256`（SSE-S3）であるため、**設定内容が矛盾している**と判断してエラーを返します。

### 発生するエラーの種類

Rustの `aws-sdk-s3` を使ってこのリクエストを実行した場合、S3 APIから HTTP 400 Bad Request が返され、以下のような `InvalidArgument` エラーが発生します。

```text
ServiceError: PutObjectError: InvalidArgument: Requests specifying Server Side Encryption with AWS KMS key Ids must provide header x-amz-server-side-encryption with value aws:kms

```

（直訳：「AWS KMSキーIDを指定したサーバーサイド暗号化リクエストでは、暗号化方式として `aws:kms` を提供しなければなりません」）

### 実装上の注意点

前の質問で登場した `Option<&str>` を使って fluent style で場合分けをする実装において、もし「KMSキーIDが渡されなかった場合は SSE-S3 にフォールバックする」という仕様にしたい場合は、以下の組み合わせが**絶対に混ざらないように**設計する必要があります。

* **SSE-KMSを使う場合:** * 暗号化方式: `ServerSideEncryption::AwsKms`
* キーID: `Some("arn:...")`


* **SSE-S3を使う場合:**
* 暗号化方式: `ServerSideEncryption::Aes256`
* キーID: **`None` （絶対に指定しない）**

