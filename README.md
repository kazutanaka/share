
struct SerialHash {
    pub serial,
    pub serial_hash,
}

- query_sessions(client: &Client, serial_hash: &str) -> Result<Vec<DynamoSession>>, Box<dyn std::error:Error>>
    - 機能 -- シリアルハッシュの発行中セッション（複数）取得
    - セッション開始API -- セッションオーバーフロー確認
    - 履歴クリアAPI -- マージ進行中の履歴クリアを防止
- create_session(client: &Client, serial_hash: &SerialHash, auth_user, token, internalid, device, display_name, expiry_minutes)
    - 機能 -- 新規セッション発行
    - セッション開始API -- セッションURL発行
- get_session(client: &Client, serial_hash: &str, session_id: &SessionId) -> Result<Option<DhynamoSession>, Box<dyn std::error::Error>>
    - 機能 -- シリアルハッシュとセッションでセッション情報取得
    - 履歴一覧API -- セッション有効性チェック
    - マージAPI -- mode==null：初回でマージモード判定処理、mode!=null：２回目以降
    - 進捗API -- セッション有効性チェック
    - 履歴クリアAPI --  セキュリティ機能
    - セッション終了API --  セッション有効性チェック、レスポンス生成ソース
- close_session(client: &Client, serial_hash: &SerialHash, session_id: &SessionId)
    - 機能 -- セッションを終了させる
    - セッション終了API -- セッション不足防止
- query_histories(client: &Client, serial_hash: &SerialHash, target: &Target) -> Result<Vec<DynamoHistory>, Box<dyn std::error::Error>>
    -  機能 -- シリアルハッシュとターゲットから履歴一覧を降順で洗い出す
    - 履歴一覧API -- 履歴一覧を取得
        - レスポンス生成に向けて serde_json を調査する
        - serde_json -- history/{syncid, target, created_at, device/{display_name, device}}
        - リスト出力
    - 履歴クリアAPI -- 全履歴のタイムスタンプID一覧を取得して削除に進む
- delete_histories(client: &Client, serial_hash: &SerialHash, target: &Target, timestamp_ids: &Vec<TimestampId>)
    - 機能 --  シリアルハッシュ、ターゲット､タイムスタンプID（複数）を削除
    - 履歴クリアAPI -- パス情報がなくなるので、履歴S3アクセス不可になる
- start_merging(client: &Client, serial_hash: &SerialHash, session_id: &SessionId, target: &target, base_syncid: &str, merge_mode: &str, merge_mode_reason: &str, merge_status: &str, merge_error: &str, completed_at: Option<DateTime<Utc>)
    -  機能 -- マージ開始時にモード、理由、ステータスを更新する
    - マージAPI -- mode=null => mode=XXX ：トランザクション更新
- update_merge_status(client: &Client, serial_hash: &SerialHash, session_id: &SessionId, 
    - マージワーカー
        - S3パスからセッションID、シリアルハッシュ、タイムスタンプIDを取り出す
        - シリアルハッシュ(PK)、セッションID(SK)でDynamoDBにアクセス可能
        - 更新カラム -- merge_status, merge_error, completed_at
